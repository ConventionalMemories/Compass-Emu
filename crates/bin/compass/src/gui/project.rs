//! Project menu: build a GRiD program from a project folder on a hidden Compass (see
//! build.rs) and run it on the machine on screen through its PC folder drive.

use std::{path::PathBuf, sync::mpsc::Receiver};

use eframe::egui::{self, RichText};

use super::{setup, App};
use crate::{
    build::{self, BuildMsg},
    session::Media,
};

#[derive(Default)]
pub struct ProjectPage {
    pub show: bool,
    rx: Option<Receiver<BuildMsg>>,
    /// Run the program when the build is done.
    run_after: bool,
    pub log: Vec<String>,
    /// The last build's result: Some(true) built.
    pub last_ok: Option<bool>,
}

impl ProjectPage {
    pub fn building(&self) -> bool {
        self.rx.is_some()
    }
}

/// The development disk for builds: the one set in the Project window, else the hard disk
/// of the selected profile.
fn build_disk(app: &App) -> String {
    let p = &app.setup.prefs().build_disk;
    if !p.is_empty() {
        return p.clone();
    }
    let cfg = match &app.sess {
        Some(s) => &s.cfg,
        None => app.setup.current(),
    };
    cfg.devices().into_iter().find(|d| d.enabled && d.kind == "2101" && !d.hd.trim().is_empty()).map(|d| d.hd).unwrap_or_default()
}

pub fn pick_project(app: &mut App) {
    if let Some(p) = rfd::FileDialog::new().set_title("Project folder (NAME holding NAME.pas)").pick_folder() {
        set_project(app, p.display().to_string());
    }
    app.resync();
}

fn set_project(app: &mut App, p: String) {
    let mut pr = app.setup.prefs().clone();
    pr.project = p.clone();
    setup::push_recent(&mut pr.recent_projects, &p);
    app.setup.set_prefs(pr);
    app.project.show = true;
}

/// Start a build of the current project (and run it after when `run`).
pub fn start(app: &mut App, run: bool, debug: bool) {
    if app.project.building() {
        return;
    }
    let project = app.setup.prefs().project.clone();
    if project.is_empty() {
        pick_project(app);
        if app.setup.prefs().project.is_empty() {
            return;
        }
        return start(app, run, debug);
    }
    let disk = build_disk(app);
    app.project.show = true;
    app.project.log.clear();
    app.project.last_ok = None;
    if disk.is_empty() {
        app.project.log.push("Pick the development disk in this window first (a disk with the GRiD Pascal compiler, like the merged dev disk).".into());
        return;
    }
    let cfg = app.sess.as_ref().map(|s| s.cfg.clone()).unwrap_or_else(|| app.setup.current().clone());
    let job = build::BuildJob {
        project: PathBuf::from(&project),
        disk: PathBuf::from(disk),
        roms: cfg.rom_dir(&app.default_roms),
        debug,
        exact: false,
    };
    app.project.log.push(format!("Building {}{}", project, if debug { " (debug)" } else { "" }));
    app.project.run_after = run;
    app.project.rx = Some(build::spawn(job));
}

/// Put NAME.run on the running machine's PC folder drive (as NAME~Run~): the drive picks it
/// up within a few seconds.
pub fn run_on_machine(app: &mut App, run: &std::path::Path, name: &str) {
    let Some(s) = &app.sess else {
        app.project.log.push("Power the machine on to run the program.".into());
        return;
    };
    let folder = s.media.iter().find_map(|m| if let Media::Folder(p) = m { Some(p.clone()) } else { None });
    let Some(folder) = folder else {
        app.project.log.push("This machine has no PC folder drive: set one in Machine setup (Drives) to run builds on it.".into());
        return;
    };
    let target = folder.join(format!("{name}~Run~"));
    match std::fs::copy(run, &target) {
        Ok(_) => {
            let vol = folder.canonicalize().ok().and_then(|c| c.file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_default();
            app.project.log.push(format!(
                "{name} copied to the PC folder drive. In a few seconds it shows in the Executive on the volume '{vol}' (Floppy Disk): open it there."
            ));
        }
        Err(e) => app.project.log.push(format!("cannot copy to {}: {e}", target.display())),
    }
}

/// Take the build thread's messages (every frame).
pub fn poll(app: &mut App) {
    let Some(rx) = &app.project.rx else { return };
    let mut done = None;
    while let Ok(m) = rx.try_recv() {
        match m {
            BuildMsg::Line(l) => app.project.log.push(l),
            BuildMsg::Done(r) => done = Some(r),
        }
    }
    if let Some(r) = done {
        app.project.rx = None;
        app.project.last_ok = Some(r.ok);
        app.say(if r.ok {
            format!("build of {} OK", r.name)
        }
        else if r.errors.is_empty() {
            format!("build of {} failed", r.name)
        }
        else {
            format!("build of {} failed: {}", r.name, r.errors[0])
        });
        if let (true, Some(run)) = (app.project.run_after, &r.run) {
            run_on_machine(app, &run.clone(), &r.name);
        }
    }
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    let mut open = app.project.show;
    egui::Window::new("Project").open(&mut open).default_width(560.0).resizable(true).show(ctx, |ui| {
        let prefs = app.setup.prefs().clone();
        ui.horizontal(|ui| {
            ui.label("Project folder");
            ui.label(RichText::new(if prefs.project.is_empty() { "(none)" } else { &prefs.project }).monospace());
        });
        ui.horizontal(|ui| {
            if ui.button("Open...").clicked() {
                pick_project(app);
            }
            if !prefs.recent_projects.is_empty() {
                ui.menu_button("Recent", |ui| {
                    for p in &prefs.recent_projects {
                        if ui.button(p).clicked() {
                            set_project(app, p.clone());
                        }
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            ui.label("Development disk");
            let shown = build_disk(app);
            ui.label(RichText::new(if shown.is_empty() { "(none)".to_string() } else { shown }).monospace());
        });
        ui.horizontal(|ui| {
            if ui.button("Pick...").on_hover_text("a hard disk image with the GRiD Pascal compiler, assembler and linker (the merged dev disk); only read").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("disk image", &["img", "IMG"]).pick_file() {
                    let mut pr = app.setup.prefs().clone();
                    pr.build_disk = p.display().to_string();
                    app.setup.set_prefs(pr);
                }
                app.resync();
            }
            if !prefs.build_disk.is_empty() && ui.button("Use the machine's hard disk").clicked() {
                let mut pr = app.setup.prefs().clone();
                pr.build_disk.clear();
                app.setup.set_prefs(pr);
            }
        });
        ui.separator();
        let busy = app.project.building();
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Build")).clicked() {
                start(app, false, false);
            }
            if ui.add_enabled(!busy, egui::Button::new("Build and run")).on_hover_text("then puts NAME~Run~ on this machine's PC folder drive").clicked() {
                start(app, true, false);
            }
            if ui.add_enabled(!busy, egui::Button::new("Debug build")).on_hover_text("with symbols for the debugger (.obj files and the link map)").clicked() {
                start(app, false, true);
            }
            if busy {
                ui.spinner();
                ui.label("building on a hidden Compass...");
            }
            else if let Some(ok) = app.project.last_ok {
                ui.label(if ok { RichText::new("BUILD OK").color(egui::Color32::from_rgb(0x8F, 0xD1, 0x8F)) } else { RichText::new("BUILD FAILED").color(egui::Color32::from_rgb(0xFF, 0x8A, 0x70)) });
            }
        });
        if !busy && app.project.last_ok == Some(true) && !prefs.project.is_empty() {
            let proj = PathBuf::from(&prefs.project);
            if let Ok((name, _, _)) = build::project_parts(&proj) {
                let run = proj.join(format!("{name}.run"));
                if run.exists() && ui.button(format!("Run {name} on this machine")).clicked() {
                    run_on_machine(app, &run, &name);
                }
            }
        }
        ui.separator();
        egui::ScrollArea::vertical().max_height(260.0).stick_to_bottom(true).show(ui, |ui| {
            for l in &app.project.log {
                let c = if l.contains("FAILED") || l.contains("***") || l.contains("ERROR") {
                    RichText::new(l).monospace().color(egui::Color32::from_rgb(0xFF, 0x8A, 0x70))
                }
                else {
                    RichText::new(l).monospace()
                };
                ui.label(c);
            }
        });
        ui.label(RichText::new("Builds run on a hidden 1129 (or 1139 for a 113x disk) with the fast core; your machine keeps running. Results go to the project folder: NAME.run, NAME.lst, screen.png.").weak().small());
    });
    app.project.show = open;
}
