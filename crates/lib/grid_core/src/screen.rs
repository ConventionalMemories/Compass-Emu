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
