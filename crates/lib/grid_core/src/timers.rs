//! The three timers at ports 08h/0Ah/0Ch (control 0Eh), as each board has them.
//!
//! 110x and 112x: the timers inside the Intel 80130 (SYSTICK, DELAY, BAUD). Real 1101
//! (GRiDProbe 0.0.11 T tests): timer 0 counts at CPU clock / 2 (2.5 MHz, as Intel's AP-130
//! and GRiD's own Cp/timer.plm with its 400 ns units say), timer 1 only moves on timer 0's
//! output pulses (GRiD's 32-bit stopwatch chain), there is no read-back command, the
//! counters run from power-on, and the control port reads FFh (handled by the board).
//!
//! 113x: a discrete P8254 on the 5 MHz CPU clock (photos of a 1137 and a 1139 board; no
//! separate timer clock on the 1139). Real 1137 (GRiDProbe 0.0.7 timers): all three count at
//! 5 MHz, the read-back command works (status bytes B4/B6), a counter stands still (reads
//! FFFF) until a count is written. Timer 1 counts at full speed but only once timer 0 runs.
//! How the board wires that is not known (a meter check of the 8254's CLK1/GATE1 pins would
//! tell); this model gates timer 1 with "timer 0 has a count and is counting".
//!
//! Timer 2 is the 8274's baud clock on every board (Serial~Device~'s table only works out
//! at 5 MHz). No timer output reaches an interrupt on any Compass.

use crate::machine::TimerChip;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Timers {
    /// true = discrete 8254 (113x), false = 80130.
    pub i8254: bool,
    /// Count last written to each counter (0 = 65536).
    pub count: [u16; 3],
    lsb_next: [bool; 3],
    /// Read/load mode (1 LSB, 2 MSB, 3 LSB then MSB) and counting mode from the control word.
    pub rl: [u8; 3],
    pub mode: [u8; 3],
    /// Counter clocks counted since the count was written, up to `start`.
    acc: [u64; 3],
    /// CPU clock since which the counter is counting (None = stopped or gated off).
    start: [Option<u64>; 3],
    /// 8254: a count was written since the last control word (else the counter holds `hold`).
    armed: [bool; 3],
    hold: [u16; 3],
    latch: [Option<u16>; 3],
    status_latch: [Option<u8>; 3],
    read_lsb: [bool; 3],
    /// 80130: timer 0 output pulses before timer 0's last load, and the pulse total when
    /// timer 1 was loaded (timer 1 counts timer 0's pulses).
    t0_pulses_before: u64,
    t1_pulses_at_load: u64,
}

impl Timers {
    pub fn new(chip: TimerChip) -> Timers {
        let i8254 = chip == TimerChip::I8254;
        Timers {
            i8254,
            count: [0; 3],
            lsb_next: [true; 3],
            // the 1137 reads status 00 before anything is written
            rl: if i8254 { [0; 3] } else { [3; 3] },
            mode: [0; 3],
            acc: [0; 3],
            // the 80130's counters run from power-on, the 8254's wait for a count
            start: if i8254 { [None; 3] } else { [Some(0); 3] },
            armed: [!i8254; 3],
            hold: [0xFFFF; 3],
            latch: [None; 3],
            status_latch: [None; 3],
            read_lsb: [true; 3],
            t0_pulses_before: 0,
            t1_pulses_at_load: 0,
        }
    }

    /// CPU clocks per counter clock.
    fn divider(&self, r: usize) -> u64 {
        if !self.i8254 && r == 0 { 2 } else { 1 }
    }

    fn n(&self, r: usize) -> u64 {
        if self.count[r] == 0 { 65536 } else { self.count[r] as u64 }
    }

    /// Counter clocks counter `r` has counted since its count was written.
    fn ticks(&self, r: usize, now: u64) -> u64 {
        if !self.i8254 && r == 1 {
            return self.t0_pulses(now).saturating_sub(self.t1_pulses_at_load);
        }
        self.acc[r] + self.start[r].map_or(0, |s| now.saturating_sub(s) / self.divider(r))
    }

    /// 80130: rising edges of timer 0's output since power-on.
    fn t0_pulses(&self, now: u64) -> u64 {
        self.t0_pulses_before + pulses(self.mode[0], self.n(0), self.ticks(0, now))
    }

    /// Live count of counter `r`.
    pub fn value(&self, r: usize, now: u64) -> u16 {
        if !self.armed[r] {
            return self.hold[r];
        }
        let n = self.n(r);
        let e = self.ticks(r, now);
        match self.mode[r] {
            2 | 6 => (n - e % n) as u16,
            3 | 7 => {
                let half = n.div_ceil(2);
                (n - 2 * (e % half)) as u16 & !1
            }
            _ => n.wrapping_sub(e) as u16,
        }
    }

    /// The counter's OUT pin (read-back status bit 7).
    fn out(&self, r: usize, now: u64) -> bool {
        if !self.armed[r] {
            return self.mode[r] != 0;
        }
        let n = self.n(r);
        let e = self.ticks(r, now);
        match self.mode[r] {
            0 | 1 => e >= n,
            3 | 7 => e % n < n.div_ceil(2),
            _ => true,
        }
    }

    /// (live count, loaded count, mode) of each counter, for the debugger.
    pub fn state(&self, now: u64) -> [(u16, u16, u8); 3] {
        [0, 1, 2].map(|r| (self.value(r, now), self.count[r], self.mode[r]))
    }

    /// 8254 timer 1 counts only while timer 0 counts.
    fn sync_gate(&mut self, now: u64) {
        if !self.i8254 {
            return;
        }
        let open = self.armed[0] && self.armed[1];
        match (open, self.start[1]) {
            (true, None) => self.start[1] = Some(now),
            (false, Some(s)) => {
                self.acc[1] += now.saturating_sub(s);
                self.start[1] = None;
            }
            _ => {}
        }
    }

    /// Read data port of counter `r`.
    pub fn read(&mut self, r: usize, now: u64) -> u8 {
        if let Some(s) = self.status_latch[r].take() {
            return s;
        }
        let v = self.latch[r].unwrap_or_else(|| self.value(r, now));
        let b = match self.rl[r] {
            1 => v as u8,
            2 => (v >> 8) as u8,
            _ => {
                let lsb = self.read_lsb[r];
                self.read_lsb[r] = !lsb;
                if lsb { v as u8 } else { (v >> 8) as u8 }
            }
        };
        // the latch is released once read out completely
        if self.rl[r] != 3 || self.read_lsb[r] {
            self.latch[r] = None;
        }
        b
    }

    /// Control word (port 0Eh).
    pub fn control(&mut self, data: u8, now: u64) {
        let c = (data >> 6) as usize;
        if c == 3 {
            // read-back: the 8254 only (the 80130 ignores it, real 1101 T tests)
            if self.i8254 {
                for r in 0..3 {
                    if data & (2 << r) == 0 {
                        continue;
                    }
                    if data & 0x20 == 0 && self.latch[r].is_none() {
                        self.latch[r] = Some(self.value(r, now));
                        self.read_lsb[r] = true;
                    }
                    if data & 0x10 == 0 && self.status_latch[r].is_none() {
                        let st = (self.out(r, now) as u8) << 7 | self.rl[r] << 4 | self.mode[r] << 1;
                        self.status_latch[r] = Some(st);
                    }
                }
            }
            return;
        }
        if (data >> 4) & 3 == 0 {
            // counter latch command
            if self.latch[c].is_none() {
                self.latch[c] = Some(self.value(c, now));
                self.read_lsb[c] = true;
            }
            return;
        }
        if self.i8254 {
            // the 8254 stops the counter until the new count is written
            self.hold[c] = self.value(c, now);
            self.armed[c] = false;
            if let Some(s) = self.start[c].take() {
                self.acc[c] += now.saturating_sub(s) / self.divider(c);
            }
            self.sync_gate(now);
        }
        self.rl[c] = (data >> 4) & 3;
        self.mode[c] = (data >> 1) & 7;
        self.lsb_next[c] = true;
        self.read_lsb[c] = true;
        self.latch[c] = None;
        self.status_latch[c] = None;
    }

    /// Data write to counter `r`. Returns true when the count is complete (loaded).
    pub fn write(&mut self, r: usize, data: u8, now: u64) -> bool {
        // 80130: the pulses timer 0 gave so far, with the count it had
        let pulses_now = if self.i8254 { 0 } else { self.t0_pulses(now) };
        match self.rl[r] {
            1 => self.count[r] = data as u16,
            2 => self.count[r] = (data as u16) << 8,
            _ => {
                if self.lsb_next[r] {
                    self.count[r] = (self.count[r] & 0xFF00) | data as u16;
                }
                else {
                    self.count[r] = (self.count[r] & 0x00FF) | (data as u16) << 8;
                }
                self.lsb_next[r] = !self.lsb_next[r];
            }
        }
        if self.rl[r] == 3 && !self.lsb_next[r] {
            return false;
        }
        if !self.i8254 && r == 0 {
            self.t0_pulses_before = pulses_now;
        }
        if !self.i8254 && r == 1 {
            self.t1_pulses_at_load = pulses_now;
        }
        self.acc[r] = 0;
        self.armed[r] = true;
        self.start[r] = Some(now);
        if self.i8254 && r == 1 {
            self.start[1] = None;
        }
        self.sync_gate(now);
        true
    }
}

/// Rising edges of OUT after `e` counter clocks of a counter in `mode` loaded with `n`.
fn pulses(mode: u8, n: u64, e: u64) -> u64 {
    match mode {
        2 | 3 | 6 | 7 => e / n,
        4 | 5 => (e > n) as u64,
        _ => (e >= n) as u64,
    }
}
