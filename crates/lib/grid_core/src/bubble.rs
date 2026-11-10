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
}

/// Adjustable timing, in CPU clocks (to be measured on a real 1101).
#[derive(Copy, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BubbleTiming {
    /// Time to move one 32-byte FSA page between the bubble and the FIFO.
    pub page_clocks: u64,
    /// Time before the first page of a read or write (seek to the page).
    pub start_clocks: u64,
    /// Initialize / Read FSA Status, per MBM.
    pub init_clocks_per_mbm: u64,
    /// A failing command.
    pub fail_clocks: u64,
}

impl Default for BubbleTiming {
    fn default() -> Self {
        // MAME's values (4 MHz controller clock), converted to 5 MHz CPU clocks.
        BubbleTiming { page_clocks: 6750, start_clocks: 6750, init_clocks_per_mbm: 75, fail_clocks: 1500 }
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
                Op::Write => self.fifo.len() < FIFO_SIZE - 22,
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
        match cmd {
            1 => {
                // Initialize
                if self.nfc() != 2 {
                    self.op = Op::Fail;
                    self.next = now + t.fail_clocks;
                }
                else {
                    self.op = Op::Init;
                    self.next = now + t.init_clocks_per_mbm * self.mbms as u64;
                }
            }
            8 => {
                self.op = Op::ReadFsa;
                self.next = now + t.init_clocks_per_mbm * self.mbms as u64;
            }
            2 | 3 => {
                if self.mbm() >= self.mbms || self.nfc() != 2 {
                    self.op = Op::Fail;
                    self.next = now + t.fail_clocks;
                }
                else {
                    self.op = if cmd == 2 { Op::Read } else { Op::Write };
                    self.counter = 0;
                    self.limit = self.pages() * self.nfc();
                    self.next = now + t.start_clocks;
                }
            }
            0x0E => {
                // MBM Purge
                self.regs[R_AR_L] = 0;
                self.regs[R_AR_M] &= !7;
                self.end(true);
            }
            9 => {
                // Abort
                if self.op == Op::None {
                    self.fifo_clear();
                }
                self.end(true);
            }
            0x0D => {
                self.fifo_clear();
                self.end(true);
            }
            0x0F => {
                self.regs[R_UR] = 0;
                self.fifo_clear();
                self.end(true);
            }
            _ => {
                self.op = Op::Fail;
                self.next = now + t.fail_clocks;
            }
        }
        self.update_drq();
    }

    /// Byte offset in the image of FSA page `k` (32 bytes) of the current transfer.
    fn page_offset(&self, k: usize) -> usize {
        self.addr() * 32 * self.nfc() + self.mbm() * MBM_SIZE + k * 32
    }

    /// Run the controller up to CPU clock `now`.
    pub fn advance(&mut self, now: u64) {
        while self.next <= now {
            let t = self.next;
            match self.op {
                Op::Fail => self.end(false),
                Op::Init => self.end(true),
                Op::ReadFsa => {
                    for _ in 0..self.mbms * 2 {
                        self.fifo_push(0x28);
                    }
                    self.end(true);
                }
                Op::Read => {
                    if self.counter < self.limit {
                        let o = self.page_offset(self.counter);
                        for i in 0..32 {
                            let v = self.image.get(o + i).copied().unwrap_or(0);
                            self.fifo_push(v);
                        }
                        self.counter += 1;
                        self.next = t + self.timing.page_clocks;
                    }
                    else {
                        self.end(true);
                    }
                }
                Op::Write => {
                    if self.fifo.len() >= 32 {
                        let o = self.page_offset(self.counter);
                        for i in 0..32 {
                            let v = self.fifo_pop();
                            if o + i < self.image.len() {
                                self.image[o + i] = v;
                            }
                        }
                        self.dirty = true;
                        self.counter += 1;
                    }
                    if self.counter < self.limit {
                        self.next = t + self.timing.page_clocks;
                    }
                    else {
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
                    Op::Write if self.fifo.len() < FIFO_SIZE => v |= SR_FIFO,
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
