//! Compass sound (all on the internal modem board; on the 1137, which has no modem, the DACs
//! and the MK5089 sit on the motherboard, assumed at the same addresses): the 8-bit sound DAC at DFE42 (DAC0832),
//! the volume DAC at DFE44 (modem / DTMF path), and the MK5089 DTMF generator on 8255 port B
//! (DFEC2: bits 0-2 column, bits 3-6 row, low = selected, bit 7 = tone on). Real-hardware
//! results (SoundTest, 2026-10-24): the sound DAC's level is its loudness and is not affected
//! by the volume DAC; DTMF needs bit 7 and exactly one row and one column.
//!
//! Output: mono f32 samples at `rate`, made by averaging the DAC level over each sample period
//! (so fast PCM loops come out right), DC removed, plus DTMF sine pairs scaled by the volume
//! DAC. Samples collect in `out` for the frontend to play or save.

use std::collections::VecDeque;

const ROWS: [f64; 4] = [697.0, 770.0, 852.0, 941.0];
const COLS: [f64; 3] = [1209.0, 1336.0, 1477.0];

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Sound {
    pub rate: u32,
    pub cpu_hz: f64,
    pub dac: u8,
    pub volume: u8,
    pub port_b: u8,
    /// 8255 port C (bit 0 voice mode, bit 1 modem MCU reset), control writes only.
    pub port_c: u8,
    /// Samples ready for the host.
    pub out: VecDeque<f32>,
    /// Keep at most this many samples (older ones are dropped, e.g. in turbo).
    pub max_out: usize,
    /// Produce samples at all (off for headless runs that don't want them).
    pub enabled: bool,
    clocks_per_sample: f64,
    next_sample: f64,
    acc: f64,
    last_t: u64,
    dc: f64,
    ph_row: f64,
    ph_col: f64,
}

impl Sound {
    pub fn new(cpu_hz: f64, rate: u32) -> Sound {
        Sound {
            rate,
            cpu_hz,
            dac: 0,
            volume: 0,
            port_b: 0x7F,
            port_c: 0,
            out: VecDeque::new(),
            max_out: rate as usize, // one second
            enabled: true,
            clocks_per_sample: cpu_hz / rate as f64,
            next_sample: cpu_hz / rate as f64,
            acc: 0.0,
            last_t: 0,
            dc: 0.0,
            ph_row: 0.0,
            ph_col: 0.0,
        }
    }

    /// The DTMF pair sounding now, if any.
    pub fn dtmf(&self) -> Option<(f64, f64)> {
        let b = self.port_b;
        if b & 0x80 == 0 {
            return None;
        }
        let cols = !b & 7;
        let rows = (!b >> 3) & 15;
        if cols.count_ones() == 1 && rows.count_ones() == 1 {
            Some((ROWS[rows.trailing_zeros() as usize], COLS[cols.trailing_zeros() as usize]))
        }
        else {
            None
        }
    }

    /// Next clock at which advance() must run.
    #[inline]
    pub fn next_event(&self) -> u64 {
        if self.enabled {
            self.next_sample as u64
        }
        else {
            u64::MAX
        }
    }

    /// Produce samples up to CPU clock `now`.
    pub fn advance(&mut self, now: u64) {
        if !self.enabled {
            self.last_t = now;
            return;
        }
        while (now as f64) >= self.next_sample {
            let end = self.next_sample;
            let lvl = self.dac as f64 / 255.0;
            self.acc += lvl * (end - self.last_t as f64).max(0.0);
            let mut v = self.acc / self.clocks_per_sample;
            self.acc = 0.0;
            self.last_t = end as u64;
            // remove DC (one-pole high-pass, ~10 Hz)
            self.dc += (v - self.dc) * (2.0 * std::f64::consts::PI * 10.0 / self.rate as f64);
            v -= self.dc;
            if let Some((r, c)) = self.dtmf() {
                let a = 0.25 * self.volume as f64 / 255.0;
                v += a * ((self.ph_row * 2.0 * std::f64::consts::PI).sin() + (self.ph_col * 2.0 * std::f64::consts::PI).sin());
                self.ph_row = (self.ph_row + r / self.rate as f64).fract();
                self.ph_col = (self.ph_col + c / self.rate as f64).fract();
            }
            if self.out.len() >= self.max_out {
                self.out.pop_front();
            }
            self.out.push_back(v as f32);
            self.next_sample += self.clocks_per_sample;
        }
        let lvl = self.dac as f64 / 255.0;
        self.acc += lvl * (now.saturating_sub(self.last_t)) as f64;
        self.last_t = now;
    }

    pub fn write_dac(&mut self, v: u8, now: u64) {
        self.advance(now);
        self.dac = v;
    }

    pub fn write_volume(&mut self, v: u8, now: u64) {
        self.advance(now);
        self.volume = v;
    }

    /// 8255 write: `reg` 0 port A, 1 port B, 2 port C, 3 control.
    pub fn write_8255(&mut self, reg: usize, v: u8, now: u64) {
        self.advance(now);
        match reg {
            1 => self.port_b = v,
            2 => self.port_c = v,
            3 => {
                if v & 0x80 == 0 {
                    // bit set/reset on port C
                    let bit = (v >> 1) & 7;
                    if v & 1 != 0 {
                        self.port_c |= 1 << bit;
                    }
                    else {
                        self.port_c &= !(1 << bit);
                    }
                }
                else {
                    // mode set: all ports reset to 0
                    self.port_b = 0;
                    self.port_c = 0;
                }
            }
            _ => {}
        }
    }

    pub fn read_8255(&self, reg: usize) -> u8 {
        match reg {
            1 => self.port_b,
            2 => self.port_c,
            _ => 0,
        }
    }
}
