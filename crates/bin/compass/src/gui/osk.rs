//! The on-screen Compass keyboard: click a key to type it, hold the mouse button on it to
//! hold the key. SHIFT, CTRL and CODE stay on for the next key (click again to let go).

use eframe::egui::{self, Color32, RichText};
use grid_core::keyboard;

/// One key: label, the character `key_for_char` knows it by ('\0' for SHIFT/CTRL/CODE),
/// width in key units.
type K = (&'static str, char, f32);

const ROWS: &[&[K]] = &[
    &[("ESC", '\x1b', 1.2), ("1", '1', 1.0), ("2", '2', 1.0), ("3", '3', 1.0), ("4", '4', 1.0), ("5", '5', 1.0), ("6", '6', 1.0), ("7", '7', 1.0), ("8", '8', 1.0), ("9", '9', 1.0), ("0", '0', 1.0), ("-", '-', 1.0), ("=", '=', 1.0), ("BACK\nSPACE", '\x08', 1.6)],
    &[("TAB", '\t', 1.6), ("Q", 'q', 1.0), ("W", 'w', 1.0), ("E", 'e', 1.0), ("R", 'r', 1.0), ("T", 't', 1.0), ("Y", 'y', 1.0), ("U", 'u', 1.0), ("I", 'i', 1.0), ("O", 'o', 1.0), ("P", 'p', 1.0), ("'", '\'', 1.0), ("RETURN", '\r', 2.2)],
    &[("CTRL", '\0', 1.9), ("A", 'a', 1.0), ("S", 's', 1.0), ("D", 'd', 1.0), ("F", 'f', 1.0), ("G", 'g', 1.0), ("H", 'h', 1.0), ("J", 'j', 1.0), ("K", 'k', 1.0), ("L", 'l', 1.0), (";", ';', 1.0), ("/", '/', 1.0), ("⬆", '\u{e003}', 1.0), ("", ' ', 0.9)],
    &[("SHIFT", '\0', 2.3), ("Z", 'z', 1.0), ("X", 'x', 1.0), ("C", 'c', 1.0), ("V", 'v', 1.0), ("B", 'b', 1.0), ("N", 'n', 1.0), ("M", 'm', 1.0), (",", ',', 1.0), (".", '.', 1.0), ("⬅", '\u{e000}', 1.0), ("⬇", '\u{e001}', 1.0), ("➡", '\u{e002}', 1.0)],
    &[("CODE", '\0', 1.9), ("SPACE", ' ', 9.0), ("SHIFT", '\0', 2.3)],
];

#[derive(Default)]
pub struct Osk {
    pub show: bool,
    /// SHIFT/CTRL/CODE latched for the next key.
    pub sticky: u8,
    /// Keys the mouse holds down now.
    pub down: Vec<(usize, u8)>,
    /// Keys clicked since the last frame (with the modifiers for them).
    pub taps: Vec<((usize, u8), u8)>,
    /// A key that is still held keeps the latched modifiers until it is let go.
    hold_mods: u8,
}

impl Osk {
    /// Modifiers the on-screen keyboard adds now.
    pub fn mods(&self) -> u8 {
        if self.down.is_empty() {
            self.sticky
        }
        else {
            self.hold_mods
        }
    }

    pub fn window(&mut self, ctx: &egui::Context) {
        let mut open = self.show;
        let mut down = vec![];
        egui::Window::new("Compass keyboard").open(&mut open).resizable(false).show(ctx, |ui| {
            let unit = 34.0;
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for row in ROWS {
                ui.horizontal(|ui| {
                    for &(label, c, w) in row.iter() {
                        let size = egui::vec2(unit * w + 4.0 * (w - 1.0).max(0.0), unit);
                        if label.is_empty() {
                            ui.allocate_exact_size(size, egui::Sense::hover());
                            continue;
                        }
                        let bit = match label {
                            "SHIFT" => keyboard::MOD_SHIFT,
                            "CTRL" => keyboard::MOD_CTRL,
                            "CODE" => keyboard::MOD_CODE,
                            _ => 0,
                        };
                        let latched = bit != 0 && self.sticky & bit != 0;
                        let text = RichText::new(label).size(if label.len() > 2 { 10.0 } else { 14.0 });
                        let mut b = egui::Button::new(text).min_size(size);
                        if latched {
                            b = b.fill(Color32::from_rgb(0x8A, 0x5A, 0x10));
                        }
                        let r = ui.add_sized(size, b);
                        if bit != 0 {
                            if r.clicked() {
                                self.sticky ^= bit;
                            }
                            continue;
                        }
                        let Some((col, kb, _)) = keyboard::key_for_char(c) else { continue };
                        if r.is_pointer_button_down_on() {
                            down.push((col, kb));
                            if !self.down.contains(&(col, kb)) {
                                self.taps.push(((col, kb), self.sticky));
                                self.hold_mods = self.sticky;
                                self.sticky = 0;
                            }
                        }
                    }
                });
            }
            ui.label(RichText::new("Click a key to type it, hold the mouse button to hold it. SHIFT, CTRL and CODE stay on for the next key.").weak().size(11.0));
        });
        self.down = down;
        if !open {
            self.down.clear();
            self.sticky = 0;
        }
        self.show = open;
    }
}
