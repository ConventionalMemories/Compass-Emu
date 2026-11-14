//! Machine setup page: saved machine profiles (compass-profiles.json next to the program)
//! and the form for one of them.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use grid_core::GridModel;
use serde::{Deserialize, Serialize};

use super::App;
use crate::session::MachineConfig;

const MODELS: &[(&str, &str)] = &[
    ("1101", "1101 (256K, 320 x 240)"),
    ("1109", "1109 (512K, 320 x 240)"),
    ("1121", "1121 Compass II (256K)"),
    ("1129", "1129 Compass II (512K)"),
    ("1131", "1131 Compass II (256K, 512 x 256)"),
    ("1137", "1137 Compass II (512K, 512 x 256, no modem)"),
    ("1139", "1139 Compass II (512K, 512 x 256)"),
];

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct ProfileFile {
    selected: usize,
    profiles: Vec<MachineConfig>,
}

pub struct SetupPage {
    file: PathBuf,
    data: ProfileFile,
    /// The profile list changed since it was saved.
    dirty: bool,
    message: Option<String>,
}

fn profile_path() -> PathBuf {
    crate::exe_dir().join("compass-profiles.json")
}

/// First profiles: the machines the package's .bat files start, where their disks exist,
/// else a 1101 without disks.
fn first_profiles() -> Vec<MachineConfig> {
    let mut v = vec![];
    for (model, disk) in [("1101", "..\\disks\\Compass-Emu 1101 disk.img"), ("1139", "..\\disks\\Compass-Emu 1139 disk.img")] {
        let disk = if cfg!(windows) { disk.to_string() } else { disk.replace('\\', "/") };
        if Path::new(&disk).exists() {
            let mut c = MachineConfig {
                name: format!("Compass {model} with 2101"),
                model: model.into(),
                hd: disk,
                bubble: if cfg!(windows) { "..\\disks\\Compass-Emu bubble.img".into() } else { "../disks/Compass-Emu bubble.img".into() },
                serial: "tcp:2323".into(),
                folder: "PC Folder".into(),
                ..Default::default()
            };
            if model == "1139" {
                for i in 0..4 {
                    let r = PathBuf::from("roms").join(format!("Compass_Socket_{i}.bin"));
                    if r.exists() {
                        c.roms[i] = r.display().to_string();
                    }
                }
            }
            v.push(c);
        }
    }
    if v.is_empty() {
        v.push(MachineConfig { name: "Compass 1101".into(), model: "1101".into(), ..Default::default() });
    }
    v
}

impl SetupPage {
    pub fn load() -> SetupPage {
        let file = profile_path();
        let mut data: ProfileFile = std::fs::read(&file).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default();
        let fresh = data.profiles.is_empty();
        if fresh {
            data.profiles = first_profiles();
        }
        data.selected = data.selected.min(data.profiles.len() - 1);
        SetupPage { file, data, dirty: fresh, message: None }
    }

    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        match serde_json::to_vec_pretty(&self.data).map_err(|e| e.to_string()).and_then(|d| std::fs::write(&self.file, d).map_err(|e| e.to_string())) {
            Ok(_) => self.dirty = false,
            Err(e) => self.message = Some(format!("could not save the profiles to {}: {e}", self.file.display())),
        }
    }

    pub fn current(&self) -> &MachineConfig {
        &self.data.profiles[self.data.selected]
    }

    /// The machine that just ran: keep what changed while it ran (a swapped floppy, the fast
    /// core) in its profile.
    pub fn remember(&mut self, cfg: &MachineConfig) {
        if let Some(p) = self.data.profiles.iter_mut().find(|p| p.name == cfg.name && !cfg.name.is_empty()) {
            if p != cfg {
                *p = cfg.clone();
                self.dirty = true;
            }
        }
    }
}

/// A path field with a Browse button.
fn path_row(ui: &mut egui::Ui, label: &str, value: &mut String, folder: bool, filters: &[&str]) -> bool {
    let mut changed = false;
    ui.label(label);
    ui.horizontal(|ui| {
        let w = (ui.available_width() - if value.is_empty() { 80.0 } else { 140.0 }).max(120.0);
        changed |= ui.add(egui::TextEdit::singleline(value).desired_width(w)).changed();
        if ui.button("Browse").clicked() {
            let mut d = rfd::FileDialog::new();
            if !filters.is_empty() {
                d = d.add_filter("image", filters);
            }
            let p = if folder { d.pick_folder() } else { d.pick_file() };
            if let Some(p) = p {
                *value = p.display().to_string();
                changed = true;
            }
        }
        if !value.is_empty() && ui.button("Clear").clicked() {
            value.clear();
            changed = true;
        }
    });
    changed
}

fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style()).inner_margin(12.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).strong().color(egui::Color32::from_rgb(0xFF, 0xC9, 0x4D)));
        ui.add_space(4.0);
        ui.spacing_mut().item_spacing.y = 5.0;
        add(ui);
    });
}

pub fn page(app: &mut App, ui: &mut egui::Ui) {
    let default_roms = app.default_roms.clone();
    let mut power = false;
    egui::Panel::left("profiles").resizable(true).default_size(260.0).show_inside(ui, |ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("Machine profiles").strong());
        ui.add_space(4.0);
        let sp = &mut app.setup;
        let mut pick = None;
        for (i, p) in sp.data.profiles.iter().enumerate() {
            let text = format!("{}\n{}", if p.name.is_empty() { "(no name)" } else { &p.name }, p.summary());
            if ui.selectable_label(i == sp.data.selected, text).clicked() {
                pick = Some(i);
            }
        }
        if let Some(i) = pick {
            sp.data.selected = i;
            sp.dirty = true;
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("New").clicked() {
                let n = sp.data.profiles.len() + 1;
                sp.data.profiles.push(MachineConfig { name: format!("Machine {n}"), model: "1101".into(), ..Default::default() });
                sp.data.selected = sp.data.profiles.len() - 1;
                sp.dirty = true;
            }
            if ui.button("Copy").clicked() {
                let mut c = sp.current().clone();
                c.name += " (copy)";
                sp.data.profiles.push(c);
                sp.data.selected = sp.data.profiles.len() - 1;
                sp.dirty = true;
            }
            if ui.add_enabled(sp.data.profiles.len() > 1, egui::Button::new("Delete")).clicked() {
                sp.data.profiles.remove(sp.data.selected);
                sp.data.selected = sp.data.selected.min(sp.data.profiles.len() - 1);
                sp.dirty = true;
            }
        });
    });
    egui::CentralPanel::default().show_inside(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let sp = &mut app.setup;
            let sel = sp.data.selected;
            let c = &mut sp.data.profiles[sel];
            let mut ch = false;
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Name");
                ch |= ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(320.0)).changed();
            });
            ui.add_space(6.0);
            let wide = ui.available_width() > 900.0;
            let cols = if wide { 2 } else { 1 };
            ui.columns(cols, |cols| {
                let (a, b) = if cols.len() == 2 {
                    let (l, r) = cols.split_at_mut(1);
                    (&mut l[0], Some(&mut r[0]))
                }
                else {
                    (&mut cols[0], None)
                };
                card(a, "Computer", |ui| {
                    ui.label("Model");
                    let cur = MODELS.iter().find(|m| m.0 == c.model).map(|m| m.1).unwrap_or("1101");
                    egui::ComboBox::from_id_salt("model").selected_text(cur).width(260.0).show_ui(ui, |ui| {
                        for (k, t) in MODELS {
                            if ui.selectable_label(c.model == *k, *t).clicked() {
                                c.model = k.to_string();
                                ch = true;
                            }
                        }
                    });
                    ui.label("CPU core");
                    ch |= ui.checkbox(&mut c.fast, "Fast (whole instructions, approximate timing)").changed();
                    ch |= path_row(ui, "ROM folder", &mut c.rom_dir, true, &[]);
                    let roms = grid_core::roms::find(c.model(), &c.rom_dir(&default_roms));
                    let missing: Vec<String> = [&roms.prom_even, &roms.prom_odd, &roms.keyboard]
                        .iter()
                        .filter(|p| !p.exists())
                        .map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
                        .collect();
                    if c.prom.is_empty() && c.prom_even.is_empty() && !missing.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(0xFF, 0x8A, 0x70), format!("Missing in the ROM folder: {}", missing.join(", ")));
                    }
                    else if let Some(n) = &roms.note {
                        ui.label(RichText::new(n).weak());
                    }
                    else {
                        ui.colored_label(egui::Color32::from_rgb(0x8F, 0xD1, 0x8F), "Boot PROM and keyboard firmware found");
                    }
                    ch |= path_row(ui, "Boot PROM (one 16K file, optional)", &mut c.prom, false, &[]);
                    ch |= path_row(ui, "Keyboard firmware (optional)", &mut c.kbd, false, &[]);
                });
                let col2 = match b {
                    Some(b) => b,
                    None => &mut *a,
                };
                card(col2, "Drives (GPIB)", |ui| {
                    ch |= path_row(ui, "2101 hard disk image", &mut c.hd, false, &["img", "IMG"]);
                    ch |= path_row(ui, "2102 floppy image", &mut c.flop, false, &["img", "IMG"]);
                    ch |= path_row(ui, "2101 floppy image", &mut c.flop2101, false, &["img", "IMG"]);
                    ch |= path_row(ui, "PC folder as the 2101 floppy", &mut c.folder, true, &[]);
                    ch |= path_row(ui, "Bubble memory image (made if missing)", &mut c.bubble, false, &[]);
                    ui.label("Disks");
                    ch |= ui.checkbox(&mut c.readonly, "Never write changes back to the image files").changed();
                });
            });
            ui.add_space(6.0);
            ui.columns(cols, |cols| {
                let split = cols.len() == 2;
                let (l, r) = cols.split_at_mut(1);
                let a = &mut l[0];
                card(a, "Application ROM sockets (Compass II)", |ui| {
                    let ok = GridModel::parse(&c.model).map(|m| m.split_board()).unwrap_or(false);
                    for i in 0..4 {
                        ch |= path_row(ui, &format!("Socket {i}"), &mut c.roms[i], false, &["bin", "BIN"]);
                    }
                    if !ok && c.roms.iter().any(|r| !r.is_empty()) {
                        ui.colored_label(egui::Color32::from_rgb(0xFF, 0x8A, 0x70), "The 1101 and 1109 have no ROM sockets");
                    }
                });
                let b = if split { &mut r[0] } else { &mut *a };
                card(b, "Serial and sound", |ui| {
                    ui.label("8274 channel A");
                    ui.vertical(|ui| {
                        ch |= ui.add(egui::TextEdit::singleline(&mut c.serial).hint_text("not connected").desired_width(260.0)).changed();
                        ui.label(RichText::new("tcp:2323 (wait for a connection), tcp:HOST:PORT, connect:2323 (null-modem link to another emulator) or a port name such as COM3").weak().small());
                    });
                    ui.label("GRiDLink (SDLC frames over TCP)");
                    ui.vertical(|ui| {
                        ch |= ui.add(egui::TextEdit::singleline(&mut c.gridlink).hint_text("not connected").desired_width(260.0)).changed();
                        ui.label(RichText::new("tcp:2400 (wait for a GRiDLink server program), tcp:HOST:PORT or connect:[HOST:]PORT; frames only, no server is built in").weak().small());
                        ch |= ui.checkbox(&mut c.gridlink_p2p, "Point-to-point cable (RTS looped to CTS) instead of multidrop").changed();
                    });
                    ui.label("Sound");
                    ch |= ui.checkbox(&mut c.mute, "No sound").changed();
                });
                card(b, "Debugging", |ui| {
                    ch |= path_row(ui, "Program symbols (folder of a debug build)", &mut c.symbols, true, &[]);
                    let mut catch = !c.no_catch;
                    if ui.checkbox(&mut catch, "Crash catcher (stop on GRiD-OS errors, divide error, INT 3, wild jumps)").changed() {
                        c.no_catch = !catch;
                        ch = true;
                    }
                });
            });
            if ch {
                sp.dirty = true;
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Save as shortcut (.bat)").clicked() {
                    let name = if c.name.is_empty() { "Compass".to_string() } else { c.name.clone() };
                    if let Some(p) = rfd::FileDialog::new().add_filter("Batch file", &["bat"]).set_file_name(format!("Start {name}.bat")).save_file() {
                        let args: Vec<String> = c.args().iter().map(|a| if a.contains(' ') || a.is_empty() { format!("\"{a}\"") } else { a.clone() }).collect();
                        let bat = format!("@echo off\r\nrem Compass-Emu machine \"{name}\"\r\ncd /d \"%~dp0\"\r\ncompass.exe {}\r\nif errorlevel 1 pause\r\n", args.join(" "));
                        sp.message = Some(match std::fs::write(&p, bat) {
                            Ok(_) => format!("wrote {}", p.display()),
                            Err(e) => format!("{}: {e}", p.display()),
                        });
                    }
                }
                ui.add_space(12.0);
                if ui.add(egui::Button::new(RichText::new("  Power on  ").strong()).fill(egui::Color32::from_rgb(0x8A, 0x63, 0x00))).clicked() {
                    power = true;
                }
            });
            if let Some(m) = &sp.message {
                ui.label(m);
            }
            ui.add_space(10.0);
            ui.label(RichText::new("Paths can be relative to the folder Compass-Emu was started in. Profiles are kept in compass-profiles.json next to compass.exe.").weak().small());
            if !app.log.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("Messages").strong());
                for l in app.log.iter().rev().take(8).collect::<Vec<_>>().into_iter().rev() {
                    ui.monospace(l);
                }
            }
        });
    });
    if power {
        app.setup.save();
        let cfg = app.setup.current().clone();
        app.power_on(cfg);
    }
}
