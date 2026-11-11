//! Turning the Compass screen into pixels: colours, the demo tools overlay, screenshots.

use std::{fs, io::BufWriter};

pub const AMBER: u32 = 0xFFB040;
pub const DARK: u32 = 0x1A0E00;
/// Overlay colours (demo tools): text box and the beam tick of the last frame mark.
const OVL_FG: u32 = 0xFFFFFF;
const OVL_BG: u32 = 0x003040;
const BEAM_TICK: u32 = 0x40D0FF;
/// 8x8 font for the overlay (MartyPC's CGA font: row r of glyph g at r * 256 + g).
const FONT: &[u8] = include_bytes!("../../../lib/marty_core/assets/cga_8by8.bin");

pub fn draw_text(buf: &mut [u32], w: usize, x: usize, y: usize, text: &str) {
    for (i, ch) in text.bytes().enumerate() {
        let x0 = x + i * 8;
        if x0 + 8 > w {
            break;
        }
        for r in 0..8 {
            let bits = FONT[r * 256 + ch as usize];
            for c in 0..8 {
                let o = (y + r) * w + x0 + c;
                if o < buf.len() {
                    buf[o] = if bits & (0x80 >> c) != 0 { OVL_FG } else { OVL_BG };
                }
            }
        }
    }
}

/// Demo tools overlay: the cycle counter table at the top and a tick at the left edge on
/// the display line where the last frame mark was reached.
pub fn draw_overlay(buf: &mut [u32], w: usize, h: usize, marks: &grid_core::marks::Marks, frame_clocks: u64) {
    let mut lines = marks.lines(frame_clocks);
    if marks.is_empty() {
        lines = vec!["no marks: OUT 0F0h/0F2h,AL".into(), "F9 = clear  F10 = hide".into()];
    }
    for (i, l) in lines.iter().take(h / 8).enumerate() {
        draw_text(buf, w, 0, i * 8, l);
    }
    if marks.frame.count > 0 && (marks.frame_line as usize) < h {
        let y = marks.frame_line as usize;
        for x in 0..12.min(w) {
            buf[y * w + x] = BEAM_TICK;
        }
    }
}

/// A horizontal line across the screen (the beam's position while paused).
pub fn draw_beam(buf: &mut [u32], w: usize, h: usize, line: usize) {
    if line < h {
        for x in 0..w {
            buf[line * w + x] = BEAM_TICK;
        }
    }
}

/// Screen pixels (0/1) to colours.
pub fn colorize(pix: &[u8], fg: u32, bg: u32) -> Vec<u32> {
    pix.iter().map(|&p| if p != 0 { fg } else { bg }).collect()
}

pub fn save_png(path: &str, pix: &[u8], w: usize, h: usize) -> Result<(), String> {
    let f = fs::File::create(path).map_err(|e| format!("{path}: {e}"))?;
    let mut enc = png::Encoder::new(BufWriter::new(f), w as u32, h as u32);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|e| format!("{path}: {e}"))?;
    let data: Vec<u8> = pix.iter().map(|&p| if p != 0 { 0xFF } else { 0x00 }).collect();
    wr.write_image_data(&data).map_err(|e| format!("{path}: {e}"))
}

/// The next free compass-NNN.png name.
pub fn next_shot_name() -> String {
    (1..10000).map(|n| format!("compass-{n:03}.png")).find(|n| !std::path::Path::new(n).exists()).unwrap_or_else(|| "compass.png".into())
}
