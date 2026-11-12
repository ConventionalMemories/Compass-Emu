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
            abort_clocks: 400,
            abort_busy_clocks: 700,
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
                    for _ in 0..self.mbms * 2 {
                        self.fifo_push(0x28);
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
                        self.next = if self.counter < self.limit { self.byte_time(self.counter) } else { self.transfer_end() };
                    }
                    else {
                        self.stop_loops(t);
                        self.end(true);
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
