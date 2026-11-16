//! Floppy library (Media > Floppy library): every .img and .IMD in a folder (and its
//! sub-folders) with its GRiD volume name and file count; double-click one to put it in a
//! floppy drive of the running machine.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui::{self, RichText};
use grid_core::hostdir::{self, Volume};

use super::App;

#[derive(Default)]
pub struct Library {
    pub show: bool,
    /// The folder the list was read from, and the list.
    dir: String,
    list: Vec<Item>,
    /// The drive (index into the machine's GPIB drives) disks go into.
    drive: Option<usize>,
    selected: Option<usize>,
    filter: String,
}

struct Item {
    path: PathBuf,
    /// Shown relative to the library folder.
    rel: String,
    kb: u64,
    /// Volume name and file count, or why the disk can't be read.
    info: Result<(String, usize), String>,
    modified: Option<SystemTime>,
}

/// What is on a floppy file: (volume name, files). .IMD files are read without making the .img.
fn look(p: &Path) -> Result<(String, usize), String> {
    let d = std::fs::read(p).map_err(|e| e.to_string())?;
    let img = if crate::imd::is_imd(p) { crate::imd::to_img(&d)? } else { d };
    let &(st, n) = hostdir::volumes(&img).first().ok_or("not a GRiD disk")?;
    let v = Volume::open_any(img[st as usize * 512..(st + n) as usize * 512].to_vec()).map_err(|e| e.0)?;
    let files = v.files().map_err(|e| e.0)?.len();
    Ok((v.name(), files))
}

fn scan(dir: &Path, base: &Path, depth: usize, out: &mut Vec<Item>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            if depth < 3 {
                scan(&p, base, depth + 1, out);
            }
            continue;
        }
        let ext = p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if ext != "img" && ext != "imd" {
            continue;
        }
        let Ok(m) = std::fs::metadata(&p) else { continue };
        // floppies only (a hard disk image is 10 or 20 MB)
        if m.len() > 1_600_000 {
            continue;
        }
        // an .img made beside an .IMD is the same disk: list the .IMD only
        if ext == "img" && ["IMD", "imd"].iter().any(|x| p.with_extension(x).exists()) {
            continue;
        }
        let rel = p.strip_prefix(base).unwrap_or(&p).display().to_string();
        out.push(Item { info: look(&p), kb: m.len() / 1024, modified: m.modified().ok(), rel, path: p });
    }
}

impl App {
    pub(crate) fn library_window(&mut self, ctx: &egui::Context) {
        let mut open = self.library.show;
        let mut dir = self.setup.prefs().floppy_library.clone();
        let mut rescan = self.library.dir != dir;
        let drives = self.floppy_drives();
        if self.library.drive.is_none_or(|d| !drives.iter().any(|x| x.0 == d)) {
            self.library.drive = drives.first().map(|d| d.0);
        }
        let mut insert = None;
        egui::Window::new("Floppy library").open(&mut open).default_size([720.0, 480.0]).resizable(true).show(ctx, |ui| {
            let lib = &mut self.library;
            ui.horizontal(|ui| {
                ui.label("Folder");
                ui.add(egui::TextEdit::singleline(&mut dir).desired_width(360.0).hint_text("a folder with .img and .IMD floppies"));
                if ui.button("Browse").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        dir = p.display().to_string();
                    }
                }
                if ui.button("Rescan").clicked() {
                    rescan = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Into");
                if drives.is_empty() {
                    ui.label(RichText::new("power the machine on to insert disks (no floppy drive now)").weak());
                }
                else {
                    let cur = drives.iter().find(|d| Some(d.0) == lib.drive).map(|d| d.1.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt("lib-drive").selected_text(cur).show_ui(ui, |ui| {
                        for (i, name) in &drives {
                            ui.selectable_value(&mut lib.drive, Some(*i), name);
                        }
                    });
                }
                ui.add_space(12.0);
                ui.label("Find");
                ui.add(egui::TextEdit::singleline(&mut lib.filter).desired_width(160.0));
                let can = lib.selected.is_some() && lib.drive.is_some();
                if ui.add_enabled(can, egui::Button::new("Insert")).clicked() {
                    insert = lib.selected;
                }
            });
            ui.label(RichText::new(format!("{} floppies; double-click one to insert it", lib.list.len())).weak().small());
            ui.separator();
            let f = lib.filter.to_lowercase();
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("lib").striped(true).num_columns(4).spacing([14.0, 4.0]).show(ui, |ui| {
                    ui.label(RichText::new("File").strong());
                    ui.label(RichText::new("Volume").strong());
                    ui.label(RichText::new("Files").strong());
                    ui.label(RichText::new("Size").strong());
                    ui.end_row();
                    for (i, it) in lib.list.iter().enumerate() {
                        let vol = match &it.info {
                            Ok((n, _)) => n.clone(),
                            Err(e) => format!("({e})"),
                        };
                        if !f.is_empty() && !it.rel.to_lowercase().contains(&f) && !vol.to_lowercase().contains(&f) {
                            continue;
                        }
                        let r = ui.selectable_label(lib.selected == Some(i), &it.rel);
                        if r.clicked() {
                            lib.selected = Some(i);
                        }
                        if r.double_clicked() {
                            lib.selected = Some(i);
                            insert = Some(i);
                        }
                        match &it.info {
                            Ok((n, files)) => {
                                ui.label(n);
                                ui.label(files.to_string());
                            }
                            Err(e) => {
                                ui.label(RichText::new(e).weak());
                                ui.label("");
                            }
                        }
                        let date = it.modified.map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d").to_string()).unwrap_or_default();
                        ui.label(format!("{} KB  {date}", it.kb));
                        ui.end_row();
                    }
                });
            });
        });
        self.library.show = open;
        if dir != self.setup.prefs().floppy_library {
            let mut p = self.setup.prefs().clone();
            p.floppy_library = dir.clone();
            self.setup.set_prefs(p);
        }
        if rescan {
            let mut list = vec![];
            if !dir.trim().is_empty() {
                let base = PathBuf::from(dir.trim());
                scan(&base, &base, 0, &mut list);
            }
            self.library.list = list;
            self.library.selected = None;
            self.library.dir = dir;
        }
        if let (Some(i), Some(d)) = (insert, self.library.drive) {
            if let Some(p) = self.library.list.get(i).map(|it| it.path.clone()) {
                self.insert_at(d, Some(p));
            }
        }
    }
}
