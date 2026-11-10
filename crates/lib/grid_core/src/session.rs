//! Driving a Compass from a program: type keys, run for a while, wait for the command line
//! prompt of the Development Executive, take screenshots. Used by build mode and by the
//! Claude Code hook (MCP server).

use std::collections::VecDeque;

use crate::{keyboard, screen::CPU_HZ, GridMachine};

/// Keys are held this long, and as long again released, so the 8741 scan sees each one.
const KEY_HOLD: f64 = 0.08;

/// Width of the "=>" prompt (two glyphs of the 6-pixel font).
const PROMPT_W: usize = 12;

/// The Development Executive's "=>" prompt as drawn by GRiD-OS (6 x 10 font).
const PROMPT_GLYPH: [&str; 7] = [
    ".......#....",
    ".......##...",
    ".#####.###..",
    ".......####.",
    ".#####.###..",
    ".......##...",
    ".......#....",
];

pub struct Session {
    pub m: GridMachine,
    /// (clock, key (column, bit) or None = release all, modifiers)
    keys: VecDeque<(u64, Option<(usize, u8)>, u8)>,
    /// The prompt seen by wait_prompt (top row).
    prompt: Option<(usize, Vec<Vec<u8>>)>,
    /// Keep sound and serial output for the caller (normally dropped).
    pub keep_output: bool,
}

/// Parse key text: plain characters plus {ret} {esc} {tab} {bs} {space} {left} {right} {up}
/// {down} and {code+x} {ctrl+x} {shift+x}. Returns (character, modifiers) pairs.
pub fn parse_keys(text: &str) -> Result<Vec<(char, u8)>, String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let (mut mods, mut ch) = (0u8, c);
        if c == '{' {
            let mut tok = String::new();
            for n in chars.by_ref() {
                if n == '}' {
                    break;
                }
                tok.push(n);
            }
            let (m, k) = match tok.split_once('+') {
                Some((m, k)) => (m.to_ascii_lowercase(), k.to_string()),
                None => (String::new(), tok.clone()),
            };
            mods = match m.as_str() {
                "code" => keyboard::MOD_CODE,
                "ctrl" => keyboard::MOD_CTRL,
                "shift" => keyboard::MOD_SHIFT,
                "" => 0,
                o => return Err(format!("unknown modifier {o} in {{{tok}}}")),
            };
            ch = match k.to_ascii_lowercase().as_str() {
                "ret" | "enter" => '\r',
                "esc" => '\x1b',
                "tab" => '\t',
                "bs" => '\x08',
                "space" => ' ',
                "left" => '\u{e000}',
                "down" => '\u{e001}',
                "right" => '\u{e002}',
                "up" => '\u{e003}',
                _ if k.chars().count() == 1 => k.chars().next().unwrap(),
                _ => return Err(format!("unknown key {{{tok}}}")),
            };
        }
        else if c == '\n' {
            ch = '\r';
        }
        if keyboard::key_for_char(ch).is_none() {
            return Err(format!("no Compass key for {ch:?}"));
        }
        out.push((ch, mods));
    }
    Ok(out)
}

impl Session {
    pub fn new(m: GridMachine) -> Session {
        Session { m, keys: VecDeque::new(), prompt: None, keep_output: false }
    }

    pub fn clocks(&self) -> u64 {
        self.m.board().clocks
    }

    pub fn seconds(&self) -> f64 {
        self.clocks() as f64 / CPU_HZ
    }

    /// Queue keys to type, starting now (after keys already queued).
    pub fn type_keys(&mut self, text: &str) -> Result<(), String> {
        let hold = (KEY_HOLD * CPU_HZ) as u64;
        let mut t = self.keys.back().map(|k| k.0 + hold).unwrap_or(self.clocks());
        for (ch, mut mods) in parse_keys(text)? {
            let (col, bit, shift) = keyboard::key_for_char(ch).unwrap();
            if shift {
                mods |= keyboard::MOD_SHIFT;
            }
            self.keys.push_back((t, Some((col, bit)), mods));
            self.keys.push_back((t + hold, None, 0));
            t += 2 * hold;
        }
        Ok(())
    }

    /// Keys still waiting to be typed.
    pub fn typing(&self) -> bool {
        !self.keys.is_empty()
    }

    /// Run for `clocks` CPU clocks, typing queued keys on time. Host-side output (sound,
    /// serial) is dropped.
    pub fn run(&mut self, clocks: u64) {
        let end = self.clocks() + clocks;
        while self.clocks() < end {
            let next_key = self.keys.front().map(|k| k.0).unwrap_or(u64::MAX);
            let step = end.min(next_key).saturating_sub(self.clocks()).clamp(1, 100_000);
            self.m.run_clocks(step);
            while let Some(&(t, key, mods)) = self.keys.front() {
                if t > self.clocks() {
                    break;
                }
                self.keys.pop_front();
                if let Some(k) = &mut self.m.board_mut().keyboard {
                    match key {
                        Some((col, bit)) => {
                            k.set_mods(mods);
                            k.set_key(col, bit, true);
                        }
                        None => k.release_all(),
                    }
                }
            }
            let keep = self.keep_output;
            let b = self.m.board_mut();
            if !keep {
                b.sound.out.clear();
                b.serial.tx_out.clear();
            }
            b.unhandled.clear();
        }
    }

    pub fn run_seconds(&mut self, s: f64) {
        self.run((s * CPU_HZ) as u64);
    }

    /// The screen now: (width, height, one byte per pixel, 0/1).
    pub fn screen(&self) -> (usize, usize, Vec<u8>) {
        let (w, h) = self.m.screen_size();
        (w, h, self.m.render_now())
    }

    /// Top rows of every "=>" prompt on screen (at the left edge of the picture), lowest last.
    pub fn prompts(&self) -> Vec<usize> {
        let (pw, ph, px) = self.screen();
        let (w, h) = self.m.screen_mode();
        let (x0, y0) = ((pw - w) / 2, (ph - h) / 2);
        let glyph: Vec<Vec<u8>> = PROMPT_GLYPH.iter().map(|r| r.bytes().map(|c| (c == b'#') as u8).collect()).collect();
        let row = |y: usize| &px[(y0 + y) * pw + x0..(y0 + y) * pw + x0 + PROMPT_W];
        // blank above; below only the "=" part, as the cursor (a small triangle under the
        // next character cell) reaches under the ">"
        let blank = |y: usize, n: usize| row(y)[..n].iter().all(|&p| p == 0);
        (1..h.saturating_sub(glyph.len()))
            .filter(|&y| blank(y - 1, PROMPT_W) && blank(y + glyph.len(), 6) && glyph.iter().enumerate().all(|(i, g)| row(y + i) == &g[..]))
            .collect()
    }

    /// Is the last line on screen (ignoring blank space below it) a bare prompt?
    fn prompt_at_bottom(&self) -> Option<usize> {
        let (pw, ph, px) = self.screen();
        let (w, h) = self.m.screen_mode();
        let (x0, y0) = ((pw - w) / 2, (ph - h) / 2);
        let bottom = (0..h).rev().find(|&y| px[(y0 + y) * pw + x0..(y0 + y) * pw + x0 + w].iter().any(|&p| p != 0))?;
        let top = *self.prompts().last()?;
        // the prompt's line is the lowest one (a cursor may sit a few rows below it)
        (bottom < top + PROMPT_GLYPH.len() + 4).then_some(top)
    }

    /// Wait (up to `max_s` emulated seconds) until the screen ends with a "=>" prompt that has
    /// not changed for two seconds.
    pub fn wait_prompt(&mut self, max_s: f64) -> bool {
        let end = self.seconds() + max_s;
        let (mut last, mut same) = (None, 0);
        while self.seconds() < end {
            self.run_seconds(1.0);
            let p = self.prompt_at_bottom();
            if std::env::var("GRID_DEBUG").is_ok() {
                eprintln!("t={:.0} prompts {:?} bottom {:?}", self.seconds(), self.prompts(), p);
            }
            same = if p.is_some() && p == last { same + 1 } else { 0 };
            last = p;
            if same >= 2 {
                self.prompt = Some((p.unwrap(), Vec::new()));
                return true;
            }
        }
        false
    }

    /// After typing a command: wait (up to `max_s` emulated seconds) until a new prompt ends
    /// the screen (lower than the one seen by `wait_prompt`, or after the screen scrolled),
    /// twice in a row one second apart. Returns the emulated seconds it took, or None.
    pub fn wait_prompt_back(&mut self, max_s: f64) -> Option<f64> {
        let ptop = self.prompt.as_ref().map(|p| p.0).unwrap_or(0);
        let start = self.seconds();
        let mut hits = 0;
        // a scroll moves the old prompt up: it is "new" once more than one prompt is seen
        while self.seconds() < start + max_s {
            self.run_seconds(1.0);
            if self.typing() || self.seconds() < start + 3.0 {
                continue;
            }
            let found = self.prompt_at_bottom().is_some_and(|top| top > ptop || self.prompts().len() > 1);
            hits = if found { hits + 1 } else { 0 };
            if hits >= 2 {
                return Some(self.seconds() - start);
            }
        }
        None
    }
}

/// Write a 0/1 bitmap as a grayscale PNG (white on black).
pub fn png_bytes(w: usize, h: usize, px: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        let data: Vec<u8> = px.iter().map(|&p| if p != 0 { 0xFF } else { 0x00 }).collect();
        enc.write_header().unwrap().write_image_data(&data).unwrap();
    }
    out
}
