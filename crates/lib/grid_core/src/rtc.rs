//! National MM58174 real-time clock (DFF41-DFF5F, odd bytes, low nibble), after MAME mm58174.cpp.
//! The chip keeps tenths, seconds, minutes, hours, day, weekday and month; no year.
//! Emulated time runs from the host clock at power-on and advances with CPU clocks.
//! While the counters ripple after each tenth, a digit read returns 0Fh like the chip; the
//! PROM's CpRealTimeClock retries such reads (up to 5000 times).

use crate::screen::CPU_HZ;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Rtc {
    pub running: bool,
    tenths: u8,
    sec: u8,
    min: u8,
    hour: u8,
    day: u8,
    wday: u8,
    month: u8,
    /// Leap-year register (0 = leap year).
    leap: u8,
    int_ctl: u8,
    /// CPU clock of the last tenth.
    last: u64,
}

/// CPU clocks after each tenth during which digit reads return 0Fh: one period of the
/// 32.768 kHz crystal (~30.5 us). The real length is not measured; RTC_UPDATE_CLK overrides it.
fn update_clocks() -> u64 {
    static C: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *C.get_or_init(|| {
        std::env::var("RTC_UPDATE_CLK")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or((CPU_HZ / 32768.0).round() as u64)
    })
}

fn days_in_month(m: u8, leap: bool) -> u8 {
    match m {
        2 => if leap { 29 } else { 28 },
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl Rtc {
    /// Start at the host's current UTC date/time.
    pub fn new() -> Rtc {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let days = (secs / 86400) as i64;
        let tod = secs % 86400;
        // civil-from-days (Howard Hinnant)
        let z = days + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
        Rtc {
            running: true,
            tenths: 0,
            sec: (tod % 60) as u8,
            min: ((tod / 60) % 60) as u8,
            hour: (tod / 3600) as u8,
            day: d,
            wday: (((days + 4).rem_euclid(7)) as u8) + 1,
            month: m,
            leap: (y % 4) as u8,
            int_ctl: 0,
            last: 0,
        }
    }

    pub fn tick(&mut self, now: u64) {
        let tenth = (CPU_HZ / 10.0) as u64;
        if !self.running {
            self.last = now;
            return;
        }
        while now - self.last >= tenth {
            self.last += tenth;
            self.tenths += 1;
            if self.tenths < 10 {
                continue;
            }
            self.tenths = 0;
            self.sec += 1;
            if self.sec < 60 {
                continue;
            }
            self.sec = 0;
            self.min += 1;
            if self.min < 60 {
                continue;
            }
            self.min = 0;
            self.hour += 1;
            if self.hour < 24 {
                continue;
            }
            self.hour = 0;
            self.wday = self.wday % 7 + 1;
            self.day += 1;
            if self.day <= days_in_month(self.month, self.leap == 0) {
                continue;
            }
            self.day = 1;
            self.month += 1;
            if self.month > 12 {
                self.month = 1;
                self.leap = (self.leap + 1) % 4;
            }
        }
    }

    /// Read a register at CPU clock `now` (after `tick(now)`).
    pub fn read(&self, reg: usize, now: u64) -> u8 {
        let digit = (0x1..=0xC).contains(&(reg & 0xF));
        if digit && self.running && self.last != 0 && now - self.last < update_clocks() {
            return 0x0F;
        }
        match reg & 0xF {
            0x1 => self.tenths,
            0x2 => self.sec % 10,
            0x3 => self.sec / 10,
            0x4 => self.min % 10,
            0x5 => self.min / 10,
            0x6 => self.hour % 10,
            0x7 => self.hour / 10,
            0x8 => self.day % 10,
            0x9 => self.day / 10,
            0xA => self.wday,
            0xB => self.month % 10,
            0xC => self.month / 10,
            0xF => self.int_ctl,
            _ => 0,
        }
    }

    pub fn write(&mut self, reg: usize, data: u8) {
        let d = data & 0xF;
        let set = |v: &mut u8, units: bool, d: u8| {
            *v = if units { *v / 10 * 10 + d } else { d * 10 + *v % 10 };
        };
        match reg & 0xF {
            0x4 => set(&mut self.min, true, d),
            0x5 => set(&mut self.min, false, d & 7),
            0x6 => set(&mut self.hour, true, d),
            0x7 => set(&mut self.hour, false, d & 3),
            0x8 => set(&mut self.day, true, d),
            0x9 => set(&mut self.day, false, d & 3),
            0xA => self.wday = d & 7,
            0xB => set(&mut self.month, true, d),
            0xC => set(&mut self.month, false, d & 1),
            0xD => self.leap = d & 3,
            0xE => {
                // Start/stop; starting resets the tenths and seconds.
                let run = d & 1 != 0;
                if run && !self.running {
                    self.tenths = 0;
                    self.sec = 0;
                }
                self.running = run;
            }
            0xF => self.int_ctl = d,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_read_0f_while_updating() {
        let mut r = Rtc::new();
        let tenth = (CPU_HZ / 10.0) as u64;
        let t = 3 * tenth;
        r.tick(t);
        assert_eq!(r.read(0x1, t), 0x0F);
        assert_eq!(r.read(0x1, t + update_clocks() - 1), 0x0F);
        assert_eq!(r.read(0xF, t), 0); // control register is not a counter
        let n = t + update_clocks();
        r.tick(n);
        assert_eq!(r.read(0x1, n), 3);
    }
}
