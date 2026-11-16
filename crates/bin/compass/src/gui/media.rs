//! Media manager: the drives of the running machine (swap or take out floppies, write
//! changes back) and a GRiD file browser for disk images (export, import, delete, subjects,
//! blank floppies).

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use grid_core::{
    gpib::DriveKind,
    hostdir::{self, Volume, PAGE},
};

use super::App;
use crate::session::Media;

/// Where the browser's disk comes from.
#[derive(Clone, PartialEq)]
enum Source {
    /// Drive i of the running machine (a copy, read only).
    Drive(usize),
    /// An image file not in use (can be changed).
    File(PathBuf),
}

#[derive(Clone)]
struct Entry {
    subject: String,
    name: String,
    fid: u32,
    size: u32,
    modified: String,
}

#[derive(Default)]
pub struct MediaPage {
    source: Option<Source>,
    img: Vec<u8>,
    volumes: Vec<(u32, u32)>,
    vol_index: usize,
    vol_name: String,
    free_kb: u32,
    entries: Vec<Entry>,
    selected: Option<usize>,
    new_subject: String,
    blank_name: String,
    message: Option<String>,
}

fn fmt_time(t: Option<std::time::SystemTime>) -> String {
    t.map(|t| {
        let d: chrono::DateTime<chrono::Local> = t.into();
        d.format("%d-%b-%y %H:%M").to_string()
    })
    .unwrap_or_default()
}

impl MediaPage {
    fn volume(&self) -> Result<Volume, String> {
        let (st, n) = *self.volumes.get(self.vol_index).ok_or("no GRiD volume on this disk")?;
        Volume::open_any(self.img[st as usize * PAGE..(st + n) as usize * PAGE].to_vec()).map_err(|e| e.0)
    }

    fn open(&mut self, source: Source, img: Vec<u8>) {
        self.source = Some(source);
        self.img = img;
        self.volumes = hostdir::volumes(&self.img);
        self.vol_index = 0;
        self.refresh();
    }

    fn refresh(&mut self) {
        self.entries.clear();
        self.selected = None;
        match self.volume() {
            Ok(v) => {
                self.vol_name = v.name();
                self.free_kb = v.free_pages() / 2;
                match v.files() {
                    Ok(files) => {
                        for (subject, name, fid) in files {
                            let (size, _, m) = v.info(fid).unwrap_or((0, None, None));
                            self.entries.push(Entry { subject, name, fid, size, modified: fmt_time(m) });
                        }
                        self.message = None;
                    }
                    Err(e) => self.message = Some(format!("cannot read the directory: {e}")),
                }
            }
            Err(e) => {
                self.vol_name.clear();
                self.message = Some(e);
            }
        }
    }

    /// Write a changed volume into the image and the image to its file.
    fn store(&mut self, v: &Volume) -> Result<(), String> {
        let Some(Source::File(p)) = &self.source else { return Err("this disk is in use by the machine".into()) };
        let (st, _) = self.volumes[self.vol_index];
        let a = st as usize * PAGE;
        self.img[a..a + v.img.len()].copy_from_slice(&v.img);
        std::fs::write(p, &self.img).map_err(|e| format!("{}: {e}", p.display()))
    }

    fn edit(&mut self, f: impl FnOnce(&mut Volume) -> Result<String, String>) {
        let r = self.volume().and_then(|mut v| {
            let msg = f(&mut v)?;
            self.store(&v)?;
            Ok(msg)
        });
        self.refresh();
        self.message = Some(match r {
            Ok(m) => m,
            Err(e) => e,
        });
    }
}

fn drive_name(d: &grid_core::gpib::Drive) -> String {
    let a = d.address();
    let unit = match d.kind {
        DriveKind::Hdd2101 => "2101 hard disk",
        DriveKind::Floppy2101 => "2101 floppy",
        DriveKind::Floppy2102 => "2102 floppy",
    };
    format!("{a} {} ({unit})", crate::session::grid_name(a))
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    let mut open = app.show_media;
    egui::Window::new("Media manager").open(&mut open).default_size([900.0, 560.0]).resizable(true).show(ctx, |ui| {
        egui::Panel::left("media-drives").resizable(true).default_size(300.0).show_inside(ui, |ui| drives(app, ui));
        egui::CentralPanel::default().show_inside(ui, |ui| browser(app, ui));
    });
    app.show_media = open;
}

fn drives(app: &mut App, ui: &mut egui::Ui) {
    ui.label(RichText::new("Drives").strong());
    let mut action: Option<Box<dyn FnOnce(&mut App)>> = None;
    match &mut app.sess {
        None => {
            ui.label("The machine is off. Use the browser on the right for disk images.");
        }
        Some(s) => {
            let n = s.m.board().gpib.drives.len();
            for i in 0..n {
                let d = &s.m.board().gpib.drives[i];
                let kind = d.kind;
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let folder = matches!(s.media[i], Media::Folder(_));
                    ui.label(RichText::new(if folder { format!("{} {} (PC folder)", d.address(), crate::session::grid_name(d.address())) } else { drive_name(d) }).strong());
                    let what = match &s.media[i] {
                        Media::Image(p) => p.display().to_string(),
                        Media::Folder(p) => format!("PC folder {} (written back when you quit)", p.display()),
                        Media::Empty => "no disk".into(),
                    };
                    ui.label(what);
                    if !d.image.is_empty() {
                        let vols = hostdir::volumes(&d.image);
                        ui.label(RichText::new(format!("{} KB, {} volume(s){}", d.image.len() / 1024, vols.len(), if d.dirty { ", changed" } else { "" })).weak());
                    }
                    ui.horizontal_wrapped(|ui| {
                        if kind != DriveKind::Hdd2101 && !folder {
                            if ui.button("Insert...").clicked() {
                                action = Some(Box::new(move |app: &mut App| app.pick_floppy(i)));
                            }
                            if !d.image.is_empty() && ui.button("Take out").clicked() {
                                action = Some(Box::new(move |app: &mut App| app.insert_at(i, None)));
                            }
                        }
                        let mut wp = s.protected(i);
                        if ui.checkbox(&mut wp, "Write protect").on_hover_text("floppy: the write-protect tab (GRiD gets error 106); hard disk and PC folder: changes are not saved").changed() {
                            action = Some(Box::new(move |app: &mut App| {
                                if let Some(s) = &mut app.sess {
                                    s.set_protected(i, wp);
                                }
                            }));
                        }
                        if d.dirty && ui.button("Write back").clicked() {
                            action = Some(Box::new(move |app: &mut App| {
                                let log = app.sess.as_mut().map(|s| s.save_drive(i)).unwrap_or_default();
                                for l in log {
                                    app.say(l);
                                }
                            }));
                        }
                        if !d.image.is_empty() && ui.button("Browse").clicked() {
                            let img = d.image.clone();
                            action = Some(Box::new(move |app: &mut App| app.media.open(Source::Drive(i), img)));
                        }
                    });
                });
            }
            ui.add_space(6.0);
            let roms: Vec<String> = s.cfg.roms.iter().enumerate().filter(|(_, r)| !r.is_empty()).map(|(i, r)| format!("socket {i}: {r}")).collect();
            ui.label(RichText::new("ROM sockets").strong());
            if roms.is_empty() {
                ui.label("empty");
            }
            for r in roms {
                ui.label(r);
            }
            if !s.cfg.bubble.is_empty() {
                ui.label(RichText::new("Bubble memory").strong());
                ui.label(&s.cfg.bubble);
            }
            ui.label(RichText::new("ROM chips, the hard disk and the bubble change with the machine off (Machine > Power off).").weak().small());
        }
    }
    if let Some(a) = action {
        a(app);
    }
}

fn browser(app: &mut App, ui: &mut egui::Ui) {
    let mp = &mut app.media;
    ui.horizontal_wrapped(|ui| {
        if ui.button("Open disk image...").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("GRiD disk image", &["img", "IMG", "imd", "IMD"]).pick_file() {
                // an .IMD is used through the .img beside it (made from it if missing)
                let r = crate::imd::floppy_path(&p).and_then(|(p, note)| std::fs::read(&p).map(|img| (p, img, note)).map_err(|e| e.to_string()));
                match r {
                    Ok((p, img, note)) => {
                        mp.open(Source::File(p), img);
                        if note.is_some() {
                            mp.message = note;
                        }
                    }
                    Err(e) => mp.message = Some(format!("{}: {e}", p.display())),
                }
            }
        }
        ui.separator();
        ui.label("New blank floppy, named");
        ui.add(egui::TextEdit::singleline(&mut mp.blank_name).hint_text("Floppy Disk").desired_width(120.0));
        if ui.button("Make...").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("GRiD floppy image", &["img"]).set_file_name("blank.img").save_file() {
                let name = if mp.blank_name.trim().is_empty() { "Floppy Disk".to_string() } else { mp.blank_name.trim().to_string() };
                let v = Volume::format(720, &name, hostdir::grid_time(std::time::SystemTime::now()));
                mp.message = Some(match std::fs::write(&p, &v.img) {
                    Ok(_) => format!("made {} (360 KB, named {name})", p.display()),
                    Err(e) => format!("{}: {e}", p.display()),
                });
            }
        }
    });
    ui.separator();
    let Some(src) = mp.source.clone() else {
        ui.label("Open a disk image, or Browse a drive on the left, to see its files.");
        if let Some(m) = &mp.message {
            ui.label(m);
        }
        return;
    };
    let writable = matches!(src, Source::File(_));
    // A drive's image is busy in the machine: refuse changes if the file is attached.
    let in_use = match (&src, &app.sess) {
        (Source::File(p), Some(s)) => s.media.iter().any(|m| matches!(m, Media::Image(q) if q == p)),
        _ => false,
    };
    let writable = writable && !in_use;
    ui.horizontal_wrapped(|ui| {
        let what = match &src {
            Source::Drive(i) => format!("drive {} (a copy taken now)", i + 1),
            Source::File(p) => p.display().to_string(),
        };
        ui.label(RichText::new(what).monospace());
        if mp.volumes.len() > 1 {
            let mut vi = mp.vol_index;
            egui::ComboBox::from_id_salt("vol").selected_text(format!("volume {}", vi + 1)).show_ui(ui, |ui| {
                for k in 0..mp.volumes.len() {
                    ui.selectable_value(&mut vi, k, format!("volume {}", k + 1));
                }
            });
            if vi != mp.vol_index {
                mp.vol_index = vi;
                mp.refresh();
            }
        }
        if !mp.vol_name.is_empty() {
            ui.label(format!("'{}', {} KB free", mp.vol_name, mp.free_kb));
        }
    });
    if !writable {
        ui.label(
            RichText::new(if in_use {
                "This image is attached to the running machine: export only (GRiD-OS caches parts of the disk)."
            }
            else {
                "A drive of the running machine: export only. Power off to change its files, or use the PC folder drive."
            })
            .weak()
            .small(),
        );
    }
    ui.horizontal_wrapped(|ui| {
        let sel = mp.selected.and_then(|i| mp.entries.get(i).cloned());
        if ui.add_enabled(sel.is_some(), egui::Button::new("Export to PC...")).clicked() {
            let e = sel.clone().unwrap();
            let host = hostdir::host_name(&e.name);
            if let Some(p) = rfd::FileDialog::new().set_file_name(&host).save_file() {
                let r = mp.volume().and_then(|v| v.read(e.fid).map_err(|e| e.0)).and_then(|d| std::fs::write(&p, d).map_err(|e| e.to_string()));
                mp.message = Some(match r {
                    Ok(_) => format!("exported {} to {}", e.name, p.display()),
                    Err(e) => e,
                });
            }
        }
        if ui.add_enabled(writable, egui::Button::new("Import from PC...")).clicked() {
            if let Some(files) = rfd::FileDialog::new().pick_files() {
                let subject = sel.as_ref().map(|e| e.subject.clone()).unwrap_or_default();
                mp.edit(|v| {
                    let mut done = vec![];
                    for f in files {
                        let host = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        let (name, text) = hostdir::grid_name(&host);
                        let mut data = std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?;
                        if text {
                            data = hostdir::to_crlf(&data);
                        }
                        v.put_file(&subject, &name, &data).map_err(|e| format!("{name}: {e}"))?;
                        done.push(name);
                    }
                    Ok(format!("imported {}", done.join(", ")))
                });
            }
        }
        if ui.add_enabled(writable && sel.is_some(), egui::Button::new("Delete")).clicked() {
            let e = sel.clone().unwrap();
            mp.edit(|v| {
                let dir = v.subject_dir(&e.subject).map_err(|e| e.0)?;
                v.delete(dir, e.fid).map_err(|e| e.0)?;
                Ok(format!("deleted {}", e.name))
            });
        }
        ui.separator();
        ui.add(egui::TextEdit::singleline(&mut mp.new_subject).hint_text("new subject").desired_width(120.0));
        if ui.add_enabled(writable && !mp.new_subject.trim().is_empty(), egui::Button::new("Add subject")).clicked() {
            let name = mp.new_subject.trim().to_string();
            mp.new_subject.clear();
            mp.edit(|v| {
                v.add_subject(&name).map_err(|e| e.0)?;
                Ok(format!("added subject {name}"))
            });
        }
    });
    if let Some(m) = &mp.message {
        ui.label(m);
    }
    ui.separator();
    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new("files").striped(true).num_columns(5).spacing([16.0, 4.0]).show(ui, |ui| {
            for h in ["Subject", "Title", "Kind", "Size", "Modified"] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for (i, e) in mp.entries.iter().enumerate() {
                let (title, kind) = match e.name.split_once('~') {
                    Some((t, k)) => (t.to_string(), k.trim_end_matches('~').to_string()),
                    None => (e.name.clone(), String::new()),
                };
                let subj = e.subject.split('~').next().unwrap_or("").to_string();
                let on = mp.selected == Some(i);
                if ui.selectable_label(on, subj).clicked() {
                    mp.selected = Some(i);
                }
                if ui.selectable_label(on, title).clicked() {
                    mp.selected = Some(i);
                }
                ui.label(kind);
                ui.label(format!("{}", e.size));
                ui.label(&e.modified);
                ui.end_row();
            }
        });
    });
}
