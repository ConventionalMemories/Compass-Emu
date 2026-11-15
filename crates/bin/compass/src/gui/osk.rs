//! The on-screen Compass keyboard: click a key to type it, hold the mouse button on it to
//! hold the key. SHIFT, CTRL and CODE stay on for the next key (click again to let go).

use eframe::egui::{self, Color32, RichText};
use grid_core::keyboard;

/// One key: label (shifted character over the plain one), the character `key_for_char`
/// knows it by ('\0' for SHIFT/CTRL/CODE, '\u{1}' for a gap), width in key units. Laid
/// out like the real Compass keyboard.
type K = (&'static str, char, f32);

const GAP: char = '\u{1}';

const ROWS: &[&[K]] = &[
    &[("", GAP, 0.55), ("!\n1", '1', 1.0), ("@\n2", '2', 1.0), ("#\n3", '3', 1.0), ("$\n4", '4', 1.0), ("%\n5", '5', 1.0), ("^\n6", '6', 1.0), ("&\n7", '7', 1.0), ("*\n8", '8', 1.0), ("(\n9", '9', 1.0), (")\n0", '0', 1.0), ("_\n-", '-', 1.0), ("+\n=", '=', 1.0), ("⬆", '\u{e003}', 1.0)],
    &[("TAB", '\t', 1.0), ("Q", 'q', 1.0), ("W", 'w', 1.0), ("E", 'e', 1.0), ("R", 'r', 1.0), ("T", 't', 1.0), ("Y", 'y', 1.0), ("U", 'u', 1.0), ("I", 'i', 1.0), ("O", 'o', 1.0), ("P", 'p', 1.0), ("\"\n'", '\'', 1.0), ("⬅", '\u{e000}', 1.0), ("➡", '\u{e002}', 1.0)],
    &[("", GAP, 0.2), ("ESC", '\x1b', 1.0), ("A", 'a', 1.0), ("S", 's', 1.0), ("D", 'd', 1.0), ("F", 'f', 1.0), ("G", 'g', 1.0), ("H", 'h', 1.0), ("J", 'j', 1.0), ("K", 'k', 1.0), ("L", 'l', 1.0), (":\n;", ';', 1.0), ("RETURN", '\r', 1.2), ("⬇", '\u{e001}', 1.0)],
    &[("SHIFT", '\0', 1.4), ("Z", 'z', 1.0), ("X", 'x', 1.0), ("C", 'c', 1.0), ("V", 'v', 1.0), ("B", 'b', 1.0), ("N", 'n', 1.0), ("M", 'm', 1.0), ("<\n,", ',', 1.0), (">\n.", '.', 1.0), ("?\n/", '/', 1.0), ("SHIFT", '\0', 1.2)],
    &[("CTRL", '\0', 1.0), ("CODE", '\0', 1.2), ("", ' ', 7.9), ("CODE", '\0', 1.2), ("BACK\nSPACE", '\x08', 1.0)],
];

/// Key cap and label colours, as on the real keyboard.
const CAP: Color32 = Color32::from_rgb(0x2B, 0x29, 0x27);
const WHITE: Color32 = Color32::from_rgb(0xE8, 0xE4, 0xDA);
const YELLOW: Color32 = Color32::from_rgb(0xF2, 0xC2, 0x3A);
const ORANGE: Color32 = Color32::from_rgb(0xF0, 0x5A, 0x28);

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
        egui::Window::new("Compass keyboard").open(&mut open).resizable(false).frame(egui::Frame::window(&ctx.global_style()).fill(Color32::from_rgb(0x1A, 0x19, 0x18))).show(ctx, |ui| {
            let unit = 34.0;
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for row in ROWS {
                ui.horizontal(|ui| {
                    for &(label, c, w) in row.iter() {
                        let size = egui::vec2(unit * w + 4.0 * (w - 1.0).max(0.0), unit);
                        if c == GAP {
                            ui.allocate_exact_size(egui::vec2(unit * w - 4.0, unit), egui::Sense::hover());
                            continue;
                        }
                        let bit = match label {
                            "SHIFT" => keyboard::MOD_SHIFT,
                            "CTRL" => keyboard::MOD_CTRL,
                            "CODE" => keyboard::MOD_CODE,
                            _ => 0,
                        };
                        let latched = bit != 0 && self.sticky & bit != 0;
                        let colour = match label {
                            "CTRL" | "CODE" => ORANGE,
                            "TAB" | "ESC" | "SHIFT" | "RETURN" | "BACK\nSPACE" | "⬆" | "⬇" | "⬅" | "➡" => YELLOW,
                            _ => WHITE,
                        };
                        let small = label.chars().count() > 2 && !label.contains('\n') || label.starts_with("BACK");
                        let text = RichText::new(label).size(if small { 9.5 } else if label.contains('\n') { 11.0 } else { 14.0 }).color(colour);
                        let mut b = egui::Button::new(text).min_size(size).fill(CAP).stroke(egui::Stroke::new(1.0, Color32::from_rgb(0x48, 0x45, 0x40)));
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
