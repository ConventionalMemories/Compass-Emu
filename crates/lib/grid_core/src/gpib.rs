//! TMS9914A GPIB controller + IEEE-488 bus + GRiD 2101/2102 disk drives.
//!
//! The bus is modelled one byte at a time instead of per handshake line: a byte the 9914 sends
//! reaches every acceptor `send_clocks` later, and a byte a talker sends is latched into the
//! 9914 data-in register `talk_clocks` after the previous one was read. Register semantics
//! follow MAME tms9914.cpp, as used by GRiD's GPIB driver (splitboard.gpib.asm) and boot PROM:
//!   aux commands rst/clrrst, sic/clrsic, ton/diston, lon/dislon, gts, tca, tcs, feoi;
//!   INT0 BI/BO/END, INT1 ERR/SRQ, bus status ATN/DAV/NDAC/NRFD/EOI/SRQ.
//! The drive protocol follows MAME grid2102.cpp (10-byte request, 512-byte sectors,
//! commands 0 init, 1 status, 4 read, 5 write, 17 format, SRQ + serial poll 0x4F when done).
//!
//! Timing constants are guesses until measured on a real 1101 + 2101/2102.

use std::collections::VecDeque;

use crate::box2101::{Box2101, Media};
use crate::box2102::{Box2102, HostLines, Lines};

// INT0 bits
pub const INT0_MAC: u8 = 0x01;
pub const INT0_END: u8 = 0x08;
pub const INT0_BO: u8 = 0x10;
pub const INT0_BI: u8 = 0x20;
pub const INT0_INT1: u8 = 0x40;
pub const INT0_INT0: u8 = 0x80;
// INT1 bits
pub const INT1_IFC: u8 = 0x01;
pub const INT1_SRQ: u8 = 0x02;
pub const INT1_ERR: u8 = 0x40;

const CMD_SPE: u8 = 0x18;
const CMD_SPD: u8 = 0x19;
const SECTOR: usize = 512;

#[derive(Copy, Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
enum Ctrl {
    /// Not controller in charge.
    Idle,
    /// Controller active, ATN asserted.
    Active,
    /// Controller standby, ATN released.
    Standby,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum DriveKind {
    /// 2101 hard disk, GPIB address 4.
    Hdd2101,
    /// 2101 floppy, GPIB address 5.
    Floppy2101,
    /// 2102 portable floppy, GPIB address 6.
    Floppy2102,
}

impl DriveKind {
    pub fn address(self) -> u8 {
        match self {
            DriveKind::Hdd2101 => 4,
            DriveKind::Floppy2101 => 5,
            DriveKind::Floppy2102 => 6,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
enum DriveState {
    Idle,
    Reading,
    Writing,
    WritingWait,
    Formatting,
}

/// GRiD 2101/2102 drive, byte-level port of MAME's grid210x_device.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Drive {
    pub kind: DriveKind,
    /// Not in saved states: the image is attached again after loading one.
    #[serde(skip)]
    pub image: Vec<u8>,
    pub dirty: bool,
    listening: bool,
    talking: bool,
    serial_polling: bool,
    pub srq: bool,
    spoll: u8,
    state: DriveState,
    sector: u32,
    io_size: u16,
    rx: Vec<u8>,
    tx: VecDeque<u8>,
    /// Clock at which the pending read/write/format completes.
    done_at: Option<u64>,
    /// Command delay in CPU clocks (MAME: 5 ms floppy, 150 us hard disk).
    pub delay_clocks: u64,
    /// Hard disk mechanics: cylinder under the heads and the pages in the box's read-ahead cache.
    #[serde(default)]
    cyl: u32,
    /// A multi-page read's reply: each 512-byte page ends with EOI (GRiD's driver takes one
    /// page a reply), and what is left is dropped when the box is addressed again.
    #[serde(default)]
    chunked: bool,
    /// Control byte of the write in progress: 1 = set status (geometry), 2 = upload to a box
    /// RAM buffer. Neither lands on the disk.
    #[serde(default)]
    write_ctrl: u8,
    /// Read-ahead cache: (page, clock at which the box has it).
    #[serde(default)]
    ra_cache: Vec<(u32, u64)>,
    /// The box answers a write once it has the data and writes it while taking the next command.
    #[serde(default)]
    busy_until: u64,
    /// Seek and rotation timing for the hard disk (off in fast mode, or with HD_NOMECH set).
    #[serde(default = "yes")]
    pub mechanics: bool,
    #[serde(skip)]
    pub log: Vec<String>,
    /// Low-level 2102: the box's own 8051 firmware answers instead of the protocol model.
    #[serde(default)]
    pub lle: Option<Box<Box2102>>,
    /// Low-level 2101 (on the box's hard disk drive): the board's 8088 runs the real firmware
    /// and answers on both of the box's addresses.
    #[serde(default)]
    pub lle2101: Option<Box<Box2101>>,
    /// This 2101 floppy is answered by the low-level 2101 box (on the hard disk drive).
    #[serde(default)]
    pub served: bool,
    /// The low-level 2101's light for this drive (kept up to date by the bus).
    #[serde(default)]
    served_busy: bool,
    /// GPIB address (the kind's usual one unless set: GRiD-OS knows 4 Hard Disk, 5 Floppy
    /// Disk, 6 Portable Floppy, 12 Extra Hard Disk, 13 Extra Floppy Disk).
    #[serde(default)]
    pub addr: u8,
    /// Always the protocol model, even when firmware for the box is there.
    #[serde(default)]
    pub protocol: bool,
    /// Firmware for this drive's box instead of the bus-wide one (`rom_2102`, `rom_2101`).
    #[serde(skip)]
    pub rom: Option<Vec<u8>>,
}

/// GPIBLOG set: print drive commands, SRQs, serial polls and ends of data on stderr.
fn gpib_log() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("GPIBLOG").is_some())
}

fn yes() -> bool {
    true
}

/// 2101 hard disk mechanics (ST-225 behind a Xebec S1410, measured with the Disk Toolkit on a
/// real 2101): 3600 rpm, 17 sectors a track, 4 heads, formatted with interleave 5; the box reads
/// ahead up to 6 pages to the end of the track into its cache, and any write drops the cache.
mod hd {
    pub const CPU_HZ: u64 = 5_016_000;
    pub const ROT: u64 = CPU_HZ / 60;
    pub const SPT: u32 = 17;
    pub const HEADS: u32 = 4;
    pub const INTERLEAVE: u64 = 5;
    pub const READ_AHEAD: u32 = 6;
    /// Box firmware time to take a command and set up the S1410 (and to answer from its cache).
    pub const OVERHEAD: u64 = CPU_HZ / 2000;
    pub const CACHE_HIT: u64 = CPU_HZ * 3 / 10_000;

    pub fn slot() -> u64 {
        ROT / SPT as u64
    }

    /// Seek time in CPU clocks for `d` cylinders, interpolated from a real 2101 (ST-225): Disk
    /// Toolkit (1 cylinder 26.4 ms more than no seek, full stroke about 150 ms) and GRiDBench
    /// 2.1b (e46 steps of 1/16 = 38 cylinders 122.5 ms a read, e44 random pages 130.3 ms).
    pub fn seek(d: u32) -> u64 {
        const T: [(u32, u64); 4] = [(0, 0), (1, 26_400), (38, 80_000), (611, 148_000)];
        let d = d.min(611);
        let i = T.iter().position(|e| e.0 >= d).unwrap_or(T.len() - 1);
        let us = if i == 0 {
            0
        }
        else {
            let ((d0, t0), (d1, t1)) = (T[i - 1], T[i]);
            t0 + (t1 - t0) * (d - d0) as u64 / (d1 - d0) as u64
        };
        us * CPU_HZ / 1_000_000
    }

    /// Time at or after `t` when logical sector `s` of a track starts under the head.
    pub fn sector_start(t: u64, s: u32) -> u64 {
        let target = (s as u64 * INTERLEAVE % SPT as u64) * slot();
        let a = t % ROT;
        t + (target + ROT - a) % ROT
    }
}

/// Floppy mechanics (2101 floppy and 2102: WD279x, 300 rpm, 9 sectors of 512 bytes, 2 sides,
/// 40 cylinders), fitted to GRiDBench 2.1b on a real 2102 with a Gotek (e61-e66 within about
/// 9%): interleave 5, side 1 skewed by 7 sectors, 3 ms a cylinder + 30 ms settle, and the
/// 2102's 8051 is slow: about 40 ms to take a command and 60 us a byte to send. The env values
/// FD_ROT/FD_IL/FD_SKEW/FD_STEP/FD_SETTLE/FD_OV (us or sectors) and FD_TALK (clocks a byte)
/// override them for fitting.
mod fd {
    use super::hd::CPU_HZ;
    pub const SPT: u32 = 9;
    pub const HEADS: u32 = 2;

    pub struct P {
        pub rot: u64,
        pub il: u32,
        pub skew: u32,
        pub step: u64,
        pub settle: u64,
        pub talk: u64,
        pub ov: u64,
    }

    fn env(k: &str, d: u64) -> u64 {
        std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
    }

    pub fn p() -> &'static P {
        static P_: std::sync::OnceLock<P> = std::sync::OnceLock::new();
        P_.get_or_init(|| P {
            rot: env("FD_ROT", 200_000) * CPU_HZ / 1_000_000,
            il: env("FD_IL", 5) as u32,
            skew: env("FD_SKEW", 7) as u32,
            step: env("FD_STEP", 3_000) * CPU_HZ / 1_000_000,
            settle: env("FD_SETTLE", 30_000) * CPU_HZ / 1_000_000,
            talk: env("FD_TALK", 300),
            ov: env("FD_OV", 40_000) * CPU_HZ / 1_000_000,
        })
    }

    pub fn slot() -> u64 {
        p().rot / SPT as u64
    }

    /// Time at or after `t` when page `page`'s sector starts under the head.
    pub fn sector_start(t: u64, page: u32) -> u64 {
        let p = p();
        let s = page % SPT;
        let h = (page / SPT) % HEADS;
        let at = ((s * p.il + h * p.skew) % SPT) as u64 * slot();
        let a = t % p.rot;
        t + (at + p.rot - a) % p.rot
    }
}

impl Drive {
    pub fn new(kind: DriveKind, image: Vec<u8>) -> Drive {
        let delay_clocks = match kind {
            DriveKind::Hdd2101 => 750,
            _ => 25_000,
        };
        Drive {
            kind,
            image,
            dirty: false,
            listening: false,
            talking: false,
            serial_polling: false,
            srq: false,
            spoll: 0,
            state: DriveState::Idle,
            sector: 0,
            io_size: 0,
            rx: Vec::new(),
            tx: VecDeque::new(),
            done_at: None,
            delay_clocks,
            cyl: 0,
            ra_cache: Vec::new(),
            chunked: false,
            write_ctrl: 0,
            busy_until: 0,
            mechanics: true,
            log: Vec::new(),
            lle: None,
            lle2101: None,
            served: false,
            served_busy: false,
            addr: kind.address(),
            protocol: false,
            rom: None,
        }
    }

    /// A drive at GPIB address `addr`.
    pub fn at(kind: DriveKind, addr: u8, image: Vec<u8>) -> Drive {
        let mut d = Drive::new(kind, image);
        d.addr = addr;
        d
    }

    /// The drive's GPIB address (older saved states have none: the kind's usual one).
    pub fn address(&self) -> u8 {
        if self.addr == 0 {
            self.kind.address()
        }
        else {
            self.addr
        }
    }

    fn mechanics(&self) -> bool {
        self.mechanics && self.kind == DriveKind::Hdd2101 && std::env::var_os("HD_NOMECH").is_none()
    }

    /// A real floppy (a folder drive is larger and stays fast).
    fn floppy_mechanics(&self) -> bool {
        self.mechanics && self.kind != DriveKind::Hdd2101 && !self.image.is_empty() && self.image.len() <= 720 * SECTOR && std::env::var_os("FD_NOMECH").is_none()
    }

    /// A low-level box answers for this drive (the protocol model stays quiet).
    pub fn low_level(&self) -> bool {
        self.lle.is_some() || self.lle2101.is_some() || self.served
    }

    /// The low-level 2101's four-character display, if this drive has the box.
    pub fn display(&self) -> Option<String> {
        self.lle2101.as_ref().map(|b| b.display())
    }

    /// A command is being carried out (for a drive activity light).
    pub fn busy(&self) -> bool {
        if let Some(l) = &self.lle {
            return l.motor_on();
        }
        if let Some(b) = &self.lle2101 {
            return b.hd_busy();
        }
        if self.served {
            return self.served_busy;
        }
        self.state != DriveState::Idle || self.done_at.is_some()
    }

    /// Clocks a byte when this drive talks: the 2102's 8051 bit-bangs GPIB.
    pub fn talk_clocks(&self) -> Option<u64> {
        (self.kind == DriveKind::Floppy2102).then(|| fd::p().talk)
    }

    /// When a floppy has read or written `pages` pages from `sector` (one WD279x command a page).
    fn floppy_done(&mut self, now: u64, sector: u32, pages: u32) -> u64 {
        let mut t = now + if self.kind == DriveKind::Floppy2102 { fd::p().ov } else { hd::OVERHEAD };
        for p in sector..sector + pages {
            let cyl = p / (fd::SPT * fd::HEADS);
            if cyl != self.cyl {
                t += cyl.abs_diff(self.cyl) as u64 * fd::p().step + fd::p().settle;
                self.cyl = cyl;
            }
            t = fd::sector_start(t, p) + fd::slot();
        }
        t
    }

    /// When a read of `size` bytes from page `sector` is ready to send.
    fn read_done(&mut self, now: u64, sector: u32, size: u16) -> u64 {
        if self.floppy_mechanics() {
            return self.floppy_done(now, sector, (size as u32).div_ceil(SECTOR as u32).max(1));
        }
        if !self.mechanics() {
            return now + self.delay_clocks;
        }
        let pages = (size as u32).div_ceil(SECTOR as u32).max(1);
        let mut t = now + hd::OVERHEAD;
        let mut p = sector;
        let end = sector + pages;
        while p < end {
            if let Some(&(_, ready)) = self.ra_cache.iter().find(|e| e.0 == p) {
                t = t.max(ready) + hd::CACHE_HIT;
                p += 1;
                continue;
            }
            // One S1410 read from p to the end of the track, at least the rest of the request or
            // the read-ahead count. The box answers once it has the requested pages and goes on
            // reading ahead meanwhile.
            let track_end = (p / hd::SPT + 1) * hd::SPT;
            let n = (end - p).max(hd::READ_AHEAD).min(track_end - p);
            t = self.seek_to(t.max(self.busy_until), p);
            let start = hd::sector_start(t, p % hd::SPT);
            let ready = |k: u32| start + (k as u64 * hd::INTERLEAVE + 1) * hd::slot();
            self.ra_cache.clear();
            self.ra_cache.extend((0..n).map(|k| (p + k, ready(k))));
            self.busy_until = ready(n - 1);
            let got = (end - p).min(n);
            t = ready(got - 1);
            p += got;
        }
        t
    }

    /// When a write of `len` bytes to page `sector` (data all received at `now`) is done.
    fn write_done(&mut self, now: u64, sector: u32, len: usize) -> u64 {
        if self.floppy_mechanics() {
            return self.floppy_done(now, sector, len.div_ceil(SECTOR).max(1) as u32);
        }
        if !self.mechanics() {
            return now + self.delay_clocks;
        }
        self.ra_cache.clear();
        let pages = len.div_ceil(SECTOR).max(1) as u32;
        let answer = now + hd::OVERHEAD;
        let mut t = answer.max(self.busy_until);
        let mut p = sector;
        let end = sector + pages;
        while p < end {
            let track_end = (p / hd::SPT + 1) * hd::SPT;
            let n = (end - p).min(track_end - p);
            t = self.seek_to(t, p);
            let start = hd::sector_start(t, p % hd::SPT);
            t = start + ((n as u64 - 1) * hd::INTERLEAVE + 1) * hd::slot();
            p += n;
        }
        self.busy_until = t;
        answer
    }

    fn seek_to(&mut self, t: u64, page: u32) -> u64 {
        let cyl = page / (hd::SPT * hd::HEADS);
        let d = cyl.abs_diff(self.cyl);
        self.cyl = cyl;
        t + hd::seek(d)
    }

    /// 7-byte error reply: error word, drive, page.
    fn error_block(code: u16, page: u32) -> [u8; 7] {
        let mut b = [0u8; 7];
        b[0..2].copy_from_slice(&code.to_le_bytes());
        b[3..7].copy_from_slice(&page.to_le_bytes());
        b
    }

    fn status_block(&self) -> [u8; 52] {
        let (count, bitmap, superblock, min_dir, name, spt, tpc): (u16, u16, u16, u16, &str, u16, u16) = match self.kind {
            DriveKind::Hdd2101 => (
                (self.image.len() / SECTOR).min(0xFFFF) as u16,
                0x2400,
                0x2420,
                10,
                "COMPASS-EMU HARD DISK",
                17,
                4,
            ),
            // A folder drive (hostdir) is larger than a floppy: report its size; the root
            // directory follows the longer bitmap.
            _ if self.image.len() / SECTOR > 720 => {
                let n = (self.image.len() / SECTOR).min(0xFFFF) as u32;
                (n as u16, 0x120, crate::hostdir::root_fid(n) as u16, 1, "COMPASS-EMU PC FOLDER", 9, 2)
            }
            _ => (720, 0x120, 0x121, 1, "48 TPI DS DD FLOPPY 30237-00", 9, 2),
        };
        let mut o = [0u8; 52];
        o[0..2].copy_from_slice(&(SECTOR as u16).to_le_bytes());
        o[2..4].copy_from_slice(&504u16.to_le_bytes());
        o[4..6].copy_from_slice(&count.to_le_bytes());
        o[6] = 1;
        o[7..9].copy_from_slice(&bitmap.to_le_bytes());
        o[9..11].copy_from_slice(&superblock.to_le_bytes());
        o[11..13].copy_from_slice(&min_dir.to_le_bytes());
        o[13] = 0;
        for i in 0..32 {
            o[14 + i] = *name.as_bytes().get(i).unwrap_or(&b' ');
        }
        o[46..48].copy_from_slice(&(SECTOR as u16).to_le_bytes());
        o[48..50].copy_from_slice(&spt.to_le_bytes());
        o[50..52].copy_from_slice(&tpc.to_le_bytes());
        o
    }

    /// Command byte (ATN asserted).
    fn command(&mut self, b: u8) {
        if self.low_level() {
            return;
        }
        let group = b & 0xE0;
        let addr = b & 0x1F;
        if group == 0x20 {
            self.rx.clear();
            self.listening = addr == self.address();
            if self.listening && self.chunked {
                self.tx.clear();
                self.chunked = false;
            }
            self.talking = false;
        }
        else if group == 0x40 {
            self.rx.clear();
            self.talking = addr == self.address();
            self.listening = false;
        }
        else if b == CMD_SPE {
            self.serial_polling = true;
        }
        else if b == CMD_SPD {
            self.serial_polling = false;
        }
    }

    /// Data byte while listening.
    fn data(&mut self, b: u8, eoi: bool, now: u64) {
        if self.low_level() {
            return;
        }
        self.rx.push(b);
        if eoi {
            self.accept(now);
            self.rx.clear();
        }
    }

    fn accept(&mut self, now: u64) {
        match self.state {
            DriveState::Idle => {
                if self.rx.len() != 10 {
                    self.log.push(format!("ignored {}-byte message", self.rx.len()));
                    return;
                }
                let cmd = self.rx[0];
                let sector = u32::from_le_bytes([self.rx[3], self.rx[4], self.rx[5], self.rx[6]]);
                let size = u16::from_le_bytes([self.rx[7], self.rx[8]]);
                self.log.push(format!("cmd {} sector {} size {}", cmd, sector, size));
                if gpib_log() {
                    eprintln!("GPIB {} {} {} {} @{}", now, cmd, sector, size, self.address());
                }
                match cmd {
                    0 => self.tx.extend([0u8; 7]),
                    1 => {
                        let s = self.status_block();
                        self.tx.extend(s);
                    }
                    4 => {
                        self.sector = sector;
                        // The 2102 reads one page a command (its ROM moves exactly 512 bytes).
                        let size = if self.kind == DriveKind::Floppy2102 { size.min(SECTOR as u16) } else { size };
                        self.io_size = size;
                        self.state = DriveState::Reading;
                        self.done_at = Some(self.read_done(now, sector, size));
                    }
                    5 => {
                        self.sector = sector;
                        self.io_size = size;
                        self.write_ctrl = self.rx[9];
                        self.state = DriveState::Writing;
                    }
                    17 => {
                        self.ra_cache.clear();
                        self.state = DriveState::Formatting;
                        self.done_at = Some(now + self.delay_clocks);
                    }
                    _ => self.log.push(format!("unsupported command {}", cmd)),
                }
            }
            DriveState::Writing => {
                let off = self.sector as usize * SECTOR;
                let end = (off + self.rx.len()).min(self.image.len());
                if off < end && !matches!(self.write_ctrl, 1 | 2) {
                    let n = end - off;
                    self.image[off..end].copy_from_slice(&self.rx[..n]);
                    self.dirty = true;
                }
                self.state = DriveState::WritingWait;
                self.done_at = Some(self.write_done(now, self.sector, self.rx.len()));
            }
            _ => {}
        }
    }

    /// Run the drive's timer up to `now`.
    fn advance(&mut self, now: u64) {
        let Some(t) = self.done_at
        else {
            return;
        };
        if t > now {
            return;
        }
        self.done_at = None;
        match self.state {
            DriveState::Reading if self.image.is_empty() => {
                // No disk in the drive (a 2101's floppy slot left empty): error 128, as a
                // real 2101 answers (GRiDBench 2.1b e51).
                self.tx.extend(Self::error_block(128, self.sector));
            }
            DriveState::WritingWait if self.image.is_empty() => self.tx.extend(Self::error_block(128, self.sector)),
            DriveState::Reading => {
                self.chunked = self.io_size as usize > SECTOR;
                let off = self.sector as usize * SECTOR;
                for i in 0..self.io_size as usize {
                    self.tx.push_back(*self.image.get(off + i).unwrap_or(&0));
                }
            }
            DriveState::Formatting => {
                let mut sec = [0xE5u8; SECTOR];
                sec[..8].fill(0xFF);
                for chunk in self.image.chunks_mut(SECTOR) {
                    let n = chunk.len();
                    chunk.copy_from_slice(&sec[..n]);
                }
                self.dirty = true;
                self.tx.extend([0u8; 7]);
            }
            DriveState::WritingWait => self.tx.extend([0u8; 7]),
            _ => return,
        }
        self.spoll = 0x0F;
        self.srq = true;
        if gpib_log() {
            eprintln!("GPIB {} srq", now);
        }
        self.state = DriveState::Idle;
    }

    /// Next byte this drive would put on the bus as talker (ATN released): (byte, eoi).
    fn peek_talk(&self) -> Option<(u8, bool)> {
        if !self.talking || self.low_level() {
            return None;
        }
        if self.serial_polling {
            return Some((self.spoll | if self.srq { 0x40 } else { 0 }, true));
        }
        let n = self.tx.len();
        self.tx.front().map(|&b| (b, n == 1 || self.chunked && n % SECTOR == 1))
    }

    fn pop_talk(&mut self, now: u64) {
        if self.serial_polling {
            self.srq = false;
            self.spoll = 0;
            self.talking = false;
            if gpib_log() {
                eprintln!("GPIB {} spoll", now);
            }
        }
        else {
            self.tx.pop_front();
            if self.chunked && self.tx.len() % SECTOR == 0 {
                // End of a page: the box stops talking until it is addressed again.
                self.talking = false;
            }
            if self.tx.is_empty() && gpib_log() {
                eprintln!("GPIB {} lastbyte", now);
            }
        }
    }

    fn ifc(&mut self) {
        self.listening = false;
        self.talking = false;
    }
}

/// TMS9914A plus the devices on its bus.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Gpib {
    int0: u8,
    int1: u8,
    mask0: u8,
    mask1: u8,
    swrst: bool,
    sic: bool,
    ton: bool,
    lon: bool,
    dai: bool,
    next_eoi: bool,
    ctrl: Ctrl,
    di: u8,
    di_full: bool,
    /// Byte being sent by the 9914 and when it completes: (byte, atn, eoi, at).
    out: Option<(u8, bool, bool, u64)>,
    /// When the next talker byte will be latched into DI.
    in_at: Option<u64>,
    srq_prev: bool,
    /// Latest time seen.
    pub now: u64,
    /// Bytes talked by drives (counted only for the GPIB_STALL test hook).
    #[serde(skip)]
    talked: u64,
    pub drives: Vec<Drive>,
    /// Clocks for the 9914 to send one byte to the acceptors.
    pub send_clocks: u64,
    /// Clocks for a drive to send one byte to the 9914.
    pub talk_clocks: u64,
    pub trace: Vec<String>,
    pub trace_on: bool,
    /// Low-level 2102 on the bus: the 9914 has DAV asserted for the byte in `out`.
    #[serde(default)]
    host_dav: bool,
    /// Low-level 2102 talking: the 9914 has latched the byte on the bus (NDAC released).
    #[serde(default)]
    host_latched: bool,
    /// 2102 firmware: with it, every 2102 drive on the bus runs low-level (its 8051 runs this
    /// ROM); without it, the protocol-level model answers.
    #[serde(skip)]
    pub rom_2102: Option<Vec<u8>>,
    /// 2101 firmware: 300205-05 (20 MB drive) and 300205-06 (10 MB). With one that fits the
    /// hard disk image (41616 or 20808 pages), the 2101 runs low-level.
    #[serde(skip)]
    pub rom_2101: [Option<Vec<u8>>; 2],
}

impl Gpib {
    /// A 2101 box answers on both its addresses: with a hard disk and no floppy image, add
    /// the box's floppy drive with no disk in it, so GRiD-OS's polls of address 5 get answers.
    pub fn add_2101_floppy_slot(&mut self) {
        let boxes: Vec<(u8, bool)> =
            self.drives.iter().filter(|d| d.kind == DriveKind::Hdd2101).map(|d| (d.address(), d.protocol)).collect();
        for (a, protocol) in boxes {
            if !self.drives.iter().any(|d| d.address() == a + 1) {
                let mut d = Drive::at(DriveKind::Floppy2101, a + 1, Vec::new());
                d.protocol = protocol;
                self.drives.push(d);
            }
        }
    }

    pub fn new() -> Gpib {
        Gpib {
            int0: 0,
            int1: 0,
            mask0: 0,
            mask1: 0,
            swrst: true,
            sic: false,
            ton: false,
            lon: false,
            dai: false,
            next_eoi: false,
            ctrl: Ctrl::Idle,
            di: 0,
            di_full: false,
            out: None,
            in_at: None,
            srq_prev: false,
            now: 0,
            talked: 0,
            drives: Vec::new(),
            send_clocks: 40,
            // The real 2101 delivers a page in about 19 ms a read from its cache (GRiDBench
            // 2.1 e41), so the box keeps up with the host's pseudo-DMA loop.
            talk_clocks: 35,
            trace: Vec::new(),
            trace_on: false,
            host_dav: false,
            host_latched: false,
            rom_2102: None,
            rom_2101: [None, None],
        }
    }

    /// Give 2102 drives added without firmware their low-level box (once a ROM is set).
    fn attach_lle(&mut self) {
        // GRID_2102_EARLY: seconds the box was switched on before the Compass (default 4; 0 =
        // together, when the box is still busy with its self test as the boot PROM calls it).
        let early = std::env::var("GRID_2102_EARLY").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(4.0);
        let now = self.now;
        for d in &mut self.drives {
            if d.kind != DriveKind::Floppy2102 || d.lle.is_some() || d.protocol {
                continue;
            }
            let Some(rom) = d.rom.as_ref().or(self.rom_2102.as_ref())
            else {
                continue;
            };
            let mut b = Box2102::new(rom.clone(), d.address());
            b.set_us(Self::to_ns(now) / 1000);
            if now == 0 {
                b.power_on_early((early * 1e6) as u64, &mut d.image, &mut d.dirty);
            }
            d.lle = Some(Box::new(b));
        }
    }

    /// The 2101 firmware for a hard disk of `pages` pages: -05 for 41616 (612 cylinders), -06
    /// for 20808 (306); an empty slot takes either. None: no ROM fits (protocol model).
    pub fn rom_2101_for(&self, pages: usize) -> Option<&Vec<u8>> {
        let [r05, r06] = &self.rom_2101;
        match pages {
            41616 => r05.as_ref(),
            20808 => r06.as_ref(),
            0 => r05.as_ref().or(r06.as_ref()),
            _ => None,
        }
    }

    /// Give each 2101 box (hard disk and floppy) its low-level board once a ROM fits.
    fn attach_2101(&mut self) {
        let hds: Vec<usize> = (0..self.drives.len()).filter(|&i| self.drives[i].kind == DriveKind::Hdd2101).collect();
        for h in hds {
            self.attach_2101_box(h);
        }
    }

    /// The 2101 floppy drive of the box whose hard disk is drive `h` (at the next address).
    fn box_floppy(&self, h: usize) -> Option<usize> {
        let a = self.drives[h].address() + 1;
        self.drives.iter().position(|d| d.kind == DriveKind::Floppy2101 && d.address() == a)
    }

    fn note_2101(&mut self, h: usize, text: String) {
        if !self.drives[h].log.iter().any(|l| l.starts_with("2101 low-level")) {
            self.drives[h].log.push(text);
        }
    }

    fn attach_2101_box(&mut self, h: usize) {
        if self.drives[h].lle2101.is_some() || self.drives[h].protocol {
            return;
        }
        let fd = self.box_floppy(h);
        if fd.is_some_and(|f| self.drives[f].image.len() > 720 * SECTOR) {
            // A PC folder on the floppy drive is larger than any floppy the firmware can use.
            self.note_2101(h, "2101 low-level: off with a PC folder as the box's floppy, protocol model used".into());
            return;
        }
        let pages = self.drives[h].image.len() / SECTOR;
        let rom = match &self.drives[h].rom {
            Some(r) => Some(r.clone()),
            None => self.rom_2101_for(pages).cloned(),
        };
        let Some(rom) = rom
        else {
            if self.rom_2101.iter().any(|r| r.is_some()) {
                self.note_2101(h, format!("2101 low-level: no ROM for a {pages}-page disk, protocol model used"));
            }
            return;
        };
        // GRID_2101_EARLY: seconds the box was switched on before the Compass (default 4).
        let early = std::env::var("GRID_2101_EARLY").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(4.0);
        let mut b = Box2101::new(rom);
        b.set_address(self.drives[h].address());
        b.set_ns(Self::to_ns(self.now));
        if let Some(f) = fd {
            self.drives[f].served = true;
        }
        if self.now == 0 {
            let (hd_d, fd_d) = Self::two(&mut self.drives, h, fd);
            let mut empty = Vec::new();
            let mut nd = false;
            let (fdi, fdd) = match fd_d {
                Some(d) => (&mut d.image, &mut d.dirty),
                None => (&mut empty, &mut nd),
            };
            let mut m = Media { hd: &mut hd_d.image, hd_dirty: &mut hd_d.dirty, fd: fdi, fd_dirty: fdd };
            b.power_on_early((early * 1e9) as u64, &mut m);
        }
        self.drives[h].lle2101 = Some(Box::new(b));
    }

    /// Drive `a` and optionally drive `b` (a != b), both mutable.
    fn two(drives: &mut [Drive], a: usize, b: Option<usize>) -> (&mut Drive, Option<&mut Drive>) {
        match b {
            None => (&mut drives[a], None),
            Some(b) if b > a => {
                let (x, y) = drives.split_at_mut(b);
                (&mut x[a], Some(&mut y[0]))
            }
            Some(b) => {
                let (x, y) = drives.split_at_mut(a);
                (&mut y[0], Some(&mut x[b]))
            }
        }
    }

    /// Drives that carry a low-level box.
    fn boxes(&self) -> Vec<usize> {
        (0..self.drives.len()).filter(|&i| self.drives[i].lle.is_some() || self.drives[i].lle2101.is_some()).collect()
    }

    fn lle_any(&self) -> bool {
        self.drives.iter().any(|d| d.lle.is_some() || d.lle2101.is_some())
    }

    /// Box time (ns) of the box on drive `i`, in machine time (without its head start).
    fn box_ns(&self, i: usize) -> u64 {
        let d = &self.drives[i];
        if let Some(b) = &d.lle {
            (b.us() - b.ahead) * 1000
        }
        else if let Some(b) = &d.lle2101 {
            b.ns() - b.ahead
        }
        else {
            u64::MAX
        }
    }

    /// Run the box on drive `i` to machine time `target` ns, or to its first line change.
    fn run_box(&mut self, i: usize, target: u64) -> bool {
        if self.drives[i].lle.is_some() {
            let d = &mut self.drives[i];
            let b = d.lle.as_mut().unwrap();
            let ahead = b.ahead;
            return b.run(target / 1000 + ahead, &mut d.image, &mut d.dirty);
        }
        let fd = self.box_floppy(i).filter(|&f| self.drives[f].served);
        let (hd_d, fd_d) = Self::two(&mut self.drives, i, fd);
        let mut empty = Vec::new();
        let mut nd = false;
        let b = hd_d.lle2101.as_mut().unwrap();
        let fbusy = b.fd_busy();
        let (fdi, fdd) = match fd_d {
            Some(d) => {
                d.served_busy = fbusy;
                (&mut d.image, &mut d.dirty)
            }
            None => (&mut empty, &mut nd),
        };
        let mut m = Media { hd: &mut hd_d.image, hd_dirty: &mut hd_d.dirty, fd: fdi, fd_dirty: fdd };
        let ahead = b.ahead;
        b.run(target + ahead, &mut m)
    }

    /// The lines all low-level boxes drive (wired OR).
    fn lle_lines(&self) -> Lines {
        let mut l = Lines::default();
        for d in &self.drives {
            let b = if let Some(b) = &d.lle {
                b.lines()
            }
            else if let Some(b) = &d.lle2101 {
                b.lines()
            }
            else {
                continue;
            };
            l.dav |= b.dav;
            l.eoi |= b.eoi;
            l.nrfd |= b.nrfd;
            l.ndac |= b.ndac;
            l.srq |= b.srq;
            l.dio |= b.dio;
        }
        l
    }

    /// Box time (ns) for a machine clock count, and back.
    fn to_ns(t: u64) -> u64 {
        (t as u128 * 1_000_000_000 / hd::CPU_HZ as u128) as u64
    }
    fn from_ns(ns: u64) -> u64 {
        (ns as u128 * hd::CPU_HZ as u128).div_ceil(1_000_000_000) as u64
    }

    /// The lines the 9914 drives, as the low-level 2102 sees them.
    fn host_lines(&self) -> HostLines {
        let sourcing = self.host_dav;
        let listen = self.listener_active();
        HostLines {
            atn: !self.swrst && self.atn(),
            ifc: self.sic,
            dav: sourcing,
            eoi: sourcing && self.out.is_some_and(|o| o.2 && !o.1),
            dio: if sourcing { self.out.map(|o| o.0).unwrap_or(0) } else { 0 },
            nrfd: listen && (self.di_full || self.host_latched),
            ndac: listen && !self.host_latched,
        }
    }

    fn push_host(&mut self) {
        let h = self.host_lines();
        for d in &mut self.drives {
            if let Some(b) = d.lle.as_mut() {
                b.set_host(h);
            }
            if let Some(b) = d.lle2101.as_mut() {
                b.set_host(h);
            }
        }
    }

    /// Would the byte being sent complete now (low-level 2102 present)? It needs DAV up and
    /// every acceptor's NDAC released.
    fn out_ready(&self, l: &Lines) -> bool {
        self.host_dav && !l.ndac
    }

    /// React to the lines the low-level 2102 drives. Returns true if something the CPU can see
    /// changed (a byte latched or sent).
    fn on_lines(&mut self) -> bool {
        // A box whose handshake runs in hardware (the 2101's 8291A) answers host line changes
        // at once: go round until the lines settle.
        let mut seen = false;
        for _ in 0..8 {
            let before = self.lle_lines();
            seen |= self.on_lines_once();
            if self.lle_lines() == before {
                break;
            }
        }
        seen
    }

    fn on_lines_once(&mut self) -> bool {
        let l = self.lle_lines();
        let mut seen = false;
        if self.out.is_none() {
            self.host_dav = false;
        }
        if let Some((b, atn, eoi, at)) = self.out {
            if !self.host_dav && !l.nrfd {
                self.host_dav = true;
            }
            if self.out_ready(&l) && self.now >= at {
                self.complete_out(b, atn, eoi);
                seen = true;
            }
        }
        if self.listener_active() {
            if l.dav && !self.host_latched && !self.di_full {
                self.host_latched = true;
                self.di = l.dio;
                self.di_full = true;
                self.int0 |= INT0_BI;
                if l.eoi {
                    self.int0 |= INT0_END;
                }
                self.tr(format!("in  {:02X}{} (2102)", l.dio, if l.eoi { " EOI" } else { "" }));
                seen = true;
            }
        }
        if !l.dav {
            self.host_latched = false;
        }
        self.push_host();
        seen
    }

    /// The byte in `out` was accepted by everyone on the bus.
    fn complete_out(&mut self, b: u8, atn: bool, eoi: bool) {
        self.out = None;
        self.host_dav = false;
        let now = self.now;
        for d in &mut self.drives {
            if atn {
                d.command(b);
            }
            else if d.listening {
                d.data(b, eoi, now);
            }
        }
        if self.can_send() {
            self.int0 |= INT0_BO;
        }
    }

    /// Run the low-level 2102 up to clock `t`, handling each change of its lines. With `stop`,
    /// return at the first change the CPU can see. Returns true if it stopped early.
    fn run_lle(&mut self, t: u64, stop: bool) -> bool {
        let boxes = self.boxes();
        if boxes.is_empty() {
            return false;
        }
        let target = Self::to_ns(t);
        // Several boxes run in turns of at most 20 us, so each sees the others' handshakes
        // (through the host) about when they happen.
        const SLICE: u64 = 20_000;
        loop {
            let slice_end = if boxes.len() > 1 {
                boxes.iter().map(|&i| self.box_ns(i)).min().unwrap_or(target).saturating_add(SLICE).min(target)
            }
            else {
                target
            };
            for &i in &boxes {
                while self.box_ns(i) < slice_end {
                    if !self.run_box(i, slice_end) {
                        break;
                    }
                    let at = Self::from_ns(self.box_ns(i)).max(self.now);
                    self.now = at.min(t);
                    let cs = self.can_send();
                    let seen = self.on_lines();
                    self.update(cs);
                    if !stop || seen {
                        return true;
                    }
                }
            }
            if slice_end >= target {
                return false;
            }
        }
    }

    fn tr(&mut self, s: String) {
        static FROM: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
        let from = *FROM.get_or_init(|| std::env::var("GPIBREG").ok().and_then(|v| v.parse().ok()).unwrap_or(u64::MAX));
        if self.now >= from && !s.starts_with("R7") {
            eprintln!("GPIBREG {:>11} {}", self.now, s);
        }
        if self.trace_on && self.trace.len() < 200_000 {
            self.trace.push(format!("{:>11} {}", self.now, s));
        }
    }

    fn atn(&self) -> bool {
        self.ctrl == Ctrl::Active
    }

    /// Can the 9914 send a byte now (controller active, or talker with ATN released)?
    fn can_send(&self) -> bool {
        !self.swrst && (self.atn() || self.ton)
    }

    fn listener_active(&self) -> bool {
        !self.swrst && self.lon && !self.atn()
    }

    /// The 9914 INT line.
    pub fn int_line(&self) -> bool {
        !self.dai && ((self.int0 & self.mask0 & 0x3F) != 0 || (self.int1 & self.mask1) != 0)
    }

    fn srq_line(&self) -> bool {
        self.drives.iter().any(|d| d.srq) || self.lle_lines().srq
    }

    /// Re-evaluate state after a change: BO when becoming able to send, talker scheduling, SRQ.
    fn update(&mut self, could_send: bool) {
        if !could_send && self.can_send() && self.out.is_none() {
            self.int0 |= INT0_BO;
        }
        let srq = self.srq_line();
        if srq && !self.srq_prev && self.ctrl != Ctrl::Idle {
            self.int1 |= INT1_SRQ;
        }
        self.srq_prev = srq;
        self.schedule_in();
    }

    fn talker(&self) -> Option<usize> {
        self.drives.iter().position(|d| d.peek_talk().is_some())
    }

    fn schedule_in(&mut self) {
        fn stall_at() -> Option<u64> {
            static S: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
            *S.get_or_init(|| std::env::var("GPIB_STALL").ok().and_then(|v| v.parse().ok()))
        }
        if self.in_at.is_none() && self.listener_active() && !self.di_full && self.talker().is_some() {
            let i = self.talker().unwrap();
            let mut c = self.drives[i].talk_clocks().unwrap_or(self.talk_clocks);
            // Test hook: GPIB_STALL=N holds the Nth byte a drive talks for 10 s (to see the
            // keyboard watchdog end a hung transfer).
            if let Some(n) = stall_at() {
                self.talked += 1;
                if self.talked % 50_000 == 0 {
                    eprintln!("GPIB_STALL: {} bytes talked at {:.2} s", self.talked, self.now as f64 / 5e6);
                }
                if self.talked == n {
                    eprintln!("GPIB_STALL: holding byte {n} for 10 s at {:.2} s", self.now as f64 / 5e6);
                    c += 50_000_000;
                }
            }
            self.in_at = Some(self.now + c);
        }
    }

    /// Run the bus and drives up to time `t`.
    pub fn advance(&mut self, t: u64) {
        self.advance_inner(t, false);
    }

    /// Earliest pending event (protocol drives, bytes in flight).
    fn next_event(&self, lle: bool) -> u64 {
        let mut next = u64::MAX;
        if let Some((_, _, _, at)) = self.out {
            // With a low-level 2102 the byte also waits for the box's handshake.
            if !lle || self.out_ready(&self.lle_lines()) {
                next = next.min(at);
            }
        }
        if let Some(at) = self.in_at {
            next = next.min(at);
        }
        for d in &self.drives {
            if let Some(at) = d.done_at {
                next = next.min(at);
            }
        }
        next
    }

    /// Run up to `t`. With `stop`, return after the first event the CPU can see (a byte in DI,
    /// a byte sent, a drive's reply) instead.
    fn advance_inner(&mut self, t: u64, stop: bool) {
        if t < self.now {
            return;
        }
        if self.rom_2102.is_some() {
            self.attach_lle();
        }
        self.attach_2101();
        let lle = self.lle_any();
        if lle {
            self.push_host();
        }
        loop {
            let next = self.next_event(lle);
            if lle {
                let target = next.min(t);
                if self.run_lle(target, stop) {
                    if stop && self.ready_seen() {
                        return;
                    }
                    continue;
                }
            }
            if next > t {
                break;
            }
            self.now = self.now.max(next);
            let cs = self.can_send();
            if let Some((b, atn, eoi, at)) = self.out {
                if at <= self.now && (!lle || self.out_ready(&self.lle_lines())) {
                    self.complete_out(b, atn, eoi);
                }
            }
            for i in 0..self.drives.len() {
                let now = self.now;
                self.drives[i].advance(now);
            }
            if let Some(at) = self.in_at {
                if at <= self.now {
                    self.in_at = None;
                    if self.listener_active() && !self.di_full {
                        if let Some(i) = self.talker() {
                            let (b, eoi) = self.drives[i].peek_talk().unwrap();
                            self.drives[i].pop_talk(self.now);
                            self.di = b;
                            self.di_full = true;
                            self.int0 |= INT0_BI;
                            if eoi {
                                self.int0 |= INT0_END;
                            }
                            self.tr(format!("in  {:02X}{}", b, if eoi { " EOI" } else { "" }));
                        }
                    }
                }
            }
            self.update(cs);
            if lle {
                self.on_lines();
            }
            if stop {
                return;
            }
        }
        self.now = t;
        let cs = self.can_send();
        self.update(cs);
    }

    /// For dma_wait with a low-level 2102: something a waiting access cares about happened.
    fn ready_seen(&self) -> bool {
        self.di_full || self.out.is_none() || self.int_line()
    }

    /// Clocks until a pseudo-DMA access at the current time can complete (0 if now).
    /// Reads wait for a byte in DI, writes for the previous byte to be accepted. The wait also ends
    /// when the 9914 raises its INT line (END, ERR), as the driver relies on, or after `limit`.
    pub fn dma_wait(&mut self, write: bool, limit: u64) -> u64 {
        let start = self.now;
        if self.lle_any() {
            loop {
                let ready = if write { self.out.is_none() } else { self.di_full };
                if ready || self.int_line() || self.now >= start + limit {
                    return self.now - start;
                }
                self.advance_inner(start + limit, true);
            }
        }
        loop {
            let ready = if write { self.out.is_none() } else { self.di_full };
            if ready || self.int_line() {
                return self.now - start;
            }
            let mut next = u64::MAX;
            if let Some((_, _, _, at)) = self.out {
                next = next.min(at);
            }
            if let Some(at) = self.in_at {
                next = next.min(at);
            }
            for d in &self.drives {
                if let Some(at) = d.done_at {
                    next = next.min(at);
                }
            }
            if next > start + limit {
                // Nothing will free the bus in time (or at all): the CPU stays held; the
                // board stops the wait when the keyboard's watchdog drops kbdDma.
                self.advance(start + limit);
                return self.now - start;
            }
            self.advance(next);
        }
    }

    pub fn read(&mut self, reg: usize) -> u8 {
        let r = self.read_inner(reg);
        if reg == 7 && self.lle_any() {
            self.on_lines();
        }
        r
    }

    fn read_inner(&mut self, reg: usize) -> u8 {
        let r = match reg {
            0 => {
                let mut v = self.int0 & 0x3F;
                if v & self.mask0 != 0 {
                    v |= INT0_INT0;
                }
                if self.int1 & self.mask1 != 0 {
                    v |= INT0_INT1;
                }
                self.int0 = 0;
                v
            }
            1 => {
                let v = self.int1;
                self.int1 = 0;
                v
            }
            2 => {
                // address status: ATN, TADS/LADS approximated by ton/lon
                (if self.atn() { 0x20 } else { 0 }) | (if self.lon { 0x04 } else { 0 }) | (if self.ton { 0x02 } else { 0 })
            }
            3 => {
                // bus status: ATN 80, DAV 40, NDAC 20, NRFD 10, EOI 08, SRQ 04, IFC 02, REN 01
                let mut v = 0u8;
                if self.atn() {
                    v |= 0x80;
                }
                // Idle acceptors hold NDAC asserted.
                let acceptor = self.atn() && !self.drives.is_empty()
                    || self.drives.iter().any(|d| d.listening)
                    || self.listener_active();
                if acceptor {
                    v |= 0x20;
                }
                if self.di_full {
                    v |= 0x10;
                }
                if self.srq_line() {
                    v |= 0x04;
                }
                if self.sic {
                    v |= 0x02;
                }
                if self.lle_any() {
                    let l = self.lle_lines();
                    let h = self.host_lines();
                    v &= !0x30;
                    v |= if l.dav || h.dav { 0x40 } else { 0 }
                        | if l.ndac || h.ndac { 0x20 } else { 0 }
                        | if l.nrfd || h.nrfd { 0x10 } else { 0 }
                        | if l.eoi || h.eoi { 0x08 } else { 0 };
                }
                v
            }
            6 => self.di,
            7 => {
                let v = self.di;
                self.int0 &= !INT0_BI;
                self.di_full = false;
                self.schedule_in();
                v
            }
            _ => 0,
        };
        if reg != 0 || self.trace_on {
            let s = format!("R{} = {:02X}", reg, r);
            if reg != 0 {
                self.tr(s);
            }
        }
        r
    }

    pub fn write(&mut self, reg: usize, data: u8) {
        self.write_inner(reg, data);
        if self.lle_any() {
            self.on_lines();
        }
    }

    fn write_inner(&mut self, reg: usize, data: u8) {
        self.tr(format!("W{} = {:02X}", reg, data));
        let cs = self.can_send();
        match reg {
            0 => self.mask0 = data & 0x3F,
            1 => self.mask1 = data,
            3 => self.aux(data & 0x1F, data & 0x80 != 0),
            7 => {
                self.int0 &= !INT0_BO;
                let eoi = std::mem::take(&mut self.next_eoi);
                if self.swrst {
                    return;
                }
                let atn = self.atn();
                let l = self.lle_lines();
                let has_acceptor = atn && !self.drives.is_empty() || self.drives.iter().any(|d| d.listening) || l.ndac || l.nrfd;
                if !self.can_send() {
                    return;
                }
                if !has_acceptor {
                    // No listener: source handshake error.
                    self.int1 |= INT1_ERR;
                    self.tr(format!("out {:02X} -> nobody (ERR)", data));
                    return;
                }
                self.tr(format!("out {:02X}{}{}", data, if atn { " ATN" } else { "" }, if eoi { " EOI" } else { "" }));
                self.out = Some((data, atn, eoi, self.now + self.send_clocks));
                return;
            }
            _ => {}
        }
        self.update(cs);
    }

    fn aux(&mut self, cmd: u8, set: bool) {
        match cmd {
            0x00 => {
                if set && !self.swrst {
                    self.int0 = 0;
                    self.int1 = 0;
                    self.ton = false;
                    self.lon = false;
                    self.ctrl = Ctrl::Idle;
                    self.out = None;
                    self.in_at = None;
                    self.di_full = false;
                }
                self.swrst = set;
                if !set && self.sic && self.ctrl == Ctrl::Idle {
                    self.ctrl = Ctrl::Active;
                }
            }
            0x08 => self.next_eoi = true,
            0x09 => {
                self.lon = set && !self.swrst;
            }
            0x0A => {
                self.ton = set && !self.swrst;
            }
            0x0B => {
                if self.ctrl == Ctrl::Active {
                    self.ctrl = Ctrl::Standby;
                }
            }
            0x0C | 0x0D => {
                if self.ctrl == Ctrl::Standby {
                    self.ctrl = Ctrl::Active;
                    // Taking control drops any byte being latched from the talker.
                    self.in_at = None;
                }
            }
            0x0F => {
                if set != self.sic {
                    self.sic = set;
                    if set {
                        for d in &mut self.drives {
                            d.ifc();
                        }
                        if !self.swrst && self.ctrl == Ctrl::Idle {
                            self.ctrl = Ctrl::Active;
                        }
                    }
                }
            }
            0x11 => {
                if !self.swrst && self.ctrl == Ctrl::Idle {
                    self.ctrl = Ctrl::Active;
                }
            }
            0x12 => self.ctrl = Ctrl::Idle,
            0x13 => self.dai = set,
            _ => self.tr(format!("aux {:02X} ignored", cmd)),
        }
    }
}
