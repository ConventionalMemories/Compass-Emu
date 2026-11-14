//! Display timing. Patent US 4,571,456: 15 MHz crystal, CPU = /3 (5 MHz), pixel clock /2.
//! Geometry from GRiDProbe 0.0.13 on a real 1101: its beam finder, with the stripe's own row
//! loop counted, gives 306.8 clocks a line and 267.5 lines in the 82061-clock frame (82207
//! after the emulator's own ruler bias), the IR3 tick ~3 lines into the picture and IR3 low for
//! ~27 lines: 307 clocks x 268 lines (60.8 Hz at 5 MHz). (GRiDBench's beam finder, without
//! the row loop, had suggested ~306 shorter lines.) Everything here is in CPU clocks (5 MHz).

pub const CPU_HZ: f64 = 5_000_000.0;
/// CPU clocks per scan line (61.4 us).
pub const LINE_CLOCKS: u64 = 307;
/// Total lines per frame (60.8 Hz), visible lines.
pub const FRAME_LINES: u64 = 268;
pub const VISIBLE_LINES: u64 = 240;
/// IR3 rises at the start of this line (lines from the top of the picture) and stays high
/// for IR3_LEN lines: on a real 1101 it rises ~950 clocks after line 0 starts and is low
/// ~8200 clocks a frame (GRiDProbe d16/p65, l54).
pub const IR3_LINE: u64 = 3;
pub const IR3_LEN: u64 = 241;

fn default_ir3_len() -> u64 {
    IR3_LEN
}
pub const FRAME_CLOCKS: u64 = LINE_CLOCKS * FRAME_LINES;
/// Visible part of a line in CPU clocks (48 us).
pub const VISIBLE_LINE_CLOCKS: u64 = 240;

pub const SCREEN_BASE: usize = 0x400;
pub const SCREEN_W: usize = 320;
pub const SCREEN_H: usize = 240;

/// Words the display fetches per visible line (320 pixels / 16).
pub const LINE_WORDS: usize = SCREEN_W / 16;

/// Display RAM contention (patent US 4,571,456): one arbiter, clocked at 15 MHz (3 states per
/// CPU clock), shares the whole RAM between the CPU and the display. The display fetches one
/// 16-bit word every `fetch_period` states during the visible part of a visible line and holds
/// the RAM for `fetch_len` states; it always wins. A CPU memory access that would need the RAM
/// during a display slot waits (whole CPU clocks) until the slot is over. During horizontal and
/// vertical blank the CPU runs at full speed. All values are adjustable until they are measured
/// on a real 1101.
#[derive(Copy, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DisplayTiming {
    pub contention: bool,
    /// 15 MHz states between display fetches (36 = 2.4 us = 12 CPU clocks).
    pub fetch_period: u32,
    /// 15 MHz states the display holds the RAM for each fetch (~7 = 0.47 us).
    pub fetch_len: u32,
    /// 15 MHz states from the start of a line to its first fetch.
    pub fetch_offset: u32,
    /// 15 MHz states a CPU read needs the RAM, counted from the start of its T2.
    pub cpu_len: u32,
    /// The same for a CPU write.
    pub cpu_write_len: u32,
    /// Line on which IR3 (vertical sync) rises, and for how many lines it stays high. On the
    /// real 1101 it comes as the picture starts (the beam finder's stripe at delay 0 is at
    /// line ~5, after the OS has seen the tick).
    pub ir3_line: u64,
    #[serde(default = "default_ir3_len")]
    pub ir3_len: u64,
    /// Geometry: CPU clocks per line, lines per frame, visible lines, words fetched per
    /// visible line (pixels / 16).
    pub line_clocks: u64,
    pub frame_lines: u64,
    pub visible_lines: u64,
    pub line_words: usize,
    /// The display fetches on every line, not just the visible ones.
    #[serde(default)]
    pub all_lines: bool,
}

impl Default for DisplayTiming {
    fn default() -> Self {
        DisplayTiming {
            // Fitted with the bus steal in machine.rs (4 clocks every 48) and the JMP SHORT
            // quirk (machine.rs) to a real 1101's GRiDBench 2.2 gap rows (c50-c61, j01-j30,
            // REP per-word costs: 1.05% rms, 3% at most): one display word every 32 states on
            // every line (GRiDProbe: the slowdown is the same in the blank as in the picture);
            // a CPU access that starts inside a display slot waits for its end.
            contention: true,
            // 0.9.1: with the 307 x 268 geometry, slots of 6 states (gap rows 1.2% rms)
            fetch_period: 32,
            fetch_len: 6,
            fetch_offset: 0,
            cpu_len: 0,
            cpu_write_len: 0,
            ir3_line: IR3_LINE,
            ir3_len: IR3_LEN,
            line_clocks: LINE_CLOCKS,
            frame_lines: FRAME_LINES,
            visible_lines: VISIBLE_LINES,
            line_words: LINE_WORDS,
            all_lines: true,
        }
    }
}

/// The 113x (Compass II 1131/1139) large screen: 512 x 256. From GRiDBench 2.2 on a real 1137
/// (2026-11-12, RTC dead): the AAM loop measured in refreshes against the emulator's gives a
/// frame of ~83500 CPU clocks (59.9 Hz); GRiDProbe's beam finder (row loop counted) gives 316
/// clocks x 263 lines (60.2 Hz). 32 words a line (every line), one every 16 states (fitted to
/// the gap rows: 1.3% rms).
pub const WIDE_W: usize = 512;
pub const WIDE_H: usize = 256;

impl DisplayTiming {
    /// Timing of the 113x large screen (to be measured on a real 1131/1139).
    pub fn wide() -> DisplayTiming {
        DisplayTiming {
            fetch_period: 16,
            // 0.8.1 refit to the real 1137's gap rows (1.2% rms, was 2.0%): shorter display
            // slots, but a CPU read or write needs the RAM for 4 states, so RAM data accesses
            // cost ~1 clock more than on the 1101 (GRiDBench c06 15.3, c07 16.2), with the
            // DRAM refresh steal at 2 clocks (machine.rs).
            fetch_len: 3,
            cpu_len: 4,
            cpu_write_len: 4,
            // GRiDProbe 0.0.12 beam finder on a real 1137: 315.7 clocks a line, 262.8 lines a
            // frame (256 visible), line 0 ~930 clocks before IR3 rises; IR3 low only ~1000-1500
            // clocks a frame.
            line_clocks: 316,
            frame_lines: 263,
            ir3_len: 259,
            visible_lines: WIDE_H as u64,
            line_words: WIDE_W / 16,
            ..DisplayTiming::default()
        }
    }

    pub fn frame_clocks(&self) -> u64 {
        self.line_clocks * self.frame_lines
    }

    /// Bytes of screen RAM the display scans per frame.
    pub fn frame_bytes(&self) -> usize {
        self.visible_lines as usize * self.line_words * 2
    }

    /// Beam position at CPU clock `total_clocks`.
    pub fn beam(&self, total_clocks: u64) -> Beam {
        let fc = self.frame_clocks();
        let in_frame = total_clocks % fc;
        Beam {
            frame: total_clocks / fc,
            line: in_frame / self.line_clocks,
            clock: in_frame % self.line_clocks,
            vblank: in_frame / self.line_clocks >= self.visible_lines,
        }
    }

    /// Wait states for a CPU RAM access whose T2 starts at CPU clock `now`.
    #[inline]
    pub fn ram_wait(&self, now: u64, write: bool) -> u32 {
        if !self.contention {
            return 0;
        }
        let in_frame = now % self.frame_clocks();
        if !self.all_lines && in_frame >= self.visible_lines * self.line_clocks {
            return 0;
        }
        let x = (in_frame % self.line_clocks) as i64 * 3 - self.fetch_offset as i64;
        let w = if write { self.cpu_write_len } else { self.cpu_len } as i64;
        let (p, l) = (self.fetch_period as i64, self.fetch_len as i64);
        let k0 = if x < 0 { 0 } else { x / p };
        for k in [k0, k0 + 1] {
            if k >= self.line_words as i64 {
                break;
            }
            let s = k * p;
            if x < s + l && x + w > s {
                return ((s + l - x + 2) / 3) as u32;
            }
        }
        0
    }

    /// CPU clock (within a line) at which the display fetches word `k` of the line.
    pub fn fetch_clock(&self, k: usize) -> u64 {
        (self.fetch_offset as u64 + k as u64 * self.fetch_period as u64) / 3
    }
}

/// Beam position at a given CPU clock count since power-on.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Beam {
    pub frame: u64,
    pub line: u64,
    /// CPU clock within the line.
    pub clock: u64,
    vblank: bool,
}

impl Beam {
    /// Beam position on the 1101 screen.
    pub fn at(total_clocks: u64) -> Beam {
        DisplayTiming::default().beam(total_clocks)
    }
    pub fn in_vblank(&self) -> bool {
        self.vblank
    }
}

/// Convert screen RAM (9600 bytes from 0400h) to 1 byte per pixel (0/1).
/// Each 16-bit word is shown with bit 15 leftmost (as in MAME), so the odd byte comes first.
pub fn render(ram: &[u8]) -> Vec<u8> {
    render_into(ram, SCREEN_W, SCREEN_H, SCREEN_W, SCREEN_H)
}

/// Render a `w` x `h` bitmap from screen RAM (`ram` starts at address 0, the bitmap at 0400h)
/// into a `out_w` x `out_h` picture, centred (the 113x compatibility mode shows the 320 x 240
/// screen in the middle of its 512 x 256 panel).
pub fn render_into(ram: &[u8], w: usize, h: usize, out_w: usize, out_h: usize) -> Vec<u8> {
    let mut out = vec![0u8; out_w * out_h];
    let (px, py) = ((out_w - w) / 2, (out_h - h) / 2);
    for y in 0..h {
        for wx in 0..w / 16 {
            let a = SCREEN_BASE + (y * (w / 16) + wx) * 2;
            let word = ram.get(a).copied().unwrap_or(0) as u16 | (ram.get(a + 1).copied().unwrap_or(0) as u16) << 8;
            for b in 0..16 {
                out[(y + py) * out_w + px + wx * 16 + b] = ((word >> (15 - b)) & 1) as u8;
            }
        }
    }
    out
}
