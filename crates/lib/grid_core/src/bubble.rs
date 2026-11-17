//! Intel 7220-1 bubble memory controller with the 1101's 384K bubble (3 x 7110, 1 Mbit each).
//!
//! Registers: DFE80 = data (FIFO, or the parametric register the register pointer selects),
//! DFE82 = status (read) / command (write). Commands as in the 7220 data sheet; the ones GRiD's
//! driver (bigbubbles.plm, tinybubbles.asm) uses are Initialize, Read/Write Bubble Data,
//! Abort, Reset FIFO, Software Reset, plus Read FSA Status and MBM Purge. Based on MAME's
//! i7220 (BSD-3-Clause, Sergey Svishchev), with the timing in CPU clocks.
//!
//! The image is the bubble's contents in address order: page `p` (64 bytes with 2 FSA channels)
//! of MBM `m` is at m * 128K + p * 64.
//!
//! The 1101 wires the FIFO data request (DRQ) to IR1: GRiD's driver runs the transfer from
//! that interrupt (it is up while the FIFO has 22 or more bytes on a read, or fewer than 18 on
//! a write).

use std::collections::VecDeque;

pub const MBM_SIZE: usize = 128 * 1024;

const R_UR: usize = 0x0A;
const R_BLR_L: usize = 0x0B;
const R_BLR_M: usize = 0x0C;
const R_AR_L: usize = 0x0E;
const R_AR_M: usize = 0x0F;

const SR_FIFO: u8 = 0x01;
const SR_PARITY: u8 = 0x02;
const SR_UE: u8 = 0x04;
const SR_CE: u8 = 0x08;
const SR_TE: u8 = 0x10;
const SR_FAIL: u8 = 0x20;
const SR_DONE: u8 = 0x40;
const SR_BUSY: u8 = 0x80;
const SR_CLEAR: u8 = 0x7E;

const FIFO_SIZE: usize = 40;

#[derive(Copy, Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
enum Phase {
    Idle,
    Exec,
    Result,
}

#[derive(Copy, Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
enum Op {
    None,
    Fail,
    Init,
    ReadFsa,
    Read,
    Write,
    /// A command that just ends (OK) at `next`.
    Quick,
}

/// Adjustable timing, in CPU clocks, fitted to GRiDProbe on a real 1101 (the 1137 times the
/// same). The MBM loops only turn while a command runs and stop where it ends. Page `a` passes
/// the detector at slot `a * stride` of a `loop_slots` loop, so the next address comes round
/// one page time (`stride` slots, 7.2 ms) later: 1 page on is 12.5 ms, 2 on 20 ms, half the
/// loop on 46 ms, the same page again a full turn (87 ms).
#[derive(Copy, Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BubbleTiming {
    /// One loop position (field rotations of the 7110).
    pub slot_clocks: u64,
    pub loop_slots: u64,
    /// Slots between consecutive page addresses (odd).
    pub stride: u64,
    /// Read: from the command to the loops starting to turn.
    pub start_clocks: u64,
    /// Write: from the command to the loops starting to turn.
    pub write_start_clocks: u64,
    /// One byte into (read) or out of (write) the FIFO while a page passes.
    pub byte_clocks: u64,
    /// From the last page passing to busy off (read / write).
    pub read_tail_clocks: u64,
    pub write_tail_clocks: u64,
    /// Write: the FIFO only takes data (status bit 0, DRQ) from this long before the first
    /// page passes (GRiDProbe b21-b24 and b55 on a real 1101: no byte goes in before then).
    pub write_lead_clocks: u64,
    /// Initialize: fixed part, then (unless an MBM Purge came since the last transfer) `init_turn_clocks`
    /// more and a turn of the loops to the next of `init_stops` stop positions.
    pub init_clocks: u64,
    pub init_stops: u64,
    pub init_turn_clocks: u64,
    pub fsa_clocks: u64,
    /// Reset FIFO, Software Reset.
    pub reset_clocks: u64,
    /// Abort when idle / during a transfer.
    pub abort_clocks: u64,
    pub abort_busy_clocks: u64,
    pub purge_clocks: u64,
    /// A failing command.
    pub fail_clocks: u64,
}

impl Default for BubbleTiming {
    fn default() -> Self {
        BubbleTiming {
            slot_clocks: 199,
            loop_slots: 2048,
            stride: 181,
            start_clocks: 26_500,
            write_start_clocks: 1_500,
            byte_clocks: 400,
            read_tail_clocks: 25_600,
            write_tail_clocks: 29_000,
            write_lead_clocks: 6_000,
            init_clocks: 2_400,
            init_stops: 40,
            init_turn_clocks: 30_000,
            fsa_clocks: 200,
            reset_clocks: 200,
            // GRiDProbe b46 / b73-b76 on a real 1101: busy off 0.16 ms after an idle Abort,
            // 0.14-0.16 ms during a read
            abort_clocks: 600,
            abort_busy_clocks: 480,
            purge_clocks: 1_250,
            fail_clocks: 1_500,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Bubble {
    pub image: Vec<u8>,
    pub dirty: bool,
    pub timing: BubbleTiming,
    mbms: usize,
    regs: [u8; 16],
    rac: usize,
    str_: u8,
    fifo: VecDeque<u8>,
    phase: Phase,
    op: Op,
    /// Next step of the current command (absolute CPU clock).
    next: u64,
    counter: usize,
    limit: usize,
    drq: bool,
    pub log: bool,
    /// Where each MBM's loops stopped (clocks into the loop).
    #[serde(default)]
    theta: [u64; 16],
    /// Current transfer: loops turning from `rot_start`, first page passing at `reach`.
    #[serde(default)]
    rot_start: u64,
    #[serde(default)]
    reach: u64,
    /// An MBM Purge came after the last data transfer (Initialize is short).
    #[serde(default)]
    purged: bool,
    /// A write's FIFO takes data (see `write_lead_clocks`).
    #[serde(default)]
    armed: bool,
    /// An Abort came between two pages of a read: until a Software Reset, a read of more than
    /// one page stops after the first with status 2C (GRiDProbe b76-b79 on a real 1101).
    #[serde(default)]
    stuck: bool,
    /// The current read is one of those (it ends after one page, failed).
    #[serde(default)]
    short: bool,
    /// MBMs written since the last Software Reset (their FSA status has bit 3 set).
    #[serde(default)]
    written: u16,
    /// Read FSA Status gives FFH for the first channel, as on a real 1101 (GRiDProbe b01/b82).
    /// The board sets it: off on the Compass II plug-in bubble board, whose ROM driver counts
    /// the modules from these bytes (see `start`).
    #[serde(default = "yes")]
    pub fsa_first_ff: bool,
}

fn yes() -> bool {
    true
}

impl Bubble {
    /// `image` must be a multiple of 128K (one per MBM; the 1101 has 3).
    pub fn new(mut image: Vec<u8>) -> Bubble {
        let mbms = (image.len() / MBM_SIZE).max(1);
        image.resize(mbms * MBM_SIZE, 0);
        Bubble {
            image,
            dirty: false,
            timing: BubbleTiming::default(),
            mbms,
            regs: [0; 16],
            rac: 0,
            str_: 0,
            fifo: VecDeque::with_capacity(FIFO_SIZE),
            phase: Phase::Idle,
            op: Op::None,
            next: u64::MAX,
            counter: 0,
            limit: 0,
            drq: false,
            log: false,
            theta: [0; 16],
            rot_start: 0,
            reach: 0,
            purged: false,
            armed: false,
            stuck: false,
            short: false,
            written: 0,
            fsa_first_ff: true,
        }
    }

    /// The DRQ line (wired to IR1 on the 1101).
    pub fn drq(&self) -> bool {
        self.drq
    }

    fn blr(&self) -> u16 {
        (self.regs[R_BLR_M] as u16) << 8 | self.regs[R_BLR_L] as u16
    }
    fn ar(&self) -> u16 {
        (self.regs[R_AR_M] as u16) << 8 | self.regs[R_AR_L] as u16
    }
    fn nfc(&self) -> usize {
        let n = (self.blr() >> 12) as usize;
        if n == 0 {
            1
        }
        else {
            n * 2
        }
    }
    fn pages(&self) -> usize {
        (self.blr() & 0x7FF) as usize
    }
    fn addr(&self) -> usize {
        (self.ar() & 0x7FF) as usize
    }
    fn mbm(&self) -> usize {
        ((self.ar() >> 11) & 15) as usize
    }

    fn update_drq(&mut self) {
        self.drq = self.phase == Phase::Exec
            && match self.op {
                Op::Read => self.fifo.len() >= 22,
                Op::Write => self.armed && self.fifo.len() < FIFO_SIZE - 22,
                _ => false,
            };
    }

    fn fifo_push(&mut self, v: u8) {
        if self.fifo.len() < FIFO_SIZE {
            self.fifo.push_back(v);
        }
        self.update_drq();
    }

    fn fifo_pop(&mut self) -> u8 {
        let v = self.fifo.pop_front().unwrap_or(0);
        self.update_drq();
        v
    }

    fn fifo_clear(&mut self) {
        self.fifo.clear();
        self.drq = false;
    }

    fn end(&mut self, ok: bool) {
        self.phase = Phase::Result;
        self.op = Op::None;
        self.next = u64::MAX;
        self.str_ |= if ok { SR_DONE } else { SR_FAIL | SR_TE };
        self.update_drq();
    }

    fn loop_clocks(&self) -> u64 {
        self.timing.slot_clocks * self.timing.loop_slots
    }

    /// Stop the current MBM's loops at clock `t` (they turned from `rot_start`).
    fn stop_loops(&mut self, t: u64) {
        let l = self.loop_clocks();
        let m = self.mbm();
        self.theta[m] = (self.theta[m] + t.saturating_sub(self.rot_start)) % l;
    }

    /// Bytes in one bubble page (64 with 2 FSA channels).
    fn page_bytes(&self) -> usize {
        self.nfc() * 32
    }

    /// When transfer byte `c` goes into (read) or out of (write) the FIFO.
    fn byte_time(&self, c: usize) -> u64 {
        let t = self.timing;
        let pb = self.page_bytes();
        self.reach + (c / pb) as u64 * t.stride * t.slot_clocks + (c % pb) as u64 * t.byte_clocks
    }

    /// When the last page has passed and busy goes off.
    fn transfer_end(&self) -> u64 {
        let t = self.timing;
        let last = (self.pages().max(1) - 1) as u64 * t.stride * t.slot_clocks;
        self.reach + last + if self.op == Op::Write { t.write_tail_clocks } else { t.read_tail_clocks }
    }

    fn quick(&mut self, now: u64, clocks: u64) {
        self.op = Op::Quick;
        self.next = now + clocks;
    }

    fn start(&mut self, cmd: u8, now: u64) {
        // A command points the register address counter back at the FIFO. The Compass II ROM
        // driver (GRiD's 1984 bigbubbles.plm) reads the utility register (RAC then 0BH), gives
        // Abort, Software Reset, Reset FIFO and Read FSA Status, and reads the data port while
        // the status says FIFO ready, counting the bytes that are not FFH: with the RAC still
        // on the parametric registers it would count five of them as modules too.
        self.rac = 0;
        self.phase = Phase::Exec;
        self.str_ &= !SR_CLEAR;
        if self.log {
            eprintln!(
                "bubble cmd {:X} BLR {:04X} AR {:04X} ER {:02X} at {}",
                cmd,
                self.blr(),
                self.ar(),
                self.regs[0x0D],
                now
            );
        }
        let t = self.timing;
        // After an MBM Purge, Initialize stays short (GRiDProbe b48 on a real 1101: 0.49 ms
        // four times in a row) until the next data transfer.
        if cmd == 0x0E {
            self.purged = true;
        }
        else if matches!(cmd, 2 | 3) {
            self.purged = false;
        }
        let purged = self.purged;
        match cmd {
            1 => {
                // Initialize: the loops turn to the next stop position.
                if self.nfc() != 2 {
                    self.op = Op::Fail;
                    self.next = now + t.fail_clocks;
                }
                else if self.mbm() >= self.mbms {
                    // An address register naming a module that is not fitted: the controller
                    // never finds that module's bootloop and stays busy until an Abort (Bubble
                    // Fix's module count and the Compass II driver rely on this failing).
                    self.op = Op::Init;
                    self.next = u64::MAX;
                }
                else {
                    // GRiDProbe on a real 1101: 6.0 .. 8.6 ms, but 0.49 ms after a Purge
                    let q = (self.loop_clocks() / t.init_stops.max(1)).max(1);
                    let th = self.theta[0];
                    let wait = if purged {
                        0
                    }
                    else {
                        let turn = t.init_turn_clocks;
                        turn + (q - (th + turn) % q) % q
                    };
                    let l = self.loop_clocks();
                    for th in self.theta.iter_mut() {
                        *th = (*th + wait) % l;
                    }
                    self.op = Op::Init;
                    self.next = now + t.init_clocks + wait;
                }
            }
            8 => {
                self.op = Op::ReadFsa;
                self.next = now + t.fsa_clocks;
            }
            2 | 3 => {
                if self.mbm() >= self.mbms || self.nfc() != 2 {
                    self.op = Op::Fail;
                    self.next = now + t.fail_clocks;
                }
                else {
                    self.op = if cmd == 2 { Op::Read } else { Op::Write };
                    self.counter = 0;
                    self.limit = self.pages() * self.page_bytes();
                    self.short = cmd == 2 && self.stuck && self.pages() > 1;
                    if self.short {
                        self.limit = self.page_bytes();
                    }
                    self.rot_start = now + if cmd == 2 { t.start_clocks } else { t.write_start_clocks };
                    let l = self.loop_clocks();
                    let pos = (self.addr() as u64 * t.stride % t.loop_slots) * t.slot_clocks;
                    self.reach = self.rot_start + (pos + l - self.theta[self.mbm()]) % l;
                    self.armed = cmd == 2;
                    self.next = if self.limit == 0 {
                        self.transfer_end()
                    }
                    else if cmd == 3 {
                        self.byte_time(0).saturating_sub(t.write_lead_clocks).max(now)
                    }
                    else {
                        self.byte_time(0)
                    };
                }
            }
            0x0E => {
                // MBM Purge
                self.regs[R_AR_L] = 0;
                self.regs[R_AR_M] &= !7;
                self.quick(now, t.purge_clocks);
            }
            9 => {
                // Abort: a transfer stops at once, its FIFO is dropped.
                if matches!(self.op, Op::Read | Op::Write) {
                    // Between two pages of a read (the last page all in, more to come), the
                    // real 7220 is left in a state where later multi-page reads fail.
                    let pb = self.page_bytes();
                    if self.op == Op::Read && self.counter > 0 && self.counter % pb == 0 && self.counter < self.limit {
                        self.stuck = true;
                    }
                    self.stop_loops(now.max(self.rot_start));
                    self.fifo_clear();
                    self.quick(now, t.abort_busy_clocks);
                }
                else {
                    self.fifo_clear();
                    self.quick(now, t.abort_clocks);
                }
            }
            0x0D => {
                self.fifo_clear();
                self.quick(now, t.reset_clocks);
            }
            0x0F => {
                self.stuck = false;
                self.written = 0;
                self.regs[R_UR] = 0;
                self.fifo_clear();
                self.quick(now, t.reset_clocks);
            }
            _ => {
                self.op = Op::Fail;
                self.next = now + t.fail_clocks;
            }
        }
        self.update_drq();
    }

    /// Byte offset in the image of transfer byte `c`.
    fn byte_offset(&self, c: usize) -> usize {
        self.addr() * self.page_bytes() + self.mbm() * MBM_SIZE + c
    }

    /// Run the controller up to CPU clock `now`.
    pub fn advance(&mut self, now: u64) {
        while self.next <= now {
            let t = self.next;
            match self.op {
                Op::Fail => self.end(false),
                Op::Init | Op::Quick => self.end(true),
                Op::ReadFsa => {
                    // GRiDProbe b01/b82 on a real 1101: FF 20 20 20 20 20 (the first channel
                    // always reads FF), with 28 for both channels of an MBM written since the
                    // last Software Reset. The Compass II driver counts the bytes that are not
                    // FFH and halves the count, so there every channel answers.
                    for c in 0..self.mbms * 2 {
                        let w = self.written >> (c / 2) & 1 != 0;
                        self.fifo_push(if c == 0 && self.fsa_first_ff { 0xFF } else if w { 0x28 } else { 0x20 });
                    }
                    self.end(true);
                }
                Op::Write if !self.armed => {
                    self.armed = true;
                    self.update_drq();
                    self.next = self.byte_time(0);
                }
                Op::Read | Op::Write => {
                    if self.counter < self.limit {
                        let o = self.byte_offset(self.counter);
                        if self.op == Op::Read {
                            let v = self.image.get(o).copied().unwrap_or(0);
                            self.fifo_push(v);
                        }
                        else {
                            let v = self.fifo_pop();
                            if o < self.image.len() {
                                self.image[o] = v;
                                self.dirty = true;
                            }
                        }
                        self.counter += 1;
                        self.next = if self.counter < self.limit {
                            self.byte_time(self.counter)
                        }
                        else if self.short {
                            // busy off ~3 ms after the first byte (real b77-b79)
                            t + 1_000
                        }
                        else {
                            self.transfer_end()
                        };
                    }
                    else if self.short {
                        self.short = false;
                        self.stop_loops(t);
                        self.end(true);
                        self.str_ = self.str_ & !SR_DONE | SR_FAIL | SR_CE | SR_UE;
                    }
                    else {
                        let write = self.op == Op::Write;
                        self.stop_loops(t);
                        self.end(true);
                        if write {
                            // A real 1101 ends nearly every write with status 42 (parity bit
                            // set, data fine; GRiDProbe b81-b87); GRiD-OS ignores the bit.
                            self.str_ |= SR_PARITY;
                            self.written |= 1 << self.mbm();
                        }
                    }
                }
                Op::None => self.next = u64::MAX,
            }
        }
    }

    /// Read DFE80 (`reg` 0) or DFE82 (`reg` 1).
    pub fn read(&mut self, reg: usize, now: u64) -> u8 {
        self.advance(now);
        if reg == 0 {
            if self.rac != 0 {
                let v = self.regs[self.rac];
                self.rac = (self.rac + 1) & 15;
                v
            }
            else {
                self.fifo_pop()
            }
        }
        else {
            let mut v = self.str_;
            if self.phase == Phase::Exec {
                v |= SR_BUSY;
                match self.op {
                    Op::Read if !self.fifo.is_empty() => v |= SR_FIFO,
                    Op::Write if self.armed && self.fifo.len() < FIFO_SIZE => v |= SR_FIFO,
                    _ => {}
                }
            }
            else if !self.fifo.is_empty() {
                v |= SR_FIFO;
            }
            if self.phase == Phase::Result {
                self.phase = Phase::Idle;
            }
            v
        }
    }

    /// Write DFE80 (`reg` 0) or DFE82 (`reg` 1).
    pub fn write(&mut self, reg: usize, v: u8, now: u64) {
        self.advance(now);
        if reg == 0 {
            if self.rac != 0 {
                self.regs[self.rac] = v;
                self.rac = (self.rac + 1) & 15;
            }
            else {
                self.fifo_push(v);
            }
        }
        else if v & 0x20 != 0 {
            // interrupt clear: nothing latched here
        }
        else if v & 0x10 != 0 {
            if self.phase == Phase::Idle || self.phase == Phase::Result || (v & 15) == 9 {
                self.start(v & 15, now);
            }
        }
        else {
            self.rac = (v & 15) as usize;
        }
    }
}

/// Pages in one 7110 module.
pub const MBM_PAGES: usize = 2048;

/// A bubble image read from a file, as `load_image` found it.
pub struct LoadedImage {
    pub image: Vec<u8>,
    /// Where the emulator saves the bubble: the file itself for a raw image, a new raw image
    /// next to it for Bubble Fix backups (they are never written over).
    pub save_to: std::path::PathBuf,
    /// What was read, for the user (empty for a raw image).
    pub note: String,
}

/// One module from a Bubble Fix backup.
struct FixModule {
    module: Option<usize>,
    pages: Vec<u8>,
}

/// Parse a GRiD Bubble Fix backup of one module ("<name> Module N~Bin~" or "~Raw~"):
/// version 2 (64-byte BUBBACK1 header, 2048 pages of 64 or 68 bytes, 2048 status bytes,
/// BUBTAIL1 tail with the page shift) or version 1 (pages and status bytes only). Gives the
/// module's 128K in page order; pages the backup could not read are zeros.
fn parse_bubfix(d: &[u8]) -> Option<FixModule> {
    let n = MBM_PAGES;
    let (module, psize, body, synced, shift) = if d.len() >= 64 && &d[..8] == b"BUBBACK1" {
        let psize = d[10] as usize;
        let pages = u16::from_le_bytes([d[12], d[13]]) as usize;
        if !(psize == 64 || psize == 68) || pages != n || d.len() < 64 + n * psize {
            return None;
        }
        let tail = 64 + n * psize + n;
        let shift = (d.len() >= tail + 16 && &d[tail..tail + 8] == b"BUBTAIL1")
            .then(|| u16::from_le_bytes([d[tail + 14], d[tail + 15]]))
            .filter(|&s| s != 0xFFFF)
            .map(|s| s as usize % n);
        (Some((d[8] & 7) as usize), psize, &d[64..], d[11] == 1, shift)
    }
    else if d.len() == n * 64 + n || d.len() == n * 68 + n {
        (None, (d.len() - n) / n, d, true, None)
    }
    else {
        return None;
    };
    // a status byte per page: 00H = not read (a file cut short keeps its pages)
    let status = |p: usize| body.get(n * psize + p).copied().unwrap_or(0x40);
    let mut pages = vec![0u8; MBM_SIZE];
    for p in 0..n {
        if status(p) == 0 {
            continue;
        }
        // a backup made without the bootloop has its pages shifted: the restore puts page p
        // at (p + shift) mod 2048, and so do we
        let q = if synced { p } else { (p + shift.unwrap_or(0)) % n };
        pages[q * 64..q * 64 + 64].copy_from_slice(&body[p * psize..p * psize + 64]);
    }
    Some(FixModule { module, pages })
}

/// Is this file a Bubble Fix backup (reads only its first bytes)?
pub fn is_bubfix_file(path: &std::path::Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let len = f.metadata().map(|m| m.len() as usize).unwrap_or(0);
    let mut head = [0u8; 8];
    let magic = f.read_exact(&mut head).is_ok() && &head == b"BUBBACK1";
    magic || len == MBM_PAGES * 65 || len == MBM_PAGES * 69
}

/// The module number (1-8) a Bubble Fix file name carries ("Bubble Module 2~Bin~"), with the
/// byte range of the digit.
fn module_in_name(name: &str) -> Option<(usize, std::ops::Range<usize>)> {
    let low = name.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = low[from..].find("module") {
        let mut j = from + i + 6;
        while low.as_bytes().get(j) == Some(&b' ') || low.as_bytes().get(j) == Some(&b'_') {
            j += 1;
        }
        if let Some(c @ b'1'..=b'8') = low.as_bytes().get(j).copied() {
            return Some(((c - b'1') as usize, j..j + 1));
        }
        from += i + 6;
    }
    None
}

/// Read a bubble image. A raw image (128K per module) is used as it is; a missing file gives
/// a blank image of `blank_bytes`. A GRiD Bubble Fix backup of one module is read with its
/// sibling modules (the same name with the other module numbers), each put at its module's
/// place; modules without a backup stay blank. The changed bubble is then saved to a raw
/// image next to the backups ("<name> bubble.img"), never into them.
pub fn load_image(path: &std::path::Path, blank_bytes: usize) -> Result<LoadedImage, String> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LoadedImage { image: vec![0u8; blank_bytes.max(MBM_SIZE)], save_to: path.to_path_buf(), note: String::new() })
        }
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let Some(first) = parse_bubfix(&data) else {
        return Ok(LoadedImage { image: data, save_to: path.to_path_buf(), note: String::new() });
    };
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let named = module_in_name(&name);
    let mut found: Vec<(usize, Vec<u8>)> = vec![];
    let mut sources = vec![path.to_path_buf()];
    let place = |m: Option<usize>, fallback: usize, pages: Vec<u8>, found: &mut Vec<(usize, Vec<u8>)>| {
        let m = m.unwrap_or(fallback);
        if !found.iter().any(|(k, _)| *k == m) {
            found.push((m, pages));
        }
    };
    place(first.module, named.as_ref().map(|n| n.0).unwrap_or(0), first.pages, &mut found);
    let mut base = name.clone();
    if let Some((_, r)) = &named {
        for k in 0..8 {
            let mut sib = name.clone();
            sib.replace_range(r.clone(), &((b'1' + k as u8) as char).to_string());
            let sp = path.with_file_name(&sib);
            if sib == name || !sp.is_file() {
                continue;
            }
            if let Some(fm) = std::fs::read(&sp).ok().and_then(|d| parse_bubfix(&d)) {
                place(fm.module, k, fm.pages, &mut found);
                sources.push(sp);
            }
        }
        // "Bubble Module 1~Bin~" -> "Bubble"
        let low = name.to_ascii_lowercase();
        let cut = low[..r.start].rfind("module").unwrap_or(r.start);
        base = name[..cut].trim_end_matches([' ', '_', '-']).to_string();
    }
    let save_to = path.with_file_name(format!("{} bubble.img", if base.is_empty() { "Bubble Fix" } else { &base }));
    // the bubble saved from these backups before (and not older than them) holds the GRiD's
    // later changes: use it
    let newest = sources.iter().filter_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok()).max();
    if let (Ok(saved), Some(newest)) = (std::fs::metadata(&save_to).and_then(|m| m.modified()), newest) {
        if saved >= newest {
            if let Ok(image) = std::fs::read(&save_to) {
                let note = format!("Bubble Fix backup: using {} (saved from these backups before)", save_to.file_name().unwrap().to_string_lossy());
                return Ok(LoadedImage { image, save_to, note });
            }
        }
    }
    found.sort_by_key(|f| f.0);
    let mbms = found.last().map(|f| f.0 + 1).unwrap_or(1).max(blank_bytes / MBM_SIZE).max(1);
    let mut image = vec![0u8; mbms * MBM_SIZE];
    for (m, pages) in &found {
        image[m * MBM_SIZE..(m + 1) * MBM_SIZE].copy_from_slice(pages);
    }
    let list = found.iter().map(|f| (f.0 + 1).to_string()).collect::<Vec<_>>().join(", ");
    let missing: Vec<String> = (0..mbms).filter(|m| !found.iter().any(|f| f.0 == *m)).map(|m| (m + 1).to_string()).collect();
    let mut note = format!(
        "Bubble Fix backup: module{} {list} of {mbms}",
        if found.len() == 1 { "" } else { "s" }
    );
    if !missing.is_empty() {
        note += &format!(" (module {} blank)", missing.join(", "));
    }
    note += &format!("; saved to {}", save_to.file_name().unwrap().to_string_lossy());
    Ok(LoadedImage { image, save_to, note })
}
