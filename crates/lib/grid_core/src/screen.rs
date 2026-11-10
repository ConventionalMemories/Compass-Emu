//! Display timing. Patent US 4,571,456 + YL3AKC's measurements: 15 MHz crystal, CPU = /3 (5 MHz),
//! pixel clock /2 (7.5 MHz), hsync 16.17 kHz (61.8 us line), vsync ~60.8 Hz, ~266 lines.
//! Everything here is in CPU clocks (5 MHz). To be calibrated on a real 1101.

pub const CPU_HZ: f64 = 5_000_000.0;
/// CPU clocks per scan line (61.8 us).
pub const LINE_CLOCKS: u64 = 309;
/// Total lines per frame, visible lines.
pub const FRAME_LINES: u64 = 266;
pub const VISIBLE_LINES: u64 = 240;
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
#[derive(Copy, Clone, Debug)]
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
    /// Line on which IR3 (vertical sync) rises; it falls at the start of line 0.
    pub ir3_line: u64,
}

impl Default for DisplayTiming {
    fn default() -> Self {
        DisplayTiming { contention: true, fetch_period: 36, fetch_len: 6, fetch_offset: 0, cpu_len: 3, cpu_write_len: 2, ir3_line: VISIBLE_LINES }
    }
}

impl DisplayTiming {
    /// Wait states for a CPU RAM access whose T2 starts at CPU clock `now`.
    #[inline]
    pub fn ram_wait(&self, now: u64, write: bool) -> u32 {
        if !self.contention {
            return 0;
        }
        let in_frame = now % FRAME_CLOCKS;
        if in_frame >= VISIBLE_LINES * LINE_CLOCKS {
            return 0;
        }
        let x = (in_frame % LINE_CLOCKS) as i64 * 3 - self.fetch_offset as i64;
        let w = if write { self.cpu_write_len } else { self.cpu_len } as i64;
        let (p, l) = (self.fetch_period as i64, self.fetch_len as i64);
        let k0 = if x < 0 { 0 } else { x / p };
        for k in [k0, k0 + 1] {
            if k >= LINE_WORDS as i64 {
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
}

impl Beam {
    pub fn at(total_clocks: u64) -> Beam {
        let frame = total_clocks / FRAME_CLOCKS;
        let in_frame = total_clocks % FRAME_CLOCKS;
        Beam {
            frame,
            line: in_frame / LINE_CLOCKS,
            clock: in_frame % LINE_CLOCKS,
        }
    }
    pub fn in_vblank(&self) -> bool {
        self.line >= VISIBLE_LINES
    }
}

/// Convert screen RAM (9600 bytes from 0400h) to 1 byte per pixel (0/1).
/// Each 16-bit word is shown with bit 15 leftmost (as in MAME), so the odd byte comes first.
pub fn render(ram: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; SCREEN_W * SCREEN_H];
    for y in 0..SCREEN_H {
        for wx in 0..SCREEN_W / 16 {
            let a = SCREEN_BASE + (y * (SCREEN_W / 16) + wx) * 2;
            let w = ram[a] as u16 | (ram[a + 1] as u16) << 8;
            for b in 0..16 {
                out[y * SCREEN_W + wx * 16 + b] = ((w >> (15 - b)) & 1) as u8;
            }
        }
    }
    out
}
