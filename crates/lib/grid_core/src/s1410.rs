//! Xebec S1410 hard disk controller with an ST-225 drive, at the level of SASI commands, as
//! inside the GRiD 2101. The 2101's board CPU drives the SASI bus; this model answers it.
//!
//! The real S1410 is a Z80 with an 8237 DMA chip and a Xebec custom serializer (3198-0025)
//! that has no documentation, so it is not emulated at chip level. Its command set and data
//! come from the firmware (ROM 104683 D, dispatcher at 0085h: 00-10h and E0-E6h) and the
//! Xebec manual; its timing comes from the drive (3600 rpm, 17 sectors a track, seeks from a
//! real 2101 with an ST-225) and the controller's buffer (two sectors).
//!
//! Time is in nanoseconds.

use serde::{Deserialize, Serialize};

const SECTOR: usize = 512;
const SPT: u32 = 17;
const ROT_NS: u64 = 1_000_000_000 / 60;
const SLOT_NS: u64 = ROT_NS / SPT as u64;

/// Timing values (ns), with env overrides for fitting: S1410_CMD, S1410_BYTE, S1410_ECC,
/// S1410_SEL (us).
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// From the last command byte to the start of the work (Z80 decodes the command).
    pub cmd: u64,
    /// One SASI byte (REQ to REQ, controller side).
    pub byte: u64,
    /// After a sector has passed the head, until its data can go out (ECC check).
    pub ecc: u64,
    /// Selection: SEL seen until BSY.
    pub sel: u64,
    /// Status to message, and message to bus free.
    pub phase: u64,
}

pub fn timing() -> &'static Timing {
    static T: std::sync::OnceLock<Timing> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let env = |k: &str, d: u64| std::env::var(k).ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(d) * 1000;
        Timing {
            cmd: env("S1410_CMD", 440),
            byte: env("S1410_BYTE", 2),
            ecc: env("S1410_ECC", 300),
            sel: env("S1410_SEL", 10),
            phase: env("S1410_PHASE", 30),
        }
    })
}

const SEEK: [(u32, u64); 12] = [
    (1, 9_500),
    (2, 12_600),
    (4, 13_200),
    (8, 14_900),
    (16, 16_000),
    (38, 21_500),
    (64, 26_500),
    (128, 28_500),
    (200, 32_200),
    (306, 41_500),
    (400, 51_000),
    (611, 70_400),
];

/// Time for the heads to arrive (ns) `d` cylinders away: measured inside a real 2101 by the
/// Disk Toolkit 0.4.1 box probe (Seek, then Test Ready until good, less the 2.5 ms the pair
/// takes for 0 cylinders), smoothed where the run was not monotonic (64-128).
pub fn seek_ns(d: u32) -> u64 {
    static TABLE: std::sync::OnceLock<Vec<(u32, u64)>> = std::sync::OnceLock::new();
    // S1410_SEEK="1:20000,38:65000,611:135000" (cylinders:us) overrides the curve for fitting.
    let t = TABLE.get_or_init(|| {
        let mut v = vec![(0u32, 0u64)];
        if let Ok(e) = std::env::var("S1410_SEEK") {
            v.extend(e.split(',').filter_map(|p| {
                let (a, b) = p.split_once(':')?;
                Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
            }));
        }
        else {
            v.extend(SEEK);
        }
        v
    });
    let max = t.last().map(|e| e.0).unwrap_or(0);
    let d = d.min(max);
    #[allow(non_snake_case)]
    let T = t.as_slice();
    #[allow(non_snake_case)]
    let i = T.iter().position(|e| e.0 >= d).unwrap_or(T.len() - 1);
    let us = if i == 0 {
        0
    }
    else {
        let ((d0, t0), (d1, t1)) = (T[i - 1], T[i]);
        t0 + (t1 - t0) * (d - d0) as u64 / (d1 - d0) as u64
    };
    us * 1000
}

/// The drive's format interleave (S1410_IL). Default 5: the 2101 firmware's own format, and what
/// the box probe read from the sector IDs of a real 2101's drive.
fn interleave() -> u64 {
    static IL: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *IL.get_or_init(|| std::env::var("S1410_IL").ok().and_then(|v| v.parse().ok()).unwrap_or(5))
}

/// A buffered seek: the S1410 answers the Seek command once it has sent the step pulses
/// (2 ms + 20 us a cylinder; a real box took 2-14 ms), while the heads arrive later.
const SEEK_CMD_NS: u64 = 1_500_000;
const STEP_PULSE_NS: u64 = 20_000;
/// Recalibrate steps back to track 0 slowly (a real box needed over 1.7 s from cylinder 611).
const RECAL_STEP_NS: u64 = 3_000_000;
/// Request Sense answers sooner than other commands (0.27 ms against Test Ready's 0.41 ms).
const SENSE_EARLY_NS: u64 = 140_000;
/// Extra wait after a Read's or Write's own seek (see `S1410::seek`); S1410_SETTLE (us).
static IMPLIED_SETTLE_NS: std::sync::LazyLock<u64> =
    std::sync::LazyLock::new(|| std::env::var("S1410_SETTLE").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(8_000) * 1000);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Phase {
    BusFree,
    /// Selected: BSY asserted, waiting for SEL to drop.
    Selected,
    Command,
    /// Working: no REQ until the data or status is ready.
    Busy,
    DataIn,
    DataOut,
    Status,
    Message,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Work {
    None,
    Read,
    Write,
    /// Data-out for a command that is not a disk write (init drive parameters, write buffer,
    /// write long).
    Param,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct S1410 {
    pub phase: Phase,
    /// REQ goes up at this time (in a phase that has REQ).
    req_at: u64,
    cdb: Vec<u8>,
    buf: Vec<u8>,
    pos: usize,
    /// Per sector of a read or write: when its data is in the buffer (read) or written (write).
    chunk_at: Vec<u64>,
    /// When the host finished moving each sector.
    moved_at: Vec<u64>,
    work: Work,
    lba: u32,
    count: u32,
    status: u8,
    /// Sense: error code, address valid.
    sense: u8,
    sense_lba: u32,
    /// Next phase time when Busy.
    busy_until: u64,
    /// Drive: cylinder under the heads, heads from init drive parameters.
    cyl: u32,
    heads: u32,
    cyls: u32,
    /// Sector buffer (write/read buffer commands).
    sbuf: Vec<u8>,
    /// Last sector timing: when the controller can start the next read (read pointer).
    read_free: u64,
    /// When the heads arrive after a buffered seek (Test Ready reports not ready until then).
    #[serde(default)]
    heads_at: u64,
    pub log: bool,
    /// Commands seen, for the box log.
    #[serde(skip)]
    pub trace: Vec<String>,
}

impl Default for S1410 {
    fn default() -> Self {
        Self::new()
    }
}

impl S1410 {
    pub fn new() -> S1410 {
        S1410 {
            phase: Phase::BusFree,
            req_at: 0,
            cdb: Vec::new(),
            buf: Vec::new(),
            pos: 0,
            chunk_at: Vec::new(),
            moved_at: Vec::new(),
            work: Work::None,
            lba: 0,
            count: 0,
            status: 0,
            sense: 0,
            sense_lba: 0,
            busy_until: 0,
            cyl: 0,
            heads: 4,
            cyls: 612,
            sbuf: vec![0; SECTOR],
            read_free: 0,
            heads_at: 0,
            log: std::env::var_os("BOXLOG").is_some(),
            trace: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.phase = Phase::BusFree;
        self.work = Work::None;
        self.buf.clear();
        self.cdb.clear();
    }

    pub fn bsy(&self) -> bool {
        self.phase != Phase::BusFree
    }

    /// C/D (true = command/status/message).
    pub fn cd(&self) -> bool {
        matches!(self.phase, Phase::Command | Phase::Status | Phase::Message)
    }

    /// I/O (true = to the host).
    pub fn io(&self) -> bool {
        matches!(self.phase, Phase::DataIn | Phase::Status | Phase::Message)
    }

    pub fn msg(&self) -> bool {
        self.phase == Phase::Message
    }

    /// REQ at time `now`.
    pub fn req(&self, now: u64) -> bool {
        matches!(self.phase, Phase::Command | Phase::DataIn | Phase::DataOut | Phase::Status | Phase::Message) && now >= self.req_at
    }

    /// When something changes without the host doing anything (REQ rising, selection, a
    /// command finishing).
    pub fn next_event(&self, now: u64) -> u64 {
        match self.phase {
            Phase::BusFree => u64::MAX,
            Phase::Busy => self.busy_until.max(now),
            _ if self.req_at > now => self.req_at,
            _ => u64::MAX,
        }
    }

    /// The host side: SEL with the controller's ID bit (data bit 0) on the bus.
    pub fn select(&mut self, now: u64, sel: bool, data: u8) {
        match self.phase {
            Phase::BusFree if sel && data & 1 != 0 => {
                self.phase = Phase::Selected;
                self.req_at = now + timing().sel;
            }
            Phase::Selected if !sel => {
                self.phase = Phase::Command;
                self.cdb.clear();
                self.req_at = now.max(self.req_at) + timing().byte;
            }
            _ => {}
        }
    }

    /// The BSY line seen by the host (asserted a little after selection).
    pub fn bsy_at(&self, now: u64) -> bool {
        match self.phase {
            Phase::BusFree => false,
            Phase::Selected => now >= self.req_at,
            _ => true,
        }
    }

    /// Run timed events up to `now`.
    pub fn advance(&mut self, now: u64, image: &mut [u8], dirty: &mut bool) {
        if self.phase == Phase::Busy && now >= self.busy_until {
            match self.work {
                Work::Read => {
                    self.phase = Phase::DataIn;
                    self.req_at = self.busy_until;
                }
                _ => self.to_status(self.busy_until),
            }
        }
        let _ = (image, dirty);
    }

    fn to_status(&mut self, at: u64) {
        self.phase = Phase::Status;
        self.req_at = at + timing().phase;
        self.work = Work::None;
    }

    fn err(&mut self, code: u8, lba: u32) {
        self.status = 0x02;
        self.sense = code;
        self.sense_lba = lba;
    }

    fn lba_of(&self) -> u32 {
        ((self.cdb[1] as u32 & 0x1F) << 16) | (self.cdb[2] as u32) << 8 | self.cdb[3] as u32
    }

    fn cyl_of(&self, lba: u32) -> u32 {
        lba / (SPT * self.heads.max(1))
    }

    /// Time at or after `t` when sector `s` (0-16) of a track starts under the head.
    fn sector_start(t: u64, s: u32) -> u64 {
        let target = (s as u64 * interleave() % SPT as u64) * SLOT_NS;
        let a = t % ROT_NS;
        t + (target + ROT_NS - a) % ROT_NS
    }

    /// The seek a Read or Write does by itself. Its first sector comes later than the heads
    /// arrive by a Seek command: a real box (Disk Toolkit 0.1.0 seek test, per read less the
    /// no-seek read) takes 26 / 78 / 151 ms more for 1 cylinder / random / full stroke, about
    /// twice the heads-in time plus 8 ms, as if the controller let the heads settle as long
    /// again before it reads an ID.
    fn seek(&mut self, t: u64, lba: u32) -> u64 {
        let t = t.max(self.heads_at);
        let c = self.cyl_of(lba);
        let d = c.abs_diff(self.cyl);
        self.cyl = c;
        let settle = if d > 0 { seek_ns(d) + *IMPLIED_SETTLE_NS } else { 0 };
        self.heads_at = t + seek_ns(d) + settle;
        self.heads_at
    }

    /// A Seek command: the heads arrive after the measured heads-in time.
    fn seek_cmd(&mut self, t: u64, lba: u32) -> u64 {
        let t = t.max(self.heads_at);
        let c = self.cyl_of(lba);
        let d = c.abs_diff(self.cyl);
        self.cyl = c;
        self.heads_at = t + seek_ns(d);
        self.heads_at
    }

    /// Host reads a byte (data-in, status or message phase) at `now`, completing REQ/ACK.
    pub fn read_byte(&mut self, now: u64, image: &mut [u8]) -> u8 {
        let t = timing();
        match self.phase {
            Phase::DataIn => {
                let v = *self.buf.get(self.pos).unwrap_or(&0);
                self.pos += 1;
                if self.work == Work::Read && self.pos % SECTOR == 0 {
                    self.moved_at.push(now);
                }
                if self.pos >= self.buf.len() {
                    if self.work == Work::Read && self.status == 0 {
                        // The read is over (or failed part way, handled when building it).
                    }
                    self.to_status(now);
                }
                else {
                    let mut at = now + t.byte;
                    if self.work == Work::Read && self.pos % SECTOR == 0 {
                        let k = self.pos / SECTOR;
                        at = at.max(self.read_ready(k, image));
                    }
                    self.req_at = at;
                }
                v
            }
            Phase::Status => {
                if self.log {
                    self.trace.push(format!("S1410 status {:02X} at {} us", self.status, now / 1000));
                }
                self.phase = Phase::Message;
                self.req_at = now + t.phase;
                self.status
            }
            Phase::Message => {
                self.phase = Phase::BusFree;
                0
            }
            _ => 0xFF,
        }
    }

    /// When sector `k` of the current read is in the buffer (computed as the host gets there,
    /// so a slow host makes the controller wait for buffer space and miss sectors).
    fn read_ready(&mut self, k: usize, _image: &mut [u8]) -> u64 {
        while self.chunk_at.len() <= k {
            let i = self.chunk_at.len();
            let lba = self.lba + i as u32;
            // Two sector buffers: sector i needs sector i-2 to have gone to the host.
            let mut t = self.read_free;
            if i >= 2 {
                t = t.max(*self.moved_at.get(i - 2).unwrap_or(&u64::MAX));
            }
            if self.cyl_of(lba) != self.cyl {
                t = self.seek(t, lba);
            }
            let start = Self::sector_start(t, lba % SPT);
            self.read_free = start + SLOT_NS;
            self.chunk_at.push(start + SLOT_NS + timing().ecc);
            if self.log {
                self.trace.push(format!("S1410 sector {} ready at {} us", lba, (start + SLOT_NS + timing().ecc) / 1000));
            }
        }
        self.chunk_at[k]
    }

    /// Host writes a byte (command or data-out phase) at `now`, completing REQ/ACK.
    pub fn write_byte(&mut self, now: u64, v: u8, image: &mut [u8], dirty: &mut bool) {
        let t = timing();
        match self.phase {
            Phase::Command => {
                self.cdb.push(v);
                if self.cdb.len() == 6 {
                    self.start(now + t.cmd, image, dirty);
                }
                else {
                    self.req_at = now + t.byte;
                }
            }
            Phase::DataOut => {
                self.buf.push(v);
                if self.buf.len() >= self.pos {
                    self.data_out_done(now, image, dirty);
                }
                else {
                    let mut at = now + t.byte;
                    if self.work == Work::Write && self.buf.len() % SECTOR == 0 {
                        // The next sector goes into a buffer once one is free.
                        let k = self.buf.len() / SECTOR;
                        self.write_sector(k - 1, now, image, dirty);
                        if k >= 2 {
                            at = at.max(self.chunk_at[k - 2]);
                        }
                    }
                    self.req_at = at;
                }
            }
            _ => {}
        }
    }

    /// Write sector `k` of the current write (its data has arrived at `now`).
    fn write_sector(&mut self, k: usize, now: u64, image: &mut [u8], dirty: &mut bool) {
        let lba = self.lba + k as u32;
        let mut t = now.max(self.chunk_at.last().copied().unwrap_or(0)).max(self.read_free);
        if self.cyl_of(lba) != self.cyl {
            t = self.seek(t, lba);
        }
        let start = Self::sector_start(t, lba % SPT);
        let end = start + SLOT_NS;
        self.read_free = end;
        self.chunk_at.push(end);
        let off = lba as usize * SECTOR;
        if off + SECTOR <= image.len() {
            image[off..off + SECTOR].copy_from_slice(&self.buf[k * SECTOR..(k + 1) * SECTOR]);
            *dirty = true;
        }
    }

    fn data_out_done(&mut self, now: u64, image: &mut [u8], dirty: &mut bool) {
        match self.work {
            Work::Write => {
                let k = self.buf.len() / SECTOR;
                self.write_sector(k - 1, now, image, dirty);
                self.phase = Phase::Busy;
                self.busy_until = *self.chunk_at.last().unwrap();
            }
            _ => {
                match self.cdb[0] {
                    0x0C => {
                        let b = &self.buf;
                        self.cyls = (b[0] as u32) << 8 | b[1] as u32;
                        self.heads = (b[2] as u32).max(1);
                    }
                    0x0F => {
                        self.sbuf.copy_from_slice(&self.buf[..SECTOR]);
                    }
                    0xE6 => {
                        // write long: data plus 4 ECC bytes (the ECC is not kept)
                        let lba = self.lba_of();
                        let off = lba as usize * SECTOR;
                        if off + SECTOR <= image.len() {
                            image[off..off + SECTOR].copy_from_slice(&self.buf[..SECTOR]);
                            *dirty = true;
                        }
                    }
                    _ => {}
                }
                self.to_status(now + t_cmd_small());
            }
        }
    }

    /// Execute the command in `cdb` (decoded at `t`).
    fn start(&mut self, t: u64, image: &mut [u8], dirty: &mut bool) {
        let op = self.cdb[0];
        let lba = self.lba_of();
        let count = match self.cdb[4] {
            0 => 256,
            n => n as u32,
        };
        let pages = (image.len() / SECTOR) as u32;
        if self.log {
            self.trace.push(format!("S1410 cmd {:02X} lba {} count {} ctl {:02X}", op, lba, self.cdb[4], self.cdb[5]));
        }
        self.status = 0;
        if op != 0x03 {
            self.sense = 0;
        }
        self.buf.clear();
        self.pos = 0;
        self.chunk_at.clear();
        self.moved_at.clear();
        self.work = Work::None;
        self.lba = lba;
        self.count = count;
        let not_ready = image.is_empty();
        let range = |n: u32| lba + n > pages;
        let mut done = t;
        match op {
            0x00 => {
                if not_ready {
                    self.err(0x04, 0);
                }
                else if t < self.heads_at {
                    // heads still moving after a buffered seek: no seek complete
                    self.err(0x02, 0);
                }
            }
            0x01 => {
                if not_ready {
                    self.err(0x04, 0);
                }
                else {
                    done = t.max(self.heads_at) + self.cyl as u64 * RECAL_STEP_NS;
                    self.heads_at = done;
                    self.cyl = 0;
                }
            }
            0x03 => {
                // request sense: 4 bytes
                let valid = if self.sense != 0 { 0x80 } else { 0 };
                self.buf = vec![self.sense | valid, (self.sense_lba >> 16) as u8 & 0x1F, (self.sense_lba >> 8) as u8, self.sense_lba as u8];
                self.sense = 0;
                self.phase = Phase::DataIn;
                self.req_at = t.saturating_sub(SENSE_EARLY_NS);
                return;
            }
            0x04 | 0x06 | 0x07 | 0x0E => {
                // format drive / track / bad track / alternate track: data 6Ch
                if not_ready {
                    self.err(0x04, 0);
                }
                else {
                    let (from, n) = if op == 0x04 { (0, pages) } else { (lba - lba % SPT, SPT) };
                    if range(0) && op != 0x04 {
                        self.err(0x21, lba);
                    }
                    else {
                        let s = from as usize * SECTOR;
                        let e = ((from + n) as usize * SECTOR).min(image.len());
                        let fill = if self.cdb[5] & 0x20 != 0 { None } else { Some(0x6Cu8) };
                        for (i, b) in image[s..e].iter_mut().enumerate() {
                            *b = fill.unwrap_or(self.sbuf[i % SECTOR]);
                        }
                        *dirty = true;
                        let tracks = n.div_ceil(SPT) as u64;
                        let cyls = (tracks / self.heads.max(1) as u64).max(1);
                        done = t + seek_ns(self.cyl) + tracks * ROT_NS * 2 + cyls * seek_ns(1);
                        self.cyl = self.cyl_of(from + n - 1);
                    }
                }
            }
            0x05 => {
                // check track format
                if not_ready {
                    self.err(0x04, 0);
                }
                else if range(0) {
                    self.err(0x21, lba);
                }
                else {
                    done = self.seek(t, lba) + ROT_NS;
                }
            }
            0x08 | 0xE5 => {
                if not_ready {
                    self.err(0x04, 0);
                }
                else if range(if op == 0xE5 { 1 } else { count }) {
                    self.err(0x21, lba);
                }
                else if op == 0xE5 {
                    // read long: data plus 4 ECC bytes
                    let off = lba as usize * SECTOR;
                    self.buf = image[off..off + SECTOR].to_vec();
                    self.buf.extend([0u8; 4]);
                    let s = self.seek(t, lba);
                    let start = Self::sector_start(s, lba % SPT);
                    self.phase = Phase::DataIn;
                    self.req_at = start + SLOT_NS;
                    return;
                }
                else {
                    let off = lba as usize * SECTOR;
                    self.buf = image[off..off + count as usize * SECTOR].to_vec();
                    self.work = Work::Read;
                    self.read_free = t.max(self.heads_at);
                    if self.cyl_of(lba) != self.cyl {
                        self.read_free = self.seek(t, lba);
                    }
                    let ready = self.read_ready(0, image);
                    self.phase = Phase::Busy;
                    self.busy_until = ready;
                    return;
                }
            }
            0x0A => {
                if not_ready {
                    self.err(0x04, 0);
                }
                else if range(count) {
                    self.err(0x21, lba);
                }
                else {
                    self.work = Work::Write;
                    self.pos = count as usize * SECTOR;
                    self.read_free = t.max(self.heads_at);
                    self.phase = Phase::DataOut;
                    self.req_at = t;
                    return;
                }
            }
            0x0B => {
                if not_ready {
                    self.err(0x04, 0);
                }
                else if range(0) {
                    self.err(0x21, lba);
                }
                else {
                    // buffered: the command ends when the steps are out, the heads follow
                    let from = self.cyl;
                    let t0 = t.max(self.heads_at);
                    self.seek_cmd(t0, lba);
                    done = t0 + SEEK_CMD_NS + from.abs_diff(self.cyl) as u64 * STEP_PULSE_NS - timing().cmd.min(SEEK_CMD_NS);
                }
            }
            0x0C | 0x0F | 0xE6 => {
                self.work = Work::Param;
                self.pos = match op {
                    0x0C => 8,
                    0x0F => SECTOR,
                    _ => SECTOR + 4,
                };
                self.phase = Phase::DataOut;
                self.req_at = t;
                return;
            }
            0x0D => {
                // read ECC burst length
                self.buf = vec![0];
                self.phase = Phase::DataIn;
                self.req_at = t;
                return;
            }
            0x10 => {
                self.buf = self.sbuf.clone();
                self.phase = Phase::DataIn;
                self.req_at = t;
                return;
            }
            0xE0 | 0xE4 => done = t + 50_000_000,
            0xE3 => {
                if not_ready {
                    self.err(0x04, 0);
                }
                else {
                    done = t + seek_ns(self.cyl) + seek_ns(self.cyls.saturating_sub(1)) * 2;
                    self.cyl = 0;
                }
            }
            _ => self.err(0x20, 0),
        }
        let _ = dirty;
        self.phase = Phase::Busy;
        self.busy_until = done;
    }
}

fn t_cmd_small() -> u64 {
    timing().phase
}
