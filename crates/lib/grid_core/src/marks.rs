//! Cycle counter marks for demo work. A program writes to ports the Compass doesn't decode
//! (nothing happens there on a real machine, so the marks can stay in a release build):
//!
//!   OUT 0F0h,AL   start section AL (0-255)
//!   OUT 0F2h,AL   end section AL: its CPU clocks go into the section's statistics
//!   OUT 0F4h,AL   frame mark: how far into the frame (counted from the IR3 tick) and on
//!                 which display line this point was reached
//!
//! The emulator keeps count, last, min, average and max clocks for each section.

use std::collections::BTreeMap;

pub const PORT_START: u16 = 0xF0;
pub const PORT_END: u16 = 0xF2;
pub const PORT_FRAME: u16 = 0xF4;

#[derive(Clone, Default, Debug)]
pub struct Stat {
    pub count: u64,
    pub total: u64,
    pub min: u64,
    pub max: u64,
    pub last: u64,
}

impl Stat {
    fn add(&mut self, v: u64) {
        if self.count == 0 || v < self.min {
            self.min = v;
        }
        self.max = self.max.max(v);
        self.count += 1;
        self.total += v;
        self.last = v;
    }

    pub fn avg(&self) -> f64 {
        if self.count == 0 {
            0.0
        }
        else {
            self.total as f64 / self.count as f64
        }
    }
}

#[derive(Default)]
pub struct Marks {
    pub sections: BTreeMap<u8, Stat>,
    open: BTreeMap<u8, u64>,
    /// Frame marks: clocks after the IR3 tick.
    pub frame: Stat,
    /// Display line of the last frame mark.
    pub frame_line: u64,
    /// Frames that went by without a frame mark, between the first and the last mark.
    pub frames_missed: u64,
    last_frame: Option<u64>,
}

impl Marks {
    pub fn start(&mut self, id: u8, now: u64) {
        self.open.insert(id, now);
    }

    pub fn end(&mut self, id: u8, now: u64) {
        if let Some(t) = self.open.remove(&id) {
            self.sections.entry(id).or_default().add(now.saturating_sub(t));
        }
    }

    /// `since_tick`: clocks since the last IR3 tick; `frame`: number of that tick.
    pub fn frame_mark(&mut self, since_tick: u64, frame: u64, line: u64) {
        self.frame.add(since_tick);
        if let Some(l) = self.last_frame {
            self.frames_missed += frame.saturating_sub(l + 1);
        }
        self.last_frame = Some(frame);
        self.frame_line = line;
    }

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty() && self.frame.count == 0
    }

    pub fn reset(&mut self) {
        *self = Marks::default();
    }

    /// One short line for the window title.
    pub fn summary(&self, frame_clocks: u64) -> String {
        let mut s = String::new();
        if let Some((id, st)) = self.sections.iter().next() {
            s += &format!("s{id} {:.0} clk", st.avg());
        }
        if self.frame.count > 0 {
            if !s.is_empty() {
                s += ", ";
            }
            s += &format!("frame {:.0}%", self.frame.avg() * 100.0 / frame_clocks as f64);
        }
        s
    }

    /// Table lines (40 columns fit the 1101 screen).
    pub fn lines(&self, frame_clocks: u64) -> Vec<String> {
        let mut v = vec!["sec  count   last    min    avg    max".to_string()];
        for (id, st) in &self.sections {
            v.push(format!(
                "{id:3} {:6} {:6} {:6} {:6.0} {:6}",
                st.count.min(999_999),
                st.last,
                st.min,
                st.avg(),
                st.max
            ));
        }
        if self.frame.count > 0 {
            let pct = |c: f64| c * 100.0 / frame_clocks as f64;
            v.push(format!(
                "frame {:.0}% ({:.0}-{:.0}) line {} missed {}",
                pct(self.frame.avg()),
                pct(self.frame.min as f64),
                pct(self.frame.max as f64),
                self.frame_line,
                self.frames_missed
            ));
        }
        v
    }

    /// A fuller report (clocks and milliseconds) for the console, a file or the MCP hook.
    pub fn report(&self, frame_clocks: u64, cpu_hz: f64) -> String {
        let ms = |c: f64| c * 1000.0 / cpu_hz;
        let mut s = format!(
            "Cycle counter marks (CPU clocks; one frame = {frame_clocks} clocks, {:.2} ms)\n",
            ms(frame_clocks as f64)
        );
        s += "section     count       last        min        avg        max    avg ms  avg % frame\n";
        for (id, st) in &self.sections {
            s += &format!(
                "{id:7} {:9} {:10} {:10} {:10.1} {:10} {:9.3} {:12.1}\n",
                st.count,
                st.last,
                st.min,
                st.avg(),
                st.max,
                ms(st.avg()),
                st.avg() * 100.0 / frame_clocks as f64
            );
        }
        if self.frame.count > 0 {
            s += &format!(
                "frame marks: {} reached on average {:.1}% into the frame (min {:.1}%, max {:.1}%), last on display line {}, {} frames without a mark\n",
                self.frame.count,
                self.frame.avg() * 100.0 / frame_clocks as f64,
                self.frame.min as f64 * 100.0 / frame_clocks as f64,
                self.frame.max as f64 * 100.0 / frame_clocks as f64,
                self.frame_line,
                self.frames_missed
            );
        }
        if self.is_empty() {
            s += "(no marks yet: OUT 0F0h,AL / OUT 0F2h,AL around a section, OUT 0F4h,AL when a frame is drawn)\n";
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_and_frames() {
        let mut m = Marks::default();
        m.start(1, 100);
        m.end(1, 350);
        m.start(1, 1000);
        m.end(1, 1150);
        m.end(2, 5); // never started: ignored
        let s = &m.sections[&1];
        assert_eq!((s.count, s.min, s.max, s.last), (2, 150, 250, 150));
        assert_eq!(s.avg(), 200.0);
        m.frame_mark(500, 7, 40);
        m.frame_mark(600, 8, 41);
        m.frame_mark(550, 11, 42);
        assert_eq!((m.frame.count, m.frames_missed, m.frame_line), (3, 2, 42));
        assert!(!m.sections.contains_key(&2));
    }
}
