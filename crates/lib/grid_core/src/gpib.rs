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

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Ctrl {
    /// Not controller in charge.
    Idle,
    /// Controller active, ATN asserted.
    Active,
    /// Controller standby, ATN released.
    Standby,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
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

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum DriveState {
    Idle,
    Reading,
    Writing,
    WritingWait,
    Formatting,
}

/// GRiD 2101/2102 drive, byte-level port of MAME's grid210x_device.
pub struct Drive {
    pub kind: DriveKind,
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
    pub log: Vec<String>,
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
            log: Vec::new(),
        }
    }

    fn status_block(&self) -> [u8; 52] {
        let (count, bitmap, superblock, min_dir, name, spt, tpc): (u16, u16, u16, u16, &str, u16, u16) = match self.kind {
            DriveKind::Hdd2101 => (
                (self.image.len() / SECTOR).min(0xFFFF) as u16,
                0x2400,
                0x2420,
                10,
                "COMPASS-EMU HARD DISK",
                10,
                4,
            ),
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
        let group = b & 0xE0;
        let addr = b & 0x1F;
        if group == 0x20 {
            self.rx.clear();
            self.listening = addr == self.kind.address();
            self.talking = false;
        }
        else if group == 0x40 {
            self.rx.clear();
            self.talking = addr == self.kind.address();
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
                match cmd {
                    0 => self.tx.extend([0u8; 7]),
                    1 => {
                        let s = self.status_block();
                        self.tx.extend(s);
                    }
                    4 => {
                        self.sector = sector;
                        self.io_size = size;
                        self.state = DriveState::Reading;
                        self.done_at = Some(now + self.delay_clocks);
                    }
                    5 => {
                        self.sector = sector;
                        self.io_size = size;
                        self.state = DriveState::Writing;
                    }
                    17 => {
                        self.state = DriveState::Formatting;
                        self.done_at = Some(now + self.delay_clocks);
                    }
                    _ => self.log.push(format!("unsupported command {}", cmd)),
                }
            }
            DriveState::Writing => {
                let off = self.sector as usize * SECTOR;
                let end = (off + self.rx.len()).min(self.image.len());
                if off < end {
                    let n = end - off;
                    self.image[off..end].copy_from_slice(&self.rx[..n]);
                    self.dirty = true;
                }
                self.state = DriveState::WritingWait;
                self.done_at = Some(now + self.delay_clocks);
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
            DriveState::Reading => {
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
        self.state = DriveState::Idle;
    }

    /// Next byte this drive would put on the bus as talker (ATN released): (byte, eoi).
    fn peek_talk(&self) -> Option<(u8, bool)> {
        if !self.talking {
            return None;
        }
        if self.serial_polling {
            return Some((self.spoll | if self.srq { 0x40 } else { 0 }, true));
        }
        self.tx.front().map(|&b| (b, self.tx.len() == 1))
    }

    fn pop_talk(&mut self) {
        if self.serial_polling {
            self.srq = false;
            self.spoll = 0;
            self.talking = false;
        }
        else {
            self.tx.pop_front();
        }
    }

    fn ifc(&mut self) {
        self.listening = false;
        self.talking = false;
    }
}

/// TMS9914A plus the devices on its bus.
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
    pub drives: Vec<Drive>,
    /// Clocks for the 9914 to send one byte to the acceptors.
    pub send_clocks: u64,
    /// Clocks for a drive to send one byte to the 9914.
    pub talk_clocks: u64,
    pub trace: Vec<String>,
    pub trace_on: bool,
}

impl Gpib {
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
            drives: Vec::new(),
            send_clocks: 40,
            talk_clocks: 100,
            trace: Vec::new(),
            trace_on: false,
        }
    }

    fn tr(&mut self, s: String) {
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
        self.drives.iter().any(|d| d.srq)
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
        if self.in_at.is_none() && self.listener_active() && !self.di_full && self.talker().is_some() {
            self.in_at = Some(self.now + self.talk_clocks);
        }
    }

    /// Run the bus and drives up to time `t`.
    pub fn advance(&mut self, t: u64) {
        if t < self.now {
            return;
        }
        loop {
            // Earliest pending event at or before t.
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
            if next > t {
                break;
            }
            self.now = next;
            let cs = self.can_send();
            if let Some((b, atn, eoi, at)) = self.out {
                if at <= next {
                    self.out = None;
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
            }
            for i in 0..self.drives.len() {
                let now = self.now;
                self.drives[i].advance(now);
            }
            if let Some(at) = self.in_at {
                if at <= next {
                    self.in_at = None;
                    if self.listener_active() && !self.di_full {
                        if let Some(i) = self.talker() {
                            let (b, eoi) = self.drives[i].peek_talk().unwrap();
                            self.drives[i].pop_talk();
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
        }
        self.now = t;
        let cs = self.can_send();
        self.update(cs);
    }

    /// Clocks until a pseudo-DMA access at the current time can complete (0 if now).
    /// Reads wait for a byte in DI, writes for the previous byte to be accepted. The wait also ends
    /// when the 9914 raises its INT line (END, ERR), as the driver relies on, or after `limit`.
    pub fn dma_wait(&mut self, write: bool, limit: u64) -> u64 {
        let start = self.now;
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
            if next == u64::MAX || next > start + limit {
                let end = start + limit.min(if next == u64::MAX { 0 } else { limit });
                self.advance(end);
                return self.now - start;
            }
            self.advance(next);
        }
    }

    pub fn read(&mut self, reg: usize) -> u8 {
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
                let has_acceptor = atn && !self.drives.is_empty() || self.drives.iter().any(|d| d.listening);
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
