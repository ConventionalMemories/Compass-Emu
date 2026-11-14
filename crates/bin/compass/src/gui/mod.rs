//! The Compass-Emu GUI (egui through eframe, OpenGL): menus, toolbar, the EL screen and a
//! status bar, plus the machine setup page, the media manager and the debugger workspace.

mod debugger;
mod media;
mod setup;

use std::{
    collections::HashSet,
    path::PathBuf,
    time::{Duration, Instant},
};

use eframe::egui::{self, Color32, Key, RichText};
use grid_core::{gpib::DriveKind, keyboard, screen};

use crate::{
    display,
    runner::Runner,
    session::{MachineConfig, Media, Session, Snapshot},
    WindowOpts,
    VERSION,
};

/// EL screen colours to pick from (lit pixel). The background stays the dark EL panel.
const COLOURS: &[(&str, u32)] = &[("Amber", display::AMBER), ("Yellow", 0xFFD860), ("White", 0xF0F0E8)];

pub fn run(start: Option<MachineConfig>, opts: &WindowOpts) -> Result<(), String> {
    let title = format!("Compass-Emu {VERSION}");
    let setup = setup::SetupPage::load();
    let prefs = setup.prefs().clone();
    let size = if prefs.window[0] >= 480.0 && prefs.window[1] >= 360.0 { prefs.window } else { [1100.0, 820.0] };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size(size)
            .with_min_inner_size([480.0, 360.0])
            .with_maximized(prefs.maximized)
            .with_drag_and_drop(true),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    let overlay = opts.overlay;
    let mut start = start;
    let mut setup = Some(setup);
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            let app = App::new(cc, setup.take().unwrap_or_else(setup::SetupPage::load), start.take(), overlay);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())
}

/// Which mouse/keyboard owner the keys go to.
#[derive(PartialEq)]
enum Fit {
    Fit,
    Integer,
}

pub struct App {
    default_roms: PathBuf,
    pub(crate) setup: setup::SetupPage,
    sess: Option<Session>,
    run: Option<Runner>,
    tex: Option<egui::TextureHandle>,
    fit: Fit,
    colour: u32,
    overlay: bool,
    show_status: bool,
    show_toolbar: bool,
    /// The window fills the monitor; menus, toolbar and status bar are hidden.
    fullscreen: bool,
    /// Frame number when full screen was last switched (the window takes a few frames).
    fullscreen_frame: u64,
    /// Window size (points) while neither maximized nor full screen, to save.
    window_size: [f32; 2],
    maximized: bool,
    /// Files dropped on the window, handled in the next frame.
    dropped: Vec<(PathBuf, bool)>,
    /// Files being dragged over the window (for the drop hint).
    hovering: usize,
    show_debugger: bool,
    show_chips: bool,
    show_media: bool,
    show_keys: bool,
    show_about: bool,
    media: media::MediaPage,
    dbg: debugger::DebugPage,
    /// Short message shown in the status bar for a few seconds.
    flash: Option<(String, Instant)>,
    snapshot: Option<Snapshot>,
    /// The emulator gets the keyboard (no text field has focus).
    owns_keys: bool,
    keys_down: HashSet<Key>,
    /// Keys pressed since the last frame, with the modifiers held at that moment.
    taps: Vec<(Key, u8)>,
    mods: u8,
    fkeys: Vec<Key>,
    /// Remove the IR3 breakpoint when it is hit ("Run to next frame" set it).
    pub(crate) dbg_once_ir3: bool,
    /// Last time each GPIB drive was busy (activity lights).
    drive_seen: Vec<Instant>,
    /// Lines of messages (what the console windows printed in 0.4).
    pub(crate) log: Vec<String>,
    #[cfg(windows)]
    _hwnd: Option<*mut core::ffi::c_void>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, setup: setup::SetupPage, start: Option<MachineConfig>, overlay: bool) -> App {
        let prefs = setup.prefs().clone();
        let mut style = (*cc.egui_ctx.global_style()).clone();
        style.visuals = egui::Visuals::dark();
        cc.egui_ctx.set_global_style(style);
        #[cfg(windows)]
        let hwnd = {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            match cc.window_handle().map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get() as *mut core::ffi::c_void),
                _ => None,
            }
        };
        #[cfg(windows)]
        if let Some(h) = hwnd {
            crate::winkeys::install(h);
            crate::winkeys::clear();
        }
        let mut app = App {
            default_roms: crate::default_path(""),
            setup,
            sess: None,
            run: None,
            tex: None,
            fit: if prefs.integer_scale { Fit::Integer } else { Fit::Fit },
            colour: if COLOURS.iter().any(|c| c.1 == prefs.colour) { prefs.colour } else { display::AMBER },
            overlay,
            show_status: prefs.show_status,
            show_toolbar: prefs.show_toolbar,
            fullscreen: false,
            fullscreen_frame: 0,
            window_size: prefs.window,
            maximized: prefs.maximized,
            dropped: vec![],
            hovering: 0,
            show_debugger: false,
            show_chips: false,
            show_media: false,
            show_keys: false,
            show_about: false,
            media: media::MediaPage::default(),
            dbg: debugger::DebugPage::default(),
            flash: None,
            snapshot: None,
            owns_keys: false,
            keys_down: HashSet::new(),
            taps: vec![],
            mods: 0,
            fkeys: vec![],
            drive_seen: vec![],
            dbg_once_ir3: false,
            log: vec![],
            #[cfg(windows)]
            _hwnd: hwnd,
        };
        if let Some(cfg) = start {
            app.power_on(cfg);
        }
        app
    }

    pub(crate) fn say(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        for l in msg.lines() {
            println!("{l}");
            self.log.push(l.to_string());
        }
        let first = msg.lines().last().unwrap_or("").to_string();
        self.flash = Some((first, Instant::now()));
        if self.log.len() > 2000 {
            self.log.drain(..self.log.len() - 2000);
        }
    }

    pub(crate) fn power_on(&mut self, cfg: MachineConfig) {
        self.power_off();
        let mut p = self.setup.prefs().clone();
        setup::push_recent(&mut p.recent_hds, &cfg.hd);
        setup::push_recent(&mut p.recent_floppies, &cfg.flop2101);
        setup::push_recent(&mut p.recent_floppies, &cfg.flop);
        self.setup.set_prefs(p);
        match Session::build(&cfg, &self.default_roms) {
            Ok(s) => {
                for n in s.notes.clone() {
                    self.say(n);
                }
                self.run = Some(Runner::new(&s.m));
                self.sess = Some(s);
                self.tex = None;
                self.snapshot = None;
                self.say(format!("GRiD Compass {} powered on", cfg.model().name()));
            }
            Err(e) => self.say(format!("could not start: {e}")),
        }
    }

    pub(crate) fn power_off(&mut self) {
        if let Some(mut s) = self.sess.take() {
            let b = s.m.board();
            if !b.marks.is_empty() {
                let r = b.marks.report(b.frame_clocks(), screen::CPU_HZ);
                let _ = std::fs::write("compass-marks.txt", &r);
                self.say("cycle counter report saved to compass-marks.txt");
            }
            for l in s.write_back() {
                self.say(l);
            }
            // The setup page shows what the machine had last (a swapped floppy included).
            self.setup.remember(&s.cfg);
            self.say("powered off");
        }
        self.run = None;
        self.snapshot = None;
    }

    fn restart(&mut self) {
        let Some(s) = &mut self.sess else { return };
        match s.restart(&self.default_roms) {
            Ok(log) => {
                let m = &self.sess.as_ref().unwrap().m;
                self.run = Some(Runner::new(m));
                for l in log {
                    self.say(l);
                }
                self.say("restarted");
            }
            Err(e) => {
                self.sess = None;
                self.run = None;
                self.say(format!("could not restart: {e}"));
            }
        }
    }

    fn toggle_pause(&mut self) {
        if let (Some(s), Some(r)) = (&mut self.sess, &mut self.run) {
            let out = r.command(&mut s.m, if r.paused { "g" } else { "p" });
            self.say(out.trim_end().to_string());
        }
    }

    fn toggle_turbo(&mut self) {
        if let (Some(s), Some(r)) = (&self.sess, &mut self.run) {
            r.set_turbo(&s.m, !r.turbo);
        }
    }

    fn screenshot(&mut self) {
        let Some(s) = &self.sess else { return };
        let (w, h) = s.m.screen_size();
        let pix = s.m.render_display();
        let name = display::next_shot_name();
        match display::save_png(&name, &pix, w, h) {
            Ok(_) => self.say(format!("saved {name}")),
            Err(e) => self.say(e),
        }
    }

    fn clear_marks(&mut self) {
        if let Some(s) = &mut self.sess {
            s.m.board_mut().marks.reset();
            self.say("cycle counter marks cleared");
        }
    }

    fn quick_save(&mut self) {
        if let Some(s) = &mut self.sess {
            self.snapshot = Some(s.snapshot());
            self.say("state kept (Machine > Quick load goes back to it)");
        }
    }

    fn quick_load(&mut self) {
        let (Some(s), Some(snap)) = (&mut self.sess, &self.snapshot) else { return };
        let r = s.restore(snap);
        match r {
            Ok(_) => {
                if let (Some(s), Some(r)) = (&self.sess, &mut self.run) {
                    r.resync(&s.m);
                }
                self.say("back to the kept state");
            }
            Err(e) => self.say(format!("could not go back: {e}")),
        }
    }

    fn save_state_file(&mut self) {
        let Some(s) = &mut self.sess else { return };
        let Some(p) = rfd::FileDialog::new().add_filter("Compass state", &["state"]).set_file_name("compass.state").save_file() else {
            return;
        };
        let data = s.m.save_state();
        match std::fs::write(&p, data) {
            Ok(_) => self.say(format!("state saved to {} (disks are not part of it)", p.display())),
            Err(e) => self.say(format!("{}: {e}", p.display())),
        }
        self.resync();
    }

    fn load_state_file(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("Compass state", &["state"]).pick_file() else {
            self.resync();
            return;
        };
        self.load_state_path(&p);
    }

    fn load_state_path(&mut self, p: &std::path::Path) {
        let r = std::fs::read(p).map_err(|e| e.to_string()).and_then(|d| match &mut self.sess {
            Some(s) => s.load_state_file(&d),
            None => Err("power the machine on first".into()),
        });
        match r {
            Ok(_) => self.say(format!("loaded {}", p.display())),
            Err(e) => self.say(format!("could not load {}: {e}", p.display())),
        }
        self.resync();
    }

    /// After something that took a while (a file dialog): don't try to catch up.
    pub(crate) fn resync(&mut self) {
        if let (Some(s), Some(r)) = (&self.sess, &mut self.run) {
            r.resync(&s.m);
        }
    }

    pub(crate) fn insert_floppy(&mut self, kind: DriveKind, path: Option<PathBuf>) {
        if let (Some(p), true) = (&path, self.sess.is_some()) {
            let mut pr = self.setup.prefs().clone();
            setup::push_recent(&mut pr.recent_floppies, &p.display().to_string());
            self.setup.set_prefs(pr);
        }
        let Some(s) = &mut self.sess else { return };
        match s.insert_floppy(kind, path.as_deref()) {
            Ok(log) => {
                for l in log {
                    self.say(l);
                }
            }
            Err(e) => self.say(e),
        }
        self.resync();
    }

    fn pick_floppy(&mut self, kind: DriveKind) {
        let p = rfd::FileDialog::new().add_filter("GRiD floppy image", &["img", "IMG", "imd", "IMD"]).pick_file();
        if p.is_some() {
            self.insert_floppy(kind, p);
        }
        else {
            self.resync();
        }
    }

    /// Files dropped on the window: floppy images go into the 2102 floppy drive (the 2101's
    /// with Shift held), a .state file is loaded, and with the machine off a hard disk image
    /// or a folder goes into the selected profile.
    fn handle_drops(&mut self) {
        for (p, shift) in std::mem::take(&mut self.dropped) {
            let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            let on = self.sess.is_some();
            let text = p.display().to_string();
            if p.is_dir() {
                if on {
                    self.say("a PC folder drive can only be set with the machine off (Machine > Power off)");
                }
                else {
                    let c = self.setup.current_mut();
                    c.folder = text.clone();
                    c.flop2101.clear();
                    self.say(format!("PC folder {text} set as the 2101 floppy of this profile"));
                }
                continue;
            }
            if ext == "state" {
                if on {
                    self.load_state_path(&p);
                }
                else {
                    self.say("power the machine on first, then drop the state file");
                }
                continue;
            }
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            let bubble = p.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().contains("bubble"));
            if matches!(ext.as_str(), "bat" | "exe" | "json" | "txt" | "png" | "zip") {
                self.say(format!("{text}: not a disk image or state file"));
            }
            else if bubble && !on {
                self.setup.current_mut().bubble = text.clone();
                self.say(format!("{text} set as this profile's bubble memory"));
            }
            else if size > 2 * 1024 * 1024 {
                if on {
                    self.say("the hard disk can only be changed with the machine off (Machine > Power off)");
                }
                else {
                    self.setup.current_mut().hd = text.clone();
                    let mut pr = self.setup.prefs().clone();
                    setup::push_recent(&mut pr.recent_hds, &text);
                    self.setup.set_prefs(pr);
                    self.say(format!("{text} set as this profile's 2101 hard disk"));
                }
            }
            else {
                let kind = if shift { DriveKind::Floppy2101 } else { DriveKind::Floppy2102 };
                if on {
                    self.insert_floppy(kind, Some(p.clone()));
                }
                else {
                    let c = self.setup.current_mut();
                    if shift {
                        c.flop2101 = text.clone();
                        c.folder.clear();
                    }
                    else {
                        c.flop = text.clone();
                    }
                    let mut pr = self.setup.prefs().clone();
                    setup::push_recent(&mut pr.recent_floppies, &text);
                    self.setup.set_prefs(pr);
                    self.say(format!("{text} set as this profile's {} floppy", if shift { "2101" } else { "2102" }));
                }
            }
        }
    }

    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        self.fullscreen = !self.fullscreen;
        self.fullscreen_frame = ctx.cumulative_frame_nr();
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        if self.fullscreen {
            self.say("full screen: double-click the screen to leave it");
        }
    }

    /// The view settings as they are now, for compass-profiles.json.
    fn store_prefs(&mut self, ctx: &egui::Context) {
        let vp = ctx.input(|i| i.viewport().clone());
        let full = vp.fullscreen.unwrap_or(self.fullscreen);
        // Left by the system (a window manager key): follow it.
        if self.fullscreen && vp.fullscreen == Some(false) && ctx.cumulative_frame_nr() > self.fullscreen_frame + 30 {
            self.fullscreen = false;
        }
        if !full {
            self.maximized = vp.maximized.unwrap_or(false);
            if !self.maximized {
                if let Some(r) = vp.inner_rect {
                    self.window_size = [r.width().round(), r.height().round()];
                }
            }
        }
        let mut p = self.setup.prefs().clone();
        p.window = self.window_size;
        p.maximized = self.maximized;
        p.integer_scale = self.fit == Fit::Integer;
        p.colour = self.colour;
        p.show_status = self.show_status;
        p.show_toolbar = self.show_toolbar;
        self.setup.set_prefs(p);
    }

    /// A Recent submenu that inserts the picked floppy image into a drive.
    fn recent_floppy_menu(&mut self, ui: &mut egui::Ui, kind: DriveKind, label: &str) {
        let list = self.setup.prefs().recent_floppies.clone();
        ui.add_enabled_ui(!list.is_empty(), |ui| {
            ui.menu_button(label, |ui| {
                for p in list {
                    let short = std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(p.clone());
                    if ui.button(short).on_hover_text(&p).clicked() {
                        self.insert_floppy(kind, Some(PathBuf::from(&p)));
                    }
                }
            });
        });
    }

    /// The keys of this frame to the Compass.
    fn feed_keys(&mut self) {
        let (Some(s), Some(r)) = (&mut self.sess, &mut self.run) else {
            self.taps.clear();
            return;
        };
        let mut down = vec![];
        let mut tapped = vec![];
        let mut mods = 0u8;
        if self.owns_keys {
            #[cfg(windows)]
            {
                let mut t = vec![];
                crate::winkeys::poll(&mut down, &mut t, &mut mods);
                tapped = t.into_iter().map(|p| (p, mods)).collect();
            }
            #[cfg(not(windows))]
            {
                for k in &self.keys_down {
                    if let Some(p) = matrix_pos(*k) {
                        down.push(p);
                    }
                }
                for (k, km) in &self.taps {
                    if let Some(p) = matrix_pos(*k) {
                        tapped.push((p, *km));
                    }
                }
                mods = self.mods;
            }
        }
        #[cfg(windows)]
        let _ = &self.mods;
        self.taps.clear();
        r.set_keys(&mut s.m, &down, &tapped, mods);
    }

    fn handle_fkeys(&mut self) {
        for k in std::mem::take(&mut self.fkeys) {
            match k {
                Key::F8 => self.toggle_pause(),
                Key::F9 => self.clear_marks(),
                Key::F10 => self.overlay = !self.overlay,
                Key::F11 => self.screenshot(),
                Key::F12 => self.toggle_turbo(),
                _ => {}
            }
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let on = self.sess.is_some();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Machine", |ui| {
                if !on && ui.button("Power on").clicked() {
                    let cfg = self.setup.current().clone();
                    self.power_on(cfg);
                }
                if on {
                    if ui.button("Power off (back to setup)").clicked() {
                        self.power_off();
                    }
                    if ui.button("Restart").clicked() {
                        self.restart();
                    }
                    ui.separator();
                    let paused = self.run.as_ref().map(|r| r.paused).unwrap_or(false);
                    if ui.button(if paused { "Go on  (F8)" } else { "Pause  (F8)" }).clicked() {
                        self.toggle_pause();
                    }
                    let mut turbo = self.run.as_ref().map(|r| r.turbo).unwrap_or(false);
                    if ui.checkbox(&mut turbo, "Turbo  (F12)").clicked() {
                        self.toggle_turbo();
                    }
                    if let Some(s) = &mut self.sess {
                        let mut fast = s.m.is_fast();
                        if ui.checkbox(&mut fast, "Fast CPU core (approximate timing)").changed() {
                            s.m.set_fast(fast);
                            s.cfg.fast = fast;
                        }
                    }
                    ui.separator();
                    if ui.button("Quick save").clicked() {
                        self.quick_save();
                    }
                    if ui.add_enabled(self.snapshot.is_some(), egui::Button::new("Quick load")).clicked() {
                        self.quick_load();
                    }
                    if ui.button("Save state to a file...").clicked() {
                        self.save_state_file();
                    }
                    if ui.button("Load state from a file...").clicked() {
                        self.load_state_file();
                    }
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Media", |ui| {
                if ui.button("Media manager...").clicked() {
                    self.show_media = true;
                }
                if on {
                    ui.separator();
                    self.recent_floppy_menu(ui, DriveKind::Floppy2102, "Recent into the 2102 drive");
                    if ui.button("Insert 2102 floppy...").clicked() {
                        self.pick_floppy(DriveKind::Floppy2102);
                    }
                    if ui.button("Take out 2102 floppy").clicked() {
                        self.insert_floppy(DriveKind::Floppy2102, None);
                    }
                    self.recent_floppy_menu(ui, DriveKind::Floppy2101, "Recent into the 2101 drive");
                    if ui.button("Insert 2101 floppy...").clicked() {
                        self.pick_floppy(DriveKind::Floppy2101);
                    }
                    if ui.button("Take out 2101 floppy").clicked() {
                        self.insert_floppy(DriveKind::Floppy2101, None);
                    }
                    ui.separator();
                    if ui.button("Write changed disks back now").clicked() {
                        let log = self.sess.as_mut().map(|s| s.write_back()).unwrap_or_default();
                        if log.is_empty() {
                            self.say("no disk has changes to write");
                        }
                        for l in log {
                            self.say(l);
                        }
                    }
                }
            });
            ui.menu_button("View", |ui| {
                ui.radio_value(&mut self.fit, Fit::Fit, "Fill the window");
                ui.radio_value(&mut self.fit, Fit::Integer, "Whole-pixel scaling");
                ui.separator();
                for (name, c) in COLOURS {
                    ui.radio_value(&mut self.colour, *c, *name);
                }
                ui.separator();
                ui.checkbox(&mut self.overlay, "Demo tools overlay  (F10)");
                ui.checkbox(&mut self.show_toolbar, "Toolbar");
                ui.checkbox(&mut self.show_status, "Status bar");
                ui.separator();
                if ui.button("Full screen  (double-click the screen)").clicked() {
                    let ctx = ui.ctx().clone();
                    self.toggle_fullscreen(&ctx);
                }
            });
            ui.menu_button("Debug", |ui| {
                ui.checkbox(&mut self.show_debugger, "Debugger workspace");
                ui.checkbox(&mut self.show_chips, "Chips");
                ui.separator();
                if ui.add_enabled(on, egui::Button::new("Clear cycle counter marks  (F9)")).clicked() {
                    self.clear_marks();
                }
                if ui.add_enabled(on, egui::Button::new("Profile the next 60 frames")).clicked() {
                    self.show_debugger = true;
                    debugger::command(self, "prof 60");
                }
                ui.separator();
                if ui.add_enabled(on, egui::Button::new("Load program symbols...")).on_hover_text("the project folder of a debug build (NAME.pas.obj files)").clicked() {
                    debugger::load_symbols_dialog(self);
                }
                if let Some(s) = &mut self.sess {
                    let d = &mut s.m.board_mut().debug;
                    ui.checkbox(&mut d.catch.on, "Crash catcher").on_hover_text("stop on GRiD-OS errors, divide error, INT 3 and wild jumps");
                    ui.checkbox(&mut d.os_log_on, "GRiD-OS call log");
                    s.cfg.no_catch = !s.m.board().debug.catch.on;
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard map").clicked() {
                    self.show_keys = true;
                }
                if ui.button("About Compass-Emu").clicked() {
                    self.show_about = true;
                }
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let paused = self.run.as_ref().map(|r| r.paused).unwrap_or(false);
            let turbo = self.run.as_ref().map(|r| r.turbo).unwrap_or(false);
            if ui.button(if paused { "Go on (F8)" } else { "Pause (F8)" }).clicked() {
                self.toggle_pause();
            }
            if ui.button("Restart").clicked() {
                self.restart();
            }
            if ui.selectable_label(turbo, "Turbo (F12)").clicked() {
                self.toggle_turbo();
            }
            if ui.button("Quick save").clicked() {
                self.quick_save();
            }
            if ui.add_enabled(self.snapshot.is_some(), egui::Button::new("Quick load")).clicked() {
                self.quick_load();
            }
            if ui.button("Screenshot (F11)").clicked() {
                self.screenshot();
            }
            ui.separator();
            if ui.selectable_label(self.show_media, "Media").clicked() {
                self.show_media = !self.show_media;
            }
            if ui.selectable_label(self.show_debugger, "Debugger").clicked() {
                self.show_debugger = !self.show_debugger;
            }
            if ui.selectable_label(self.overlay, "Overlay (F10)").clicked() {
                self.overlay = !self.overlay;
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let Some(s) = &mut self.sess else {
                ui.label("Powered off");
                return;
            };
            let r = self.run.as_ref().unwrap();
            let model = s.m.model;
            ui.label(format!("{} ({}K)", model.name(), model.ram_size() / 1024));
            ui.separator();
            if r.paused {
                ui.label(RichText::new("Paused").color(Color32::from_rgb(0xFF, 0xC9, 0x4D)));
            }
            else {
                ui.label(format!("Speed {:.0}%", r.speed * 100.0));
            }
            if r.turbo {
                ui.label(RichText::new("Turbo").color(Color32::from_rgb(0xFF, 0xC9, 0x4D)));
            }
            if s.m.is_fast() {
                ui.label("Fast core");
            }
            ui.label(format!("{:.0} fps", r.fps));
            ui.separator();
            let now = Instant::now();
            let drives = &s.m.board().gpib.drives;
            self.drive_seen.resize(drives.len(), now - Duration::from_secs(1));
            for (i, d) in drives.iter().enumerate() {
                if d.busy() {
                    self.drive_seen[i] = now;
                }
                let lit = now.duration_since(self.drive_seen[i]) < Duration::from_millis(120);
                let name = match (d.kind, &s.media[i]) {
                    (_, Media::Folder(_)) => "PC folder",
                    (DriveKind::Hdd2101, _) => "2101 HD",
                    (DriveKind::Floppy2101, _) => "2101 floppy",
                    (DriveKind::Floppy2102, _) => "2102 floppy",
                };
                if d.image.is_empty() {
                    continue;
                }
                let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(
                    rect.center(),
                    4.5,
                    if lit { Color32::from_rgb(0xFF, 0xB0, 0x00) } else { Color32::from_rgb(0x4A, 0x4A, 0x52) },
                );
                ui.label(if d.dirty { format!("{name} *") } else { name.to_string() });
                if let Some(t) = d.display() {
                    // The low-level 2101's four-character display.
                    ui.label(RichText::new(format!("[{t}]")).monospace()).on_hover_text("2101 display");
                }
            }
            if let Some(l) = &s.serial {
                ui.separator();
                ui.label(format!("Serial {} {}", s.cfg.serial, if l.connected() { "(connected)" } else { "(waiting)" }));
            }
            if let Some(l) = &s.gridlink {
                ui.separator();
                ui.label(format!(
                    "GRiDLink {} {}",
                    s.cfg.gridlink,
                    if l.connected() { format!("(connected, frames {} out / {} in)", l.frames_out, l.frames_in) } else { "(waiting)".into() }
                ));
            }
            let b = s.m.board();
            let marks = b.marks.summary(b.frame_clocks());
            if !marks.is_empty() {
                ui.separator();
                ui.label(marks);
            }
            if let Some((msg, t)) = &self.flash {
                if t.elapsed() < Duration::from_secs(6) {
                    ui.separator();
                    ui.label(RichText::new(msg).weak());
                }
            }
        });
    }

    /// The Compass screen, as large as fits. True when it was double-clicked (full screen).
    fn screen(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(s) = &mut self.sess else { return false };
        let (w, h) = s.m.screen_size();
        let pix = s.m.render_display();
        let mut buf = display::colorize(&pix, self.colour, display::DARK);
        let b = s.m.board();
        if self.overlay {
            display::draw_overlay(&mut buf, w, h, &b.marks, b.frame_clocks());
        }
        if self.run.as_ref().map(|r| r.paused).unwrap_or(false) && self.show_debugger {
            display::draw_beam(&mut buf, w, h, b.display.beam(b.clocks).line as usize);
        }
        let img = egui::ColorImage::new(
            [w, h],
            buf.iter().map(|&c| Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)).collect(),
        );
        let tex = match &mut self.tex {
            Some(t) if t.size() == [w, h] => {
                t.set(img, egui::TextureOptions::NEAREST);
                t.clone()
            }
            _ => {
                let t = ui.ctx().load_texture("compass-screen", img, egui::TextureOptions::NEAREST);
                self.tex = Some(t.clone());
                t
            }
        };
        let avail = ui.available_rect_before_wrap();
        let mut k = (avail.width() / w as f32).min(avail.height() / h as f32).max(0.1);
        if self.fit == Fit::Integer && k >= 1.0 {
            k = k.floor();
        }
        let size = egui::vec2(w as f32 * k, h as f32 * k);
        let rect = egui::Rect::from_center_size(avail.center(), size);
        let resp = ui.allocate_rect(avail, egui::Sense::click());
        if resp.clicked() {
            ui.ctx().memory_mut(|m| m.stop_text_input());
        }
        if !self.fullscreen {
            ui.painter().rect_filled(rect.expand(10.0), 8.0, Color32::from_rgb(0x1A, 0x17, 0x12));
        }
        ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        resp.double_clicked()
    }

    fn keys_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("Keyboard map").open(&mut self.show_keys).resizable(false).show(ctx, |ui| {
            ui.label("PC keys work by position (as on a US keyboard), not by the letter your layout prints.");
            ui.add_space(4.0);
            ui.columns(2, |cols| {
                cols[0].label(RichText::new("Compass key").strong());
                egui::Grid::new("keys-compass").striped(true).num_columns(2).show(&mut cols[0], |ui| {
                    for (k, v) in [
                        ("A-Z, 0-9", "the same keys"),
                        ("- = , . / ; '", "the same keys"),
                        ("RETURN", "Enter"),
                        ("BACKSPACE", "Backspace"),
                        ("TAB", "Tab"),
                        ("ESC", "Esc"),
                        ("SPACE", "Space"),
                        ("arrows", "arrow keys"),
                        ("SHIFT", "Shift"),
                        ("CTRL", "Ctrl"),
                        ("CODE", "Alt"),
                    ] {
                        ui.monospace(k);
                        ui.label(v);
                        ui.end_row();
                    }
                });
                cols[1].label(RichText::new("Emulator keys").strong());
                egui::Grid::new("keys-emu").striped(true).num_columns(2).show(&mut cols[1], |ui| {
                    for (k, v) in [
                        ("F8", "pause / go on"),
                        ("F9", "clear the cycle counter marks"),
                        ("F10", "demo tools overlay"),
                        ("F11", "screenshot (compass-NNN.png)"),
                        ("F12", "turbo on / off"),
                        ("double-click", "full screen on / off"),
                    ] {
                        ui.monospace(k);
                        ui.label(v);
                        ui.end_row();
                    }
                });
            });
            ui.add_space(4.0);
            ui.label(RichText::new("The Compass has no [ ] \\ ` keys and no keypad, so those PC keys do nothing. The Compass keyboard reads at most two keys at a time, besides SHIFT, CTRL and CODE.").weak());
            ui.label(RichText::new("Keys go to the Compass while no text field has the focus; click the screen to give them back.").weak());
        });
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("About Compass-Emu").open(&mut self.show_about).resizable(false).show(ctx, |ui| {
            ui.heading(format!("Compass-Emu {VERSION}"));
            ui.label("A cycle-accurate emulator of the GRiD Compass.");
            ui.label("By Conventional Memories (C.M._), based on MartyPC by Daniel Balsom. MIT license.");
            ui.label("Written with Claude (Anthropic); timing checked against real Compass machines.");
            ui.hyperlink("https://github.com/ConventionalMemories/Compass-Emu");
        });
    }
}

/// PC key (by position) -> (column, bit) in the Compass matrix; the same keys as the
/// classic window.
#[cfg_attr(windows, allow(dead_code))]
fn matrix_pos(k: Key) -> Option<(usize, u8)> {
    let c = match k {
        Key::A => 'a', Key::B => 'b', Key::C => 'c', Key::D => 'd', Key::E => 'e', Key::F => 'f',
        Key::G => 'g', Key::H => 'h', Key::I => 'i', Key::J => 'j', Key::K => 'k', Key::L => 'l',
        Key::M => 'm', Key::N => 'n', Key::O => 'o', Key::P => 'p', Key::Q => 'q', Key::R => 'r',
        Key::S => 's', Key::T => 't', Key::U => 'u', Key::V => 'v', Key::W => 'w', Key::X => 'x',
        Key::Y => 'y', Key::Z => 'z',
        Key::Num0 => '0', Key::Num1 => '1', Key::Num2 => '2', Key::Num3 => '3', Key::Num4 => '4',
        Key::Num5 => '5', Key::Num6 => '6', Key::Num7 => '7', Key::Num8 => '8', Key::Num9 => '9',
        Key::Space => ' ', Key::Enter => '\r', Key::Tab => '\t',
        Key::Escape => '\x1b', Key::Backspace => '\x08',
        Key::Minus => '-', Key::Equals => '=', Key::Comma => ',', Key::Period => '.',
        Key::Slash => '/', Key::Semicolon => ';', Key::Quote => '\'',
        Key::ArrowLeft => '\u{e000}', Key::ArrowDown => '\u{e001}', Key::ArrowRight => '\u{e002}', Key::ArrowUp => '\u{e003}',
        _ => return None,
    };
    keyboard::key_for_char(c).map(|(col, bit, _)| (col, bit))
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        let owns = self.owns_keys && self.sess.is_some();
        let m = raw.modifiers;
        self.hovering = raw.hovered_files.len();
        for f in raw.dropped_files.drain(..) {
            if let Some(p) = f.path {
                self.dropped.push((p, m.shift));
            }
        }
        self.mods = (if m.shift { keyboard::MOD_SHIFT } else { 0 }) | (if m.ctrl { keyboard::MOD_CTRL } else { 0 }) | (if m.alt { keyboard::MOD_CODE } else { 0 });
        let mut keep = Vec::with_capacity(raw.events.len());
        for ev in raw.events.drain(..) {
            match &ev {
                egui::Event::Key { key, physical_key, pressed, repeat, modifiers, .. } => {
                    if matches!(key, Key::F8 | Key::F9 | Key::F10 | Key::F11 | Key::F12) {
                        if *pressed && !*repeat {
                            self.fkeys.push(*key);
                        }
                        continue;
                    }
                    if owns {
                        let k = physical_key.unwrap_or(*key);
                        if *pressed {
                            if self.keys_down.insert(k) {
                                let m = *modifiers;
                                let km = (if m.shift { keyboard::MOD_SHIFT } else { 0 }) | (if m.ctrl { keyboard::MOD_CTRL } else { 0 }) | (if m.alt { keyboard::MOD_CODE } else { 0 });
                                self.taps.push((k, km));
                            }
                        }
                        else {
                            self.keys_down.remove(&k);
                        }
                        continue;
                    }
                }
                egui::Event::Text(_) if owns => continue,
                egui::Event::WindowFocused(false) => self.keys_down.clear(),
                _ => {}
            }
            keep.push(ev);
        }
        raw.events = keep;
        if !owns {
            self.keys_down.clear();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.owns_keys = !ctx.egui_wants_keyboard_input();
        self.handle_drops();
        self.handle_fkeys();
        self.feed_keys();
        if let (Some(s), Some(r)) = (&mut self.sess, &mut self.run) {
            if let Some(msg) = r.run_frame(&mut s.m) {
                if self.dbg_once_ir3 && r.paused {
                    s.m.board_mut().debug.irqs.remove(&3);
                    self.dbg_once_ir3 = false;
                }
                if msg.starts_with("profile done") {
                    self.dbg.profile = Some(msg.clone());
                    self.show_debugger = true;
                }
                if msg.starts_with("CRASH CATCHER") {
                    self.show_debugger = true;
                }
                self.say(msg);
            }
            if let Some(s) = &mut self.sess {
                s.pump_io();
            }
        }
        let title = match &self.sess {
            Some(s) => format!("Compass-Emu {VERSION} - GRiD Compass {}", s.m.model.name()),
            None => format!("Compass-Emu {VERSION}"),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        // Full screen keeps only the Compass screen (and the debugger, if it is open).
        let full = self.fullscreen && self.sess.is_some();
        if !full {
            egui::Panel::top("menu").show_inside(ui, |ui| self.menu_bar(ui));
        }
        if self.sess.is_some() && self.show_toolbar && !full {
            egui::Panel::top("tools").show_inside(ui, |ui| self.toolbar(ui));
        }
        if self.show_status && !full {
            egui::Panel::bottom("status").show_inside(ui, |ui| self.status_bar(ui));
        }
        if self.sess.is_some() && self.show_debugger {
            debugger::panels(self, ui);
        }
        let frame = if full { egui::Frame::NONE.fill(Color32::BLACK) } else { egui::Frame::central_panel(ui.style()) };
        egui::CentralPanel::default().frame(frame).show_inside(ui, |ui| {
            if self.sess.is_some() {
                if self.screen(ui) {
                    self.toggle_fullscreen(&ctx);
                }
            }
            else {
                setup::page(self, ui);
            }
            if self.hovering > 0 {
                let r = ui.max_rect();
                ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(170));
                let text = if self.sess.is_some() {
                    "Drop a floppy image (.img or .IMD) to insert it in the 2102 drive\nhold Shift for the 2101 drive; a .state file loads that state"
                }
                else {
                    "Drop a disk image or a folder to put it in this profile\nfloppy: 2102 drive (Shift: 2101), large image: hard disk, folder: PC folder drive"
                };
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(18.0), Color32::from_rgb(0xFF, 0xC9, 0x4D));
            }
        });
        // Back to the setup page (power off): leave full screen.
        if self.fullscreen && self.sess.is_none() {
            self.toggle_fullscreen(&ctx);
        }
        self.store_prefs(&ctx);
        if self.hovering > 0 {
            ctx.request_repaint();
        }
        if self.show_media {
            media::window(self, &ctx);
        }
        if self.show_chips {
            debugger::chips_window(self, &ctx);
        }
        self.keys_window(&ctx);
        self.about_window(&ctx);
        if self.sess.is_some() {
            ctx.request_repaint();
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.power_off();
        self.setup.save();
    }
}
