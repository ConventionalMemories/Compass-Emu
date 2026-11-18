//! Saved states with a picture of the screen: Machine > Save state, and the Saved states
//! window (a grid of thumbnails to load or delete). They live in saved-states/<profile>/
//! next to compass.exe, a .state file and a .png of the screen each. A state holds no disks.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui::{self, Color32, RichText};

use super::App;
use crate::display;

#[derive(Default)]
pub struct SavedStates {
    pub show: bool,
    /// The folder the list was read from, and the list (newest first).
    dir: PathBuf,
    list: Vec<Entry>,
    stale: bool,
}

struct Entry {
    state: PathBuf,
    time: Option<SystemTime>,
    thumb: Option<egui::TextureHandle>,
    /// The colour the thumbnail was tinted with.
    tint: u32,
}

/// The folder for a profile's saved states.
pub fn folder(profile: &str) -> PathBuf {
    let name: String = profile.chars().map(|c| if c.is_alphanumeric() || " -_.()".contains(c) { c } else { '_' }).collect();
    let name = name.trim().trim_matches('.').to_string();
    crate::data_dir().join("saved-states").join(if name.is_empty() { "Compass".to_string() } else { name })
}

fn png_of(state: &Path) -> PathBuf {
    state.with_extension("png")
}

/// A grayscale PNG as a texture, lit pixels in `colour`.
fn load_thumb(ctx: &egui::Context, png_path: &Path, colour: u32) -> Option<egui::TextureHandle> {
    let f = std::fs::File::open(png_path).ok()?;
    let dec = png::Decoder::new(std::io::BufReader::new(f));
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let step = buf.len() / (w * h).max(1);
    let px: Vec<Color32> = (0..w * h)
        .map(|i| {
            let c = if buf[i * step] > 0x7F { colour } else { display::DARK };
            Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
        })
        .collect();
    let img = egui::ColorImage::new([w, h], px);
    Some(ctx.load_texture(png_path.display().to_string(), img, egui::TextureOptions::LINEAR))
}

impl SavedStates {
    fn refresh(&mut self, dir: &Path) {
        let mut list: Vec<Entry> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("state")))
                    .map(|p| Entry { time: std::fs::metadata(&p).and_then(|m| m.modified()).ok(), state: p, thumb: None, tint: 0 })
                    .collect()
            })
            .unwrap_or_default();
        list.sort_by(|a, b| b.time.cmp(&a.time));
        self.list = list;
        self.dir = dir.to_path_buf();
        self.stale = false;
    }
}

impl App {
    /// Machine > Save state: the state and a picture of the screen, into the profile's folder.
    pub(crate) fn save_state_slot(&mut self) {
        let Some(s) = &mut self.sess else { return };
        let dir = folder(&s.cfg.name);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.say(format!("{}: {e}", dir.display()));
            return;
        }
        let name = chrono::Local::now().format("%Y-%m-%d %H-%M-%S").to_string();
        let p = dir.join(format!("{name}.state"));
        let data = s.m.save_state();
        let (w, h) = s.m.screen_size();
        let pix = s.m.render_display();
        let r = std::fs::write(&p, data).map_err(|e| format!("{}: {e}", p.display())).and_then(|_| display::save_png(&png_of(&p).display().to_string(), &pix, w, h));
        match r {
            Ok(_) => self.say(format!("state saved: {name} (Machine > Saved states; disks are not part of it)")),
            Err(e) => self.say(e),
        }
        self.states.stale = true;
    }

    pub(crate) fn states_window(&mut self, ctx: &egui::Context) {
        let Some(s) = &self.sess else { return };
        let dir = folder(&s.cfg.name);
        if self.states.stale || self.states.dir != dir {
            self.states.refresh(&dir);
        }
        let colour = self.colour;
        let mut open = self.states.show;
        let mut load = None;
        let mut delete = None;
        let mut save = false;
        egui::Window::new("Saved states").open(&mut open).default_size([720.0, 480.0]).resizable(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Save state now").clicked() {
                    save = true;
                }
                ui.label(RichText::new(format!("{} ({} saved)", dir.display(), self.states.list.len())).weak().small());
            });
            ui.label(RichText::new("A state is the machine (memory, chips, CPU) at that moment; the disks are not in it, they stay as they are now.").weak().small());
            ui.separator();
            if self.states.list.is_empty() {
                ui.label("No saved states for this profile yet.");
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                    for (i, e) in self.states.list.iter_mut().enumerate() {
                        if e.thumb.is_none() || e.tint != colour {
                            e.thumb = load_thumb(ui.ctx(), &png_of(&e.state), colour);
                            e.tint = colour;
                        }
                        egui::Frame::group(ui.style()).inner_margin(6.0).show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(200.0);
                                let r = match &e.thumb {
                                    Some(t) => {
                                        let [w, h] = t.size();
                                        let size = egui::vec2(200.0, 200.0 * h as f32 / w as f32);
                                        ui.add(egui::Image::new(t).fit_to_exact_size(size).sense(egui::Sense::click()))
                                    }
                                    None => ui.add_sized([200.0, 150.0], egui::Label::new("no picture").sense(egui::Sense::click())),
                                };
                                if r.on_hover_text("double-click to load").double_clicked() {
                                    load = Some(i);
                                }
                                let title = e.state.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                                ui.label(title);
                                ui.horizontal(|ui| {
                                    if ui.button("Load").clicked() {
                                        load = Some(i);
                                    }
                                    if ui.button("Delete").clicked() {
                                        delete = Some(i);
                                    }
                                });
                            });
                        });
                    }
                });
            });
        });
        self.states.show = open;
        if save {
            self.save_state_slot();
        }
        if let Some(i) = load {
            let p = self.states.list[i].state.clone();
            self.load_state_path(&p);
        }
        if let Some(i) = delete {
            let p = self.states.list[i].state.clone();
            let _ = std::fs::remove_file(png_of(&p));
            match std::fs::remove_file(&p) {
                Ok(_) => self.say(format!("deleted {}", p.display())),
                Err(e) => self.say(format!("{}: {e}", p.display())),
            }
            self.states.stale = true;
        }
    }
}
