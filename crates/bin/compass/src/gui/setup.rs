//! Machine setup page: saved machine profiles (compass-profiles.json next to the program)
//! and the form for one of them.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use grid_core::GridModel;
use serde::{Deserialize, Serialize};

use super::App;
use crate::session::{addr_choices, free_addr, grid_name, DriveRoms, GpibDevice, MachineConfig};

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
    /// Window and view settings, kept from one run to the next.
    gui: GuiPrefs,
}

/// How many disk images the Recent lists keep.
const RECENT: usize = 8;

/// View settings and recent files (the "gui" part of compass-profiles.json).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuiPrefs {
    /// Window size in points (0 = the first-run size).
    pub window: [f32; 2],
    pub maximized: bool,
    /// Whole-pixel scaling instead of filling the window.
    pub integer_scale: bool,
    /// Lit pixel colour, 0xRRGGBB.
    pub colour: u32,
    pub show_status: bool,
    pub show_toolbar: bool,
    /// Floppy images inserted lately, newest first.
    pub recent_floppies: Vec<String>,
    /// Hard disk images used lately, newest first.
    pub recent_hds: Vec<String>,
    /// Project > the project folder being built.
    pub project: String,
    pub recent_projects: Vec<String>,
    /// Development disk for builds ("" = the machine's hard disk).
    pub build_disk: String,
    /// View > EL panel look.
    pub el_look: bool,
    /// Keys by their position on a US keyboard (the older way) instead of by the letter
    /// printed on the PC key.
    pub keys_by_position: bool,
    /// Media > Floppy library: the folder it lists.
    pub floppy_library: String,
}

impl Default for GuiPrefs {
    fn default() -> Self {
        GuiPrefs {
            window: [0.0, 0.0],
            maximized: false,
            integer_scale: false,
            colour: crate::display::AMBER,
            show_status: true,
            show_toolbar: true,
            recent_floppies: vec![],
            recent_hds: vec![],
            project: String::new(),
            recent_projects: vec![],
            build_disk: String::new(),
            el_look: false,
            keys_by_position: false,
            floppy_library: String::new(),
        }
    }
}

/// Put a path at the top of a recent list.
pub fn push_recent(list: &mut Vec<String>, path: &str) {
    if path.is_empty() {
        return;
    }
    let full = std::fs::canonicalize(path).map(|p| p.display().to_string()).unwrap_or_else(|_| path.to_string());
    // canonicalize gives \\?\C:\... on Windows; keep the plain form.
    let full = full.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(full);
    list.retain(|p| p != &full);
    list.insert(0, full);
    list.truncate(RECENT);
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

    /// The saved view settings.
    pub fn prefs(&self) -> &GuiPrefs {
        &self.data.gui
    }

    /// Change the saved view settings (saved with the profiles).
    pub fn set_prefs(&mut self, p: GuiPrefs) {
        if p != self.data.gui {
            self.data.gui = p;
            self.dirty = true;
        }
    }

    /// The selected profile, to change (saved with the profiles).
    pub fn current_mut(&mut self) -> &mut MachineConfig {
        self.dirty = true;
        &mut self.data.profiles[self.data.selected]
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

const OK: egui::Color32 = egui::Color32::from_rgb(0x8F, 0xD1, 0x8F);
const BAD: egui::Color32 = egui::Color32::from_rgb(0xFF, 0x8A, 0x70);

/// One ROM: its role, where it comes from (or that it is missing) and its own file field.
fn rom_row(ui: &mut egui::Ui, role: &str, own: &mut String, used: Option<&Path>, needed: bool, missing_note: &str) -> bool {
    ui.label(RichText::new(role).strong());
    match used {
        Some(p) if p.exists() => ui.colored_label(OK, format!("{}", p.display())),
        Some(p) => ui.colored_label(BAD, format!("missing: {}", p.display())),
        None if needed => ui.colored_label(BAD, missing_note),
        None => ui.label(RichText::new(missing_note).weak()),
    };
    let ch = path_row_recent(ui, "", own, false, &[], &[]);
    ui.end_row();
    ch
}

/// The ROMs card: the ROM folder, every ROM the machine uses and where it was found, a file
/// field for each, and Import.
fn roms_card(ui: &mut egui::Ui, c: &mut MachineConfig, default_roms: &Path, import: &mut bool) -> bool {
    let mut ch = false;
    ui.horizontal(|ui| {
        ui.label("ROMs are looked for in the ROM folder under their usual names; a file set below is used instead.");
    });
    ch |= path_row(ui, "ROM folder (empty = roms next to compass.exe)", &mut c.rom_dir, true, &[]);
    ui.horizontal(|ui| {
        if ui.button("Import ROMs...").on_hover_text("pick a folder (a MAME roms folder, your own dumps, zip files in it): every Compass ROM found there by its checksum is copied into the ROM folder under the right name").clicked() {
            *import = true;
        }
        ui.label(RichText::new(format!("copies into {}", c.rom_dir(default_roms).display())).weak().small());
    });
    ui.add_space(4.0);
    let dir = c.rom_dir(default_roms);
    let found = grid_core::roms::find(c.model(), &dir);
    let dr = c.drive_roms(default_roms);
    egui::Grid::new("roms").num_columns(3).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
        // boot PROM: one combined file, else the even/odd pair from the folder
        let prom_used: PathBuf = if !c.prom.trim().is_empty() {
            PathBuf::from(c.prom.trim())
        }
        else if !found.prom_even.exists() {
            found.prom_even.clone()
        }
        else {
            found.prom_odd.clone()
        };
        ui.label(RichText::new("Boot PROM").strong());
        if c.prom.trim().is_empty() && found.prom_even.exists() && found.prom_odd.exists() {
            ui.colored_label(OK, format!("{} + {}", file_name(&found.prom_even), file_name(&found.prom_odd)));
        }
        else if prom_used.exists() {
            ui.colored_label(OK, format!("{}", prom_used.display()));
        }
        else {
            ui.colored_label(BAD, format!("missing: {}", prom_used.display()));
        }
        ch |= path_row_recent(ui, "", &mut c.prom, false, &[], &[]);
        ui.end_row();
        let kbd = if c.kbd.trim().is_empty() { found.keyboard.clone() } else { PathBuf::from(c.kbd.trim()) };
        ch |= rom_row(ui, "Keyboard (8741)", &mut c.kbd, Some(&kbd), true, "");
        let any = |k: &str| c.devices().iter().any(|d| d.enabled && d.kind == k && d.level != "protocol");
        let fd_note = if any("2102") { "not found: the 2102 runs as the protocol model (300237-02.bin)" } else { "not found (300237-02.bin); only used by a low-level 2102" };
        ch |= rom_row(ui, "2102 floppy firmware", &mut c.rom_2102, dr.fd2102.as_deref(), false, fd_note);
        let (h20, h10) = (dr.hd2101[0].clone(), dr.hd2101[1].clone());
        ch |= rom_row(ui, "2101 firmware, 20 MB box", &mut c.rom_2101_20, h20.as_deref(), false, "not found (300205-05.bin)");
        ch |= rom_row(ui, "2101 firmware, 10 MB box", &mut c.rom_2101_10, h10.as_deref(), false, "not found (300205-06.bin)");
        let modem_used = if c.rom_modem.trim().is_empty() { found.modem.clone() } else { Some(PathBuf::from(c.rom_modem.trim())) };
        ch |= rom_row(ui, "Internal modem (8749)", &mut c.rom_modem, modem_used.as_deref(), false, "not found (16020-007.bin): no modem MCU, Modem~Device~ can't start");
        let mc = crate::session::fpu_microcode(&MachineConfig { fpu_micro: true, ..c.clone() }, &dir).flatten();
        let mc_note = if c.fpu_micro { "not found: the instruction-level 8087 runs (8087mc.txt)" } else { "optional, only for Computer > 8087 microcode (8087mc.txt)" };
        ch |= rom_row(ui, "8087 microcode", &mut c.fpu_rom, mc.as_deref(), false, mc_note);
    });
    ui.label(RichText::new("A 2101 box takes the firmware that fits its hard disk image's size; a unit under GPIB devices can name its own file instead.").weak().small());
    ch
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// A path field with a Browse button.
fn path_row(ui: &mut egui::Ui, label: &str, value: &mut String, folder: bool, filters: &[&str]) -> bool {
    path_row_recent(ui, label, value, folder, filters, &[])
}

/// A path field with Browse, and a Recent menu when the list has entries.
fn path_row_recent(ui: &mut egui::Ui, label: &str, value: &mut String, folder: bool, filters: &[&str], recent: &[String]) -> bool {
    let mut changed = false;
    if !label.is_empty() {
        ui.label(label);
    }
    ui.horizontal(|ui| {
        let extra = if recent.is_empty() { 0.0 } else { 70.0 };
        let w = (ui.available_width() - extra - if value.is_empty() { 80.0 } else { 140.0 }).max(120.0);
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
        if !recent.is_empty() {
            ui.menu_button("Recent", |ui| {
                for p in recent {
                    if ui.button(p).clicked() {
                        *value = p.clone();
                        changed = true;
                    }
                }
            });
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

/// Which firmware a unit will run: (file, note). None: the protocol model runs.
fn unit_firmware(d: &GpibDevice, dr: &DriveRoms) -> (Option<PathBuf>, String) {
    if d.kind == "folder" {
        return (None, "answers GRiD's disk commands directly".into());
    }
    if d.level == "protocol" {
        return (None, "protocol model chosen".into());
    }
    if !d.rom.trim().is_empty() {
        let p = PathBuf::from(d.rom.trim());
        return if p.exists() { (Some(p.clone()), format!("firmware {}", file_name(&p))) } else { (None, format!("missing: {}", p.display())) };
    }
    if d.kind == "2102" {
        return match &dr.fd2102 {
            Some(p) if p.exists() => (Some(p.clone()), format!("firmware {} found", file_name(p))),
            _ => (None, "no firmware (300237-02): protocol model runs".into()),
        };
    }
    // 2101: the firmware that fits the hard disk's size
    let pages = if d.hd.trim().is_empty() { 0 } else { std::fs::metadata(d.hd.trim()).map(|m| m.len() as usize / 512).unwrap_or(0) };
    let ok = |p: &Option<PathBuf>| p.clone().filter(|p| p.exists());
    let (r05, r06) = (ok(&dr.hd2101[0]), ok(&dr.hd2101[1]));
    let pick = match pages {
        41616 => r05,
        20808 => r06,
        0 => r05.or(r06),
        _ => return (None, format!("{} KB hard disk fits no 2101 firmware: protocol model runs", pages / 2)),
    };
    match pick {
        Some(p) => (Some(p.clone()), format!("firmware {} found", file_name(&p))),
        None => (None, format!("no {} firmware: protocol model runs", if pages == 20808 { "300205-06" } else { "300205-05" })),
    }
}

/// "4 Hard Disk + 5 Floppy Disk" for a 2101, "6 Portable Floppy" for the others.
fn addr_text(kind: &str, a: u8) -> String {
    if kind == "2101" {
        format!("{a} {} + {} {}", grid_name(a), a + 1, grid_name(a + 1))
    }
    else {
        format!("{a} {}", grid_name(a))
    }
}

/// The bubble memory as a device: on/off, image, size of a new image, Make blank, write protect.
fn bubble_card(ui: &mut egui::Ui, c: &mut MachineConfig) -> bool {
    let mut ch = false;
    ui.horizontal(|ui| {
        let mut on = !c.bubble_off;
        if ui.checkbox(&mut on, "").on_hover_text("on: the bubble is in the machine at the next Power on; off: left out, settings kept").changed() {
            c.bubble_off = !on;
            ch = true;
        }
        ui.label(RichText::new("Intel 7220 bubble memory").strong());
        if c.bubble_off {
            ui.label(RichText::new("off for now").weak());
        }
        else if c.bubble.trim().is_empty() {
            ui.label(RichText::new("no image: no bubble in the machine").weak());
        }
    });
    ui.add_enabled_ui(!c.bubble_off, |ui| {
        ch |= path_row(ui, "Bubble image (raw, 128 KB per 7110 module)", &mut c.bubble, false, &[]);
        let p = PathBuf::from(c.bubble.trim());
        ui.horizontal(|ui| {
            match std::fs::metadata(&p) {
                Ok(m) if !c.bubble.trim().is_empty() => {
                    let kb = m.len() / 1024;
                    ui.label(format!("{kb} KB, {} module(s)", (m.len() as usize).div_ceil(grid_core::bubble::MBM_SIZE)));
                }
                _ if !c.bubble.trim().is_empty() => {
                    ui.label(RichText::new("not there yet: a blank one is made at Power on").weak());
                }
                _ => {}
            }
        });
        ui.horizontal(|ui| {
            ui.label("New image");
            let cur = if c.bubble_kb == 0 { 384 } else { c.bubble_kb };
            egui::ComboBox::from_id_salt("bubble-kb").selected_text(format!("{cur} KB")).width(90.0).show_ui(ui, |ui| {
                for kb in [128u32, 256, 384, 512] {
                    let label = if kb == 384 { "384 KB (1101)".to_string() } else { format!("{kb} KB") };
                    if ui.selectable_label(cur == kb, label).clicked() {
                        c.bubble_kb = if kb == 384 { 0 } else { kb };
                        ch = true;
                    }
                }
            });
            if ui.button("Make blank...").on_hover_text("writes an empty bubble image; format it on the Compass with Initialize Media").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("Bubble image", &["img"]).set_file_name("bubble.img").save_file() {
                    if std::fs::write(&f, vec![0u8; c.bubble_bytes()]).is_ok() {
                        c.bubble = f.display().to_string();
                        ch = true;
                    }
                }
            }
        });
        ch |= ui.checkbox(&mut c.bubble_protect, "Write protect (GRiD can write, the image is never saved)").changed();
    });
    ch
}

const HD_PROTECT: &str = "GRiD can write to the hard disk as usual, but the changes are never saved to the image file (a hard disk has no write-protect tab)";
const FD_PROTECT: &str = "the disk's write-protect tab: GRiD can read it but not change it (writes give error 106, as on a real disk); also while running, in the Media menu";

/// The GPIB devices: one card per unit, with an Add menu.
fn gpib_section(ui: &mut egui::Ui, c: &mut MachineConfig, dr: &DriveRoms, recent_hds: &[String], recent_floppies: &[String]) -> bool {
    let mut ch = false;
    if c.gpib.is_empty() && !c.devices().is_empty() {
        c.normalize();
        ch = true;
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("GPIB devices").strong().size(16.0).color(egui::Color32::from_rgb(0xFF, 0xC9, 0x4D)));
        ui.label(RichText::new("the units on the Compass's GPIB bus").weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("+ Add device", |ui| {
                let used: Vec<u8> = c.gpib.iter().filter(|d| d.enabled).flat_map(|d| d.addrs()).collect();
                for (kind, text) in [("2101", "2101 Disc System (hard disk + floppy)"), ("2102", "2102 Floppy"), ("folder", "PC folder drive")] {
                    if ui.button(text).clicked() {
                        c.gpib.push(GpibDevice::new(kind, free_addr(kind, &used)));
                        ch = true;
                        ui.close();
                    }
                }
            });
        });
    });
    for p in c.gpib_problems() {
        ui.colored_label(BAD, p);
    }
    if c.gpib.is_empty() {
        ui.label(RichText::new("No units: the Compass boots from its bubble memory or ROMs only. Add a 2101 for a hard disk.").weak());
    }
    let n2101 = c.gpib.iter().filter(|d| d.kind == "2101").count();
    let mut remove = None;
    for (i, d) in c.gpib.iter_mut().enumerate() {
        ui.add_space(4.0);
        egui::Frame::group(ui.style())
            .inner_margin(10.0)
            .fill(if d.enabled { ui.visuals().faint_bg_color } else { ui.visuals().extreme_bg_color })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ch |= ui.checkbox(&mut d.enabled, "").on_hover_text("on: part of the machine at the next Power on; off: left out, settings kept").changed();
                    let title = if d.kind == "2101" && n2101 > 1 && d.addr != 4 { format!("{} (second box)", d.title()) } else { d.title().to_string() };
                    ui.label(RichText::new(title).strong().size(14.0));
                    if !d.enabled {
                        ui.label(RichText::new("off for now").weak());
                    }
                    ui.add_space(8.0);
                    ui.label("GPIB");
                    egui::ComboBox::from_id_salt(("addr", i)).selected_text(addr_text(&d.kind, d.addr)).width(230.0).show_ui(ui, |ui| {
                        let mut list = addr_choices(&d.kind).to_vec();
                        if !list.contains(&d.addr) {
                            list.push(d.addr);
                        }
                        for a in list {
                            if ui.selectable_label(d.addr == a, addr_text(&d.kind, a)).clicked() {
                                d.addr = a;
                                ch = true;
                            }
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                });
                ui.add_enabled_ui(d.enabled, |ui| {
                    if d.kind != "folder" {
                        let (fw, note) = unit_firmware(d, dr);
                        ui.horizontal(|ui| {
                            ui.label("Emulation");
                            let mut low = d.level != "protocol";
                            let a = ui.selectable_value(&mut low, true, "Low level").on_hover_text("runs the unit's own firmware on its own CPU: real timing and behaviour (needs the firmware ROM)");
                            let b = ui.selectable_value(&mut low, false, "Protocol model").on_hover_text("answers GRiD's disk commands directly: no firmware needed");
                            if a.changed() || b.changed() {
                                d.level = if low { String::new() } else { "protocol".into() };
                                ch = true;
                            }
                            ui.add_space(8.0);
                            if fw.is_some() {
                                ui.colored_label(OK, note);
                            }
                            else if d.level == "protocol" {
                                ui.label(RichText::new(note).weak());
                            }
                            else {
                                ui.colored_label(BAD, note);
                            }
                        });
                        egui::CollapsingHeader::new(RichText::new("Own firmware file").small()).id_salt(("fw", i)).default_open(!d.rom.is_empty()).show(ui, |ui| {
                            ch |= path_row(ui, "", &mut d.rom, false, &[]);
                            ui.label(RichText::new("empty: the one from the ROMs card").weak().small());
                        });
                    }
                    let flop = &["img", "IMG", "imd", "IMD"];
                    match d.kind.as_str() {
                        "2101" => {
                            ui.columns(2, |cols| {
                                ch |= path_row_recent(&mut cols[0], &format!("Hard disk image ({} {})", d.addr, grid_name(d.addr)), &mut d.hd, false, &["img", "IMG"], recent_hds);
                                ch |= cols[0].checkbox(&mut d.hd_protect, "Write protect").on_hover_text(HD_PROTECT).changed();
                                let note = if d.disk.is_empty() { ": drive there, no disk (insert one any time)" } else { "" };
                                ch |= path_row_recent(&mut cols[1], &format!("Floppy disk ({} {}){note}", d.addr + 1, grid_name(d.addr + 1)), &mut d.disk, false, flop, recent_floppies);
                                ch |= cols[1].checkbox(&mut d.disk_protect, "Write protect").on_hover_text(FD_PROTECT).changed();
                            });
                        }
                        "2102" => {
                            let note = if d.disk.is_empty() { ": drive there, no disk (insert one any time)" } else { "" };
                            ch |= path_row_recent(ui, &format!("Floppy disk{note}"), &mut d.disk, false, flop, recent_floppies);
                            ch |= ui.checkbox(&mut d.disk_protect, "Write protect").on_hover_text(FD_PROTECT).changed();
                        }
                        _ => {
                            ch |= path_row(ui, "PC folder (its files show as a GRiD floppy disk)", &mut d.folder, true, &[]);
                            ch |= ui.checkbox(&mut d.disk_protect, "Write protect").on_hover_text("GRiD can change the files, but nothing is written back to the PC folder").changed();
                            ui.label(RichText::new("Its own GPIB unit, so a 2101 next to it stays low level. Put it at 13 Extra Floppy Disk next to a 2101 at 4, or at 5 Floppy Disk in place of the 2101's floppy drive.").weak().small());
                        }
                    }
                });
            });
    }
    if let Some(i) = remove {
        c.gpib.remove(i);
        ch = true;
    }
    if ch {
        c.normalize();
    }
    ch
}

pub fn page(app: &mut App, ui: &mut egui::Ui) {
    let default_roms = app.default_roms.clone();
    let recent_hds = app.setup.data.gui.recent_hds.clone();
    let recent_floppies = app.setup.data.gui.recent_floppies.clone();
    let mut power = false;
    egui::Panel::left("profiles").resizable(true).default_size(260.0).show_inside(ui, |ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("Machine profiles").strong());
        ui.add_space(4.0);
        let sp = &mut app.setup;
        let mut pick = None;
        let green = egui::Color32::from_rgb(0x2E, 0x7D, 0x32);
        // the panel's own width (the rows must not widen a resizable panel)
        let panel_w = ui.available_width();
        for (i, p) in sp.data.profiles.iter().enumerate() {
            let sel = i == sp.data.selected;
            let text = format!("{}\n{}", if p.name.is_empty() { "(no name)" } else { &p.name }, p.summary());
            ui.horizontal(|ui| {
                let bw = if sel { 92.0 } else { 30.0 };
                let w = (panel_w - bw - ui.spacing().item_spacing.x - 2.0).max(80.0);
                let r = ui.add(egui::Button::selectable(sel, RichText::new(text)).right_text("").min_size(egui::vec2(w, 38.0)).wrap_mode(egui::TextWrapMode::Wrap));
                if r.clicked() {
                    pick = Some(i);
                }
                if r.double_clicked() {
                    pick = Some(i);
                    power = true;
                }
                let b = if sel { egui::Button::new(RichText::new("▶ Power on").strong()).fill(green) } else { egui::Button::new("▶") };
                if ui.add_sized([bw, 38.0], b).on_hover_text(format!("Power on {}", p.name)).clicked() {
                    pick = Some(i);
                    power = true;
                }
            });
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
        ui.add_space(6.0);
        ui.label(RichText::new("Double-click a profile to power it on.").weak().small());
    });
    egui::CentralPanel::default().show_inside(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let sp = &mut app.setup;
            let sel = sp.data.selected;
            let c = &mut sp.data.profiles[sel];
            let mut ch = false;
            let mut import = false;
            ui.add_space(6.0);
            let mut shortcut = false;
            ui.horizontal(|ui| {
                ui.label("Name");
                ch |= ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(320.0)).changed();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("▶ Power on").strong()).fill(egui::Color32::from_rgb(0x2E, 0x7D, 0x32))).clicked() {
                        power = true;
                    }
                    if ui.button("Save as shortcut (.bat)...").clicked() {
                        shortcut = true;
                    }
                });
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
                    ch |= ui.checkbox(&mut c.fast, "Fast (whole instructions, approximate timing; switchable while running)").changed();
                    ui.label("8087");
                    ch |= ui
                        .checkbox(&mut c.fpu_micro, "Run on the 8087's own microcode ROM (you supply the ROM; switchable while running)")
                        .on_hover_text("slower, closer to the real chip; see docs/8087-microcode.md. Off: the instruction-level 8087")
                        .changed();
                });
                let col2 = match b {
                    Some(b) => b,
                    None => &mut *a,
                };
                card(col2, "Bubble memory", |ui| {
                    ch |= bubble_card(ui, c);
                });
            });
            ui.add_space(8.0);
            let dr = c.drive_roms(&default_roms);
            ch |= gpib_section(ui, c, &dr, &recent_hds, &recent_floppies);
            ui.add_space(6.0);
            card(ui, "ROMs", |ui| {
                ch |= roms_card(ui, c, &default_roms, &mut import);
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
                    ui.label("Internal modem phone line (see docs/modem.md)");
                    ui.vertical(|ui| {
                        ch |= ui.add(egui::TextEdit::singleline(&mut c.modem_listen).hint_text("no incoming calls").desired_width(260.0)).changed();
                        ui.label(RichText::new("incoming calls: tcp:2400 (a connection to this port rings the Compass) or tcp:HOST:PORT").weak().small());
                        ch |= ui.add(egui::TextEdit::singleline(&mut c.modem_dial).hint_text("phone book").desired_width(260.0)).changed();
                        ui.label(RichText::new("NUMBER=HOST:PORT, ... (* = any other number); 127*0*0*1*2400 dials that address directly").weak().small());
                        ch |= ui.checkbox(&mut c.modem_answer_300, "Callers connect at 300 baud (else 1200)").changed();
                        ch |= ui.checkbox(&mut c.modem_off, "Leave the modem MCU out").changed();
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
            if import {
                let dest = c.rom_dir(&default_roms);
                if let Some(src) = rfd::FileDialog::new().set_title("Folder with the ROMs (a MAME roms folder, your dumps...)").pick_folder() {
                    let log = crate::romimport::import(&src, &dest);
                    sp.message = Some(log.join("\n"));
                }
            }
            if ch {
                sp.dirty = true;
            }
            if shortcut {
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
            ui.add_space(10.0);
            if let Some(m) = &sp.message {
                ui.label(m);
            }
            ui.add_space(10.0);
            ui.label(RichText::new("Paths can be relative to the folder Compass-Emu was started in. Profiles are kept in compass-profiles.json next to compass.exe. Disk images and folders can also be dropped on this window.").weak().small());
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
