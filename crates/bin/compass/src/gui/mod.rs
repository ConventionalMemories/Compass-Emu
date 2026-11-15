//! The Compass-Emu GUI (egui through eframe, OpenGL): menus, toolbar, the EL screen and a
//! status bar, plus the machine setup page, the media manager and the debugger workspace.

mod debugger;
mod led;
mod media;
mod osk;
mod project;
mod setup;
mod states;

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
    states: states::SavedStates,
    show_keys: bool,
    show_about: bool,
    /// The 2101 front panel window.
    show_panel: bool,
    /// The on-screen Compass keyboard.
    osk: osk::Osk,
    /// EL panel look: glow, pixel gaps and a short afterglow.
    el_look: bool,
    /// Keys by US position instead of by the printed letter.
    by_position: bool,
    /// Characters typed this frame (keys by printed letter: digits and punctuation).
    text_taps: Vec<char>,
    /// Afterglow brightness of each pixel (EL panel look).
    el_glow: Vec<f32>,
    tex_glow: Option<egui::TextureHandle>,
    clipboard: Option<arboard::Clipboard>,
    media: media::MediaPage,
    pub(crate) project: project::ProjectPage,
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
    /// Last look at the PC folder for new files.
    last_sync: Instant,
    /// Last time GRiD used the PC folder drive.
    folder_used: Instant,
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
            states: Default::default(),
            show_keys: false,
            show_about: false,
            show_panel: false,
            osk: osk::Osk::default(),
            el_look: prefs.el_look,
            by_position: prefs.keys_by_position,
            text_taps: vec![],
            el_glow: vec![],
            tex_glow: None,
            clipboard: None,
            media: media::MediaPage::default(),
            project: project::ProjectPage::default(),
            dbg: debugger::DebugPage::default(),
            flash: None,
            snapshot: None,
            owns_keys: false,
            keys_down: HashSet::new(),
            taps: vec![],
            mods: 0,
            fkeys: vec![],
            drive_seen: vec![],
            last_sync: Instant::now(),
            folder_used: Instant::now(),
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
        for d in cfg.devices().iter().filter(|d| d.enabled) {
            setup::push_recent(&mut p.recent_hds, &d.hd);
            setup::push_recent(&mut p.recent_floppies, &d.disk);
        }
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

    /// The system clipboard (kept open: on Linux a copy lasts only while it is).
    fn clipboard(&mut self) -> Result<&mut arboard::Clipboard, arboard::Error> {
        if self.clipboard.is_none() {
            self.clipboard = Some(arboard::Clipboard::new()?);
        }
        Ok(self.clipboard.as_mut().unwrap())
    }

    /// Edit > Paste text: type the clipboard's text on the Compass keyboard.
    fn paste(&mut self) {
        let text = self.clipboard().and_then(|c| c.get_text());
        let Some(r) = &mut self.run else { return };
        match text {
            Ok(t) => {
                let skipped = r.paste_text(&t);
                let n = r.pasting();
                let chars = if n == 1 { "character" } else { "characters" };
                if skipped > 0 {
                    self.say(format!("typing {n} {chars} ({skipped} have no Compass key and are left out)"));
                }
                else {
                    self.say(format!("typing {n} {chars}"));
                }
            }
            Err(e) => self.say(format!("no text on the clipboard ({e})")),
        }
    }

    /// Edit > Copy screen: the Compass screen to the clipboard as a picture.
    fn copy_screen(&mut self) {
        let Some(s) = &self.sess else { return };
        let (w, h) = s.m.screen_size();
        let buf = display::colorize(&s.m.render_display(), self.colour, display::DARK);
        let bytes: Vec<u8> = buf.iter().flat_map(|&c| [(c >> 16) as u8, (c >> 8) as u8, c as u8, 0xFF]).collect();
        let img = arboard::ImageData { width: w, height: h, bytes: bytes.into() };
        match self.clipboard().and_then(|c| c.set_image(img)) {
            Ok(_) => self.say(format!("screen copied ({w} x {h})")),
            Err(e) => self.say(format!("could not copy the screen: {e}")),
        }
    }

    fn set_rate(&mut self, rate: f64) {
        if let (Some(s), Some(r)) = (&self.sess, &mut self.run) {
            r.set_rate(&s.m, rate);
        }
    }

    /// The speed slider (percent of a real Compass).
    fn speed_slider(&mut self, ui: &mut egui::Ui) {
        let Some(r) = &self.run else { return };
        let mut pct = (r.rate * 100.0).round();
        let resp = ui
            .add(egui::Slider::new(&mut pct, 10.0..=400.0).logarithmic(true).suffix("%").integer().text("speed"))
            .on_hover_text("real-time speed, 100% = a real Compass (double-click: back to 100%)");
        if resp.double_clicked() {
            pct = 100.0;
        }
        if (pct / 100.0 - r.rate).abs() > 1e-6 {
            self.set_rate(pct / 100.0);
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
        let Some(s) = &self.sess else { return };
        match s.floppy_drive(kind) {
            Some(i) => self.insert_at(i, path),
            None => self.say(format!("this machine has no {} drive (add it on the setup page)", if kind == DriveKind::Floppy2102 { "2102 floppy" } else { "2101 floppy" })),
        }
    }

    /// Put a floppy image into GPIB drive `i` (None = take the disk out).
    pub(crate) fn insert_at(&mut self, i: usize, path: Option<PathBuf>) {
        if let (Some(p), true) = (&path, self.sess.is_some()) {
            let mut pr = self.setup.prefs().clone();
            setup::push_recent(&mut pr.recent_floppies, &p.display().to_string());
            self.setup.set_prefs(pr);
        }
        let Some(s) = &mut self.sess else { return };
        match s.insert_floppy_at(i, path.as_deref()) {
            Ok(log) => {
                for l in log {
                    self.say(l);
                }
            }
            Err(e) => self.say(e),
        }
        self.resync();
    }

    pub(crate) fn pick_floppy(&mut self, i: usize) {
        let p = rfd::FileDialog::new().add_filter("GRiD floppy image", &["img", "IMG", "imd", "IMD"]).pick_file();
        if p.is_some() {
            self.insert_at(i, p);
        }
        else {
            self.resync();
        }
    }

    /// The machine's floppy drives (not the PC folder): (drive index, "5 Floppy Disk").
    pub(crate) fn floppy_drives(&self) -> Vec<(usize, String)> {
        let Some(s) = &self.sess else { return vec![] };
        let d = &s.m.board().gpib.drives;
        (0..d.len())
            .filter(|&i| d[i].kind != DriveKind::Hdd2101 && !matches!(s.media.get(i), Some(Media::Folder(_))))
            .map(|i| {
                let a = d[i].address();
                (i, format!("{a} {}", crate::session::grid_name(a)))
            })
            .collect()
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
                    let d = self.setup.current_mut().device_mut("folder");
                    d.folder = text.clone();
                    let name = crate::session::grid_name(d.addr);
                    self.say(format!("PC folder {text} set as this profile's PC folder drive ({name})"));
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
                    self.setup.current_mut().device_mut("2101").hd = text.clone();
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
                    self.setup.current_mut().device_mut(if shift { "2101" } else { "2102" }).disk = text.clone();
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
        p.el_look = self.el_look;
        p.keys_by_position = self.by_position;
        self.setup.set_prefs(p);
    }

    /// A Recent submenu that inserts the picked floppy image into a drive.
    fn recent_floppy_menu(&mut self, ui: &mut egui::Ui, i: usize, label: &str) {
        let list = self.setup.prefs().recent_floppies.clone();
        ui.add_enabled_ui(!list.is_empty(), |ui| {
            ui.menu_button(label, |ui| {
                for p in list {
                    let short = std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(p.clone());
                    if ui.button(short).on_hover_text(&p).clicked() {
                        self.insert_at(i, Some(PathBuf::from(&p)));
                    }
                }
            });
        });
    }

    /// About once a second, while GRiD has left the PC folder drive alone for two seconds,
    /// bring in files added or changed in the folder.
    fn folder_sync(&mut self) {
        let Some(s) = &mut self.sess else { return };
        let Some(i) = s.media.iter().position(|m| matches!(m, Media::Folder(_))) else { return };
        if s.m.board().gpib.drives.get(i).is_some_and(|d| d.busy()) {
            self.folder_used = Instant::now();
        }
        if self.last_sync.elapsed() < Duration::from_secs(1) || self.folder_used.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_sync = Instant::now();
        for l in s.sync_folder() {
            self.say(l);
        }
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
                crate::winkeys::poll(!self.by_position, &mut down, &mut t, &mut mods);
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
        // keys by printed letter: a typed character goes in as the Compass key that makes it,
        // with that key's own Shift (whatever Shift/AltGr the PC needed for it)
        for c in self.text_taps.drain(..) {
            if let Some((col, bit, shift)) = keyboard::key_for_char(c) {
                tapped.push(((col, bit), crate::runner::EXACT | if shift { keyboard::MOD_SHIFT } else { 0 }));
            }
        }
        // the on-screen keyboard
        let om = self.osk.mods();
        for (p, pm) in self.osk.taps.drain(..) {
            tapped.push((p, pm | mods));
        }
        down.extend(self.osk.down.iter().copied());
        mods |= om;
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
                    self.speed_slider(ui);
                    if let Some(s) = &mut self.sess {
                        let mut fast = s.m.is_fast();
                        if ui.checkbox(&mut fast, "Fast CPU core (approximate timing)").changed() {
                            s.m.set_fast(fast);
                            s.cfg.fast = fast;
                        }
                        let mut micro = s.m.fpu_microcode();
                        if ui.checkbox(&mut micro, "8087 on its microcode ROM").on_hover_text("off: the instruction-level 8087; the switch happens between two instructions").changed() {
                            let r = s.set_fpu_micro(micro, &self.default_roms);
                            self.say(r.unwrap_or_else(|e| e));
                        }
                    }
                    ui.separator();
                    if ui.button("Quick save").clicked() {
                        self.quick_save();
                    }
                    if ui.add_enabled(self.snapshot.is_some(), egui::Button::new("Quick load")).clicked() {
                        self.quick_load();
                    }
                    ui.separator();
                    if ui.button("Save state").on_hover_text("the machine and a picture of its screen, kept with this profile").clicked() {
                        self.save_state_slot();
                    }
                    if ui.button("Saved states...").clicked() {
                        self.states.show = true;
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
            ui.menu_button("Edit", |ui| {
                if ui.add_enabled(on, egui::Button::new("Paste text")).on_hover_text("type the clipboard's text on the Compass keyboard").clicked() {
                    self.paste();
                }
                let left = self.run.as_ref().map(|r| r.pasting()).unwrap_or(0);
                let label = if left > 0 { format!("Stop pasting ({left} left)") } else { "Stop pasting".to_string() };
                if ui.add_enabled(left > 0, egui::Button::new(label)).clicked() {
                    if let Some(r) = &mut self.run {
                        r.cancel_paste();
                    }
                }
                if ui.add_enabled(on, egui::Button::new("Copy screen")).on_hover_text("the Compass screen to the clipboard as a picture").clicked() {
                    self.copy_screen();
                }
            });
            ui.menu_button("Media", |ui| {
                if ui.button("Media manager...").clicked() {
                    self.show_media = true;
                }
                if on {
                    ui.separator();
                    let drives = self.floppy_drives();
                    if drives.is_empty() {
                        ui.label(RichText::new("no floppy drives (add one on the setup page)").weak());
                    }
                    for (i, name) in drives {
                        let what = match self.sess.as_ref().and_then(|s| s.media.get(i)) {
                            Some(Media::Image(p)) => p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                            _ => "empty".into(),
                        };
                        ui.menu_button(format!("{name}: {what}"), |ui| {
                            if ui.button("Insert...").clicked() {
                                self.pick_floppy(i);
                            }
                            self.recent_floppy_menu(ui, i, "Recent");
                            if ui.button("Take out").clicked() {
                                self.insert_at(i, None);
                            }
                            if let Some(s) = &mut self.sess {
                                let mut wp = s.protected(i);
                                if ui.checkbox(&mut wp, "Write protect").changed() {
                                    s.set_protected(i, wp);
                                }
                            }
                        });
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
            ui.menu_button("Project", |ui| {
                if ui.button("Project window...").clicked() {
                    self.project.show = true;
                }
                if ui.button("Open project folder...").clicked() {
                    project::pick_project(self);
                }
                ui.separator();
                let busy = self.project.building();
                if ui.add_enabled(!busy, egui::Button::new("Build")).clicked() {
                    project::start(self, false, false);
                }
                if ui.add_enabled(!busy, egui::Button::new("Build and run")).clicked() {
                    project::start(self, true, false);
                }
                if ui.add_enabled(!busy, egui::Button::new("Debug build")).clicked() {
                    project::start(self, false, true);
                }
            });
            ui.menu_button("View", |ui| {
                ui.radio_value(&mut self.fit, Fit::Fit, "Fill the window");
                ui.radio_value(&mut self.fit, Fit::Integer, "Whole-pixel scaling");
                ui.separator();
                for (name, c) in COLOURS {
                    ui.radio_value(&mut self.colour, *c, *name);
                }
                ui.checkbox(&mut self.el_look, "EL panel look").on_hover_text("glow, gaps between the pixels and a short afterglow");
                ui.separator();
                ui.checkbox(&mut self.overlay, "Demo tools overlay  (F10)");
                ui.checkbox(&mut self.osk.show, "Compass keyboard (on screen)");
                ui.menu_button("PC keys", |ui| {
                    ui.radio_value(&mut self.by_position, false, "By the printed letter (any layout)")
                        .on_hover_text("letters by the letter on the key, digits and punctuation by the character they type (AZERTY, QWERTZ...)");
                    ui.radio_value(&mut self.by_position, true, "By position (US layout)").on_hover_text("every key by where it sits on a US keyboard, whatever is printed on it");
                });
                ui.checkbox(&mut self.show_panel, "2101 front panel");
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
            if ui.button("Paste").on_hover_text("Edit > Paste text").clicked() {
                self.paste();
            }
            if ui.selectable_label(self.osk.show, "Keyboard").on_hover_text("the on-screen Compass keyboard").clicked() {
                self.osk.show = !self.osk.show;
            }
            ui.separator();
            if !turbo {
                self.speed_slider(ui);
                ui.separator();
            }
            if ui.selectable_label(self.show_media, "Media").clicked() {
                self.show_media = !self.show_media;
            }
            if ui.selectable_label(self.show_debugger, "Debugger").clicked() {
                self.show_debugger = !self.show_debugger;
            }
            if ui.selectable_label(self.overlay, "Overlay (F10)").clicked() {
                self.overlay = !self.overlay;
            }
            ui.separator();
            let busy = self.project.building();
            if ui.add_enabled(!busy, egui::Button::new(if busy { "Building..." } else { "Build and run" })).on_hover_text("Project > Build and run").clicked() {
                project::start(self, true, false);
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
                if (r.rate - 1.0).abs() > 1e-6 && !r.turbo {
                    ui.label(RichText::new(format!("(set to {:.0}%)", r.rate * 100.0)).weak());
                }
            }
            if r.pasting() > 0 {
                ui.label(RichText::new(format!("Pasting ({} left)", r.pasting())).color(Color32::from_rgb(0xFF, 0xC9, 0x4D)));
            }
            if r.turbo {
                ui.label(RichText::new("Turbo").color(Color32::from_rgb(0xFF, 0xC9, 0x4D)));
            }
            if s.m.is_fast() {
                ui.label("Fast core");
            }
            if s.m.fpu_microcode() {
                ui.label("8087 microcode");
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
                let a = d.address();
                let name = match (d.kind, &s.media[i]) {
                    (_, Media::Folder(_)) => format!("PC folder ({a})"),
                    (DriveKind::Hdd2101, _) => format!("{a} {}", crate::session::grid_name(a)),
                    (_, Media::Empty) => format!("{a} {} (empty)", crate::session::grid_name(a)),
                    _ => format!("{a} {}", crate::session::grid_name(a)),
                };
                if d.image.is_empty() && d.kind == DriveKind::Hdd2101 {
                    continue;
                }
                let name = if s.protected(i) { format!("{name} WP") } else { name };
                let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(
                    rect.center(),
                    4.5,
                    if lit { Color32::from_rgb(0xFF, 0xB0, 0x00) } else { Color32::from_rgb(0x4A, 0x4A, 0x52) },
                );
                let low = d.lle.is_some() || d.lle2101.is_some() || (d.kind == DriveKind::Floppy2101 && d.served);
                let level = if low { "low level: the drive's own firmware runs" } else { "protocol model (no firmware)" };
                ui.label(if d.dirty { format!("{name} *") } else { name.clone() })
                    .on_hover_text(format!("{level}{}", if d.dirty { "\n* = changes not written to the image yet" } else { "" }));
                ui.label(RichText::new(if low { "LL" } else { "P" }).small().weak()).on_hover_text(level);
                if let Some(t) = d.display() {
                    // The low-level 2101's four-character display.
                    if led::display(ui, &t, 2.0).on_hover_text("2101 display (View > 2101 front panel for a bigger one)").clicked() {
                        self.show_panel = true;
                    }
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
        let mut buf = if self.el_look {
            // EL panel look: a lit pixel fades out over a few frames instead of at once.
            if self.el_glow.len() != pix.len() {
                self.el_glow = vec![0.0; pix.len()];
            }
            for (g, &p) in self.el_glow.iter_mut().zip(pix.iter()) {
                *g = if p != 0 { 1.0 } else { *g * 0.4 };
            }
            self.el_glow.iter().map(|&g| mix(display::DARK, self.colour, g)).collect()
        }
        else {
            display::colorize(&pix, self.colour, display::DARK)
        };
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
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
        if self.el_look {
            self.el_overlay(ui, rect, w, h, k);
        }
        resp.double_clicked()
    }

    /// EL panel look on top of the screen: a soft glow around lit pixels and dark gaps
    /// between the pixels (when they are big enough to show).
    fn el_overlay(&mut self, ui: &mut egui::Ui, rect: egui::Rect, w: usize, h: usize, k: f32) {
        let p = ui.painter_at(rect);
        // glow: the afterglow image blurred (3 x 3 box, twice) and laid over softly
        let mut g = self.el_glow.clone();
        for _ in 0..2 {
            g = blur(&g, w, h);
        }
        let (dr, dg, db) = rgb(display::DARK);
        let (lr, lg, lb) = rgb(self.colour);
        let img = egui::ColorImage::new(
            [w, h],
            g.iter()
                .map(|&v| {
                    let a = (v.min(1.0) * 110.0) as u8;
                    let f = |d: u8, l: u8| (d as f32 + (l as f32 - d as f32) * 0.9) as u8;
                    Color32::from_rgba_unmultiplied(f(dr, lr), f(dg, lg), f(db, lb), a)
                })
                .collect(),
        );
        let tex = match &mut self.tex_glow {
            Some(t) if t.size() == [w, h] => {
                t.set(img, egui::TextureOptions::LINEAR);
                t.clone()
            }
            _ => {
                let t = ui.ctx().load_texture("compass-glow", img, egui::TextureOptions::LINEAR);
                self.tex_glow = Some(t.clone());
                t
            }
        };
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        p.image(tex.id(), rect, uv, Color32::WHITE);
        // gaps between the pixels, on whole screen pixels so they stay sharp
        if k >= 2.5 {
            let ppp = ui.ctx().pixels_per_point();
            let t = ((k * ppp * 0.2).round().max(1.0)) / ppp;
            let c = Color32::from_rgba_unmultiplied(0x0A, 0x05, 0x00, 200);
            let snap = |v: f32| (v * ppp).round() / ppp;
            for x in 1..w {
                let x = snap(rect.left() + x as f32 * k);
                p.rect_filled(egui::Rect::from_min_max(egui::pos2(x - t, rect.top()), egui::pos2(x, rect.bottom())), 0.0, c);
            }
            for y in 1..h {
                let y = snap(rect.top() + y as f32 * k);
                p.rect_filled(egui::Rect::from_min_max(egui::pos2(rect.left(), y - t), egui::pos2(rect.right(), y)), 0.0, c);
            }
        }
    }

    fn keys_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("Keyboard map").open(&mut self.show_keys).resizable(false).show(ctx, |ui| {
            let by_position = self.by_position;
            ui.label(if by_position {
                "PC keys work by position (as on a US keyboard), not by the letter your layout prints."
            }
            else {
                "PC keys work by the letter printed on them: letters by their letter, digits and punctuation by the character they type (any layout). With Ctrl or Alt held, digits and punctuation go by position."
            });
            ui.label(RichText::new("View > PC keys switches between the two.").weak());
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
            #[cfg(windows)]
            ui.label(RichText::new(crate::winkeys::status()).weak());
        });
    }

    /// The 2101 box's front: its LED display and the drive lights.
    fn panel_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_panel;
        egui::Window::new("2101 front panel").open(&mut open).resizable(false).show(ctx, |ui| {
            let Some(s) = &self.sess else {
                ui.label("Powered off");
                return;
            };
            let drives = &s.m.board().gpib.drives;
            let boxes: Vec<_> = drives.iter().filter(|d| d.kind == DriveKind::Hdd2101).collect();
            if boxes.is_empty() {
                ui.label("This machine has no 2101 Disc System.");
                return;
            }
            for (n, hd) in boxes.iter().enumerate() {
                if n > 0 {
                    ui.add_space(8.0);
                }
                let a = hd.address();
                ui.label(RichText::new(format!("GPIB {a} {} + {} {}", crate::session::grid_name(a), a + 1, crate::session::grid_name(a + 1))).weak());
                match hd.display() {
                    Some(t) => {
                        egui::Frame::new().fill(Color32::from_rgb(0x16, 0x15, 0x14)).inner_margin(14.0).corner_radius(6.0).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                led::display(ui, &t, 7.0);
                                ui.add_space(24.0);
                                ui.label(RichText::new("DISC SYSTEM 2101").color(Color32::from_rgb(0xD8, 0xD0, 0xB8)).strong());
                            });
                        });
                    }
                    None => {
                        ui.label("No display: this box runs as the protocol model (low level needs its firmware ROM, see Machine setup).");
                    }
                }
                ui.horizontal(|ui| {
                    let flop = drives.iter().find(|d| d.kind == DriveKind::Floppy2101 && d.address() == a + 1);
                    for (d, label) in [(Some(*hd), "hard disk"), (flop, "floppy")] {
                        let Some(d) = d else { continue };
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 5.0, if d.busy() { Color32::from_rgb(0xFF, 0xB0, 0x00) } else { Color32::from_rgb(0x4A, 0x4A, 0x52) });
                        ui.label(label);
                    }
                });
            }
        });
        self.show_panel = open;
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
fn rgb(c: u32) -> (u8, u8, u8) {
    ((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// Colour `t` of the way from `a` to `b` (0xRRGGBB).
fn mix(a: u32, b: u32, t: f32) -> u32 {
    let (ar, ag, ab) = rgb(a);
    let (br, bg, bb) = rgb(b);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u32;
    f(ar, br) << 16 | f(ag, bg) << 8 | f(ab, bb)
}

/// 3 x 3 box blur.
fn blur(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut row = vec![0.0f32; src.len()];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let l = if x > 0 { src[i - 1] } else { 0.0 };
            let r = if x + 1 < w { src[i + 1] } else { 0.0 };
            row[i] = (l + src[i] + r) / 3.0;
        }
    }
    let mut out = vec![0.0f32; src.len()];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let u = if y > 0 { row[i - w] } else { 0.0 };
            let d = if y + 1 < h { row[i + w] } else { 0.0 };
            out[i] = (u + row[i] + d) / 3.0;
        }
    }
    out
}

/// Keys that go in by the key itself when keys go by printed letter: letters (by their
/// layout letter), space, Return, Tab, Esc, Backspace and the arrows.
fn printed_key(k: Key) -> bool {
    use Key::*;
    matches!(
        k,
        A | B | C | D | E | F | G | H | I | J | K | L | M | N | O | P | Q | R | S | T | U | V | W | X | Y | Z | Space | Enter | Tab | Escape | Backspace | ArrowLeft | ArrowRight | ArrowUp | ArrowDown
    )
}

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
                        let k = if self.by_position { physical_key.unwrap_or(*key) } else { *key };
                        // by printed letter: digits and punctuation come as typed text,
                        // unless Ctrl or Code (Alt) alone is held (no text then)
                        let text_key = !self.by_position && !printed_key(k) && !(modifiers.ctrl ^ modifiers.alt);
                        if text_key {
                            continue;
                        }
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
                egui::Event::Text(t) if owns => {
                    if !self.by_position {
                        self.text_taps.extend(t.chars().filter(|c| !c.is_alphabetic() && *c != ' '));
                    }
                    continue;
                }
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
        project::poll(self);
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
            self.folder_sync();
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
        if self.states.show && self.sess.is_some() {
            self.states_window(&ctx);
        }
        self.keys_window(&ctx);
        self.about_window(&ctx);
        if self.show_panel {
            self.panel_window(&ctx);
        }
        if self.project.show {
            project::window(self, &ctx);
        }
        if self.osk.show && self.sess.is_some() {
            self.osk.window(&ctx);
        }
        if self.project.building() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if self.sess.is_some() {
            ctx.request_repaint();
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.power_off();
        self.setup.save();
    }
}
