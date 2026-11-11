//! Debugger workspace: registers, disassembly, breakpoints, cycle counter marks, profiler,
//! memory and the console (the same commands as the 0.4 console debugger), plus a chips window.

use eframe::egui::{self, Color32, RichText};
use grid_core::{debug::parse_addr, screen};

use super::App;

const HILITE: Color32 = Color32::from_rgb(0xFF, 0xC9, 0x4D);

pub struct DebugPage {
    pub profile: Option<String>,
    console: String,
    mem_addr: String,
    bp_kind: usize,
    bp_text: String,
    dis_addr: String,
}

impl Default for DebugPage {
    fn default() -> Self {
        DebugPage { profile: None, console: String::new(), mem_addr: "0040:0000".into(), bp_kind: 0, bp_text: String::new(), dis_addr: String::new() }
    }
}

/// Run a console command and log what it printed.
pub fn command(app: &mut App, line: &str) {
    let (Some(s), Some(r)) = (&mut app.sess, &mut app.run) else { return };
    let out = r.command(&mut s.m, line);
    app.say(format!("> {line}"));
    if !out.trim().is_empty() {
        app.say(out.trim_end().to_string());
    }
}

fn section(ui: &mut egui::Ui, title: &str, open: bool, add: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).strong().color(HILITE)).default_open(open).show(ui, add);
}

pub fn panels(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::bottom("console").resizable(true).default_size(180.0).show_inside(ui, |ui| console(app, ui));
    egui::Panel::right("debugger").resizable(true).default_size(520.0).show_inside(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            controls(app, ui);
            section(ui, "Registers", true, |ui| registers(app, ui));
            section(ui, "Disassembly", true, |ui| disassembly(app, ui));
            section(ui, "Breakpoints and watches", true, |ui| breakpoints(app, ui));
            section(ui, "Cycle counter marks", true, |ui| marks(app, ui));
            section(ui, "Profiler", false, |ui| profiler(app, ui));
            section(ui, "Memory", false, |ui| memory(app, ui));
        });
    });
}

fn controls(app: &mut App, ui: &mut egui::Ui) {
    let paused = app.run.as_ref().map(|r| r.paused).unwrap_or(false);
    ui.horizontal_wrapped(|ui| {
        if ui.button(if paused { "Go on (F8)" } else { "Pause (F8)" }).clicked() {
            command(app, if paused { "g" } else { "p" });
        }
        if ui.add_enabled(paused, egui::Button::new("Step")).clicked() {
            command(app, "s");
        }
        if ui.add_enabled(paused, egui::Button::new("Step 10")).clicked() {
            command(app, "s 10");
        }
        if ui.add_enabled(paused, egui::Button::new("Run to next frame")).on_hover_text("break on IR3, the vertical sync tick").clicked() {
            let had = app.sess.as_ref().map(|s| s.m.board().debug.irqs.contains(&3)).unwrap_or(true);
            if let Some(s) = &mut app.sess {
                s.m.board_mut().debug.irqs.insert(3);
            }
            command(app, "g");
            // the IR3 breakpoint stays only if it was set before
            if !had {
                app.dbg_once_ir3 = true;
            }
        }
        if ui.button("Chips").clicked() {
            app.show_chips = !app.show_chips;
        }
    });
    if paused {
        if let Some(s) = &app.sess {
            let b = s.m.board();
            let beam = b.display.beam(b.clocks);
            ui.label(
                RichText::new(format!(
                    "Stopped. Beam on line {} of {} ({:.0}% into the frame), clock {}",
                    beam.line,
                    b.display.frame_lines,
                    (b.clocks % b.frame_clocks()) as f64 * 100.0 / b.frame_clocks() as f64,
                    b.clocks
                ))
                .color(HILITE),
            );
        }
    }
}

fn registers(app: &mut App, ui: &mut egui::Ui) {
    let Some(s) = &mut app.sess else { return };
    let (r, ip, f, _) = s.m.regs();
    let names = ["AX", "BX", "CX", "DX", "SP", "BP", "SI", "DI", "ES", "CS", "SS", "DS"];
    egui::Grid::new("regs").num_columns(8).spacing([10.0, 2.0]).show(ui, |ui| {
        for (i, n) in names.iter().enumerate() {
            ui.monospace(RichText::new(*n).weak());
            ui.monospace(format!("{:04X}", r[i]));
            if i % 4 == 3 {
                ui.end_row();
            }
        }
        ui.monospace(RichText::new("IP").weak());
        ui.monospace(format!("{ip:04X}"));
        ui.monospace(RichText::new("flags").weak());
        let flag = |b: u16, c: char| if f & (1 << b) != 0 { c } else { c.to_ascii_lowercase() };
        ui.monospace(format!(
            "{}{}{}{}{}{}{}{}{}",
            flag(11, 'O'),
            flag(10, 'D'),
            flag(9, 'I'),
            flag(8, 'T'),
            flag(7, 'S'),
            flag(6, 'Z'),
            flag(4, 'A'),
            flag(2, 'P'),
            flag(0, 'C')
        ));
        ui.end_row();
    });
    if let Some(fpu) = s.m.fpu() {
        ui.add_space(4.0);
        ui.label(RichText::new("8087 stack").weak());
        let mut any = false;
        for i in 0..8 {
            if let Some(v) = fpu.st_f64(i) {
                ui.monospace(format!("ST{i}  {v}"));
                any = true;
            }
        }
        if !any {
            ui.monospace("(empty)");
        }
    }
}

fn disassembly(app: &mut App, ui: &mut egui::Ui) {
    let Some(s) = &mut app.sess else { return };
    let mut toggle: Option<u32> = None;
    ui.horizontal(|ui| {
        ui.label("From");
        ui.add(egui::TextEdit::singleline(&mut app.dbg.dis_addr).hint_text("CS:IP").desired_width(110.0));
        if ui.small_button("CS:IP").clicked() {
            app.dbg.dis_addr.clear();
        }
    });
    let (cs, ip) = s.m.cs_ip();
    let (seg, off) = app
        .dbg
        .dis_addr
        .split_once(':')
        .and_then(|(a, b)| Some((u16::from_str_radix(a.trim(), 16).ok()?, u16::from_str_radix(b.trim(), 16).ok()?)))
        .unwrap_or((cs, ip));
    let text = s.m.disassemble(seg, off, 16);
    let pc = ((cs as u32) << 4) + ip as u32;
    for line in text.lines() {
        let lin = parse_addr(&line[..9.min(line.len())]).ok();
        ui.horizontal(|ui| {
            let bp = lin.map(|a| s.m.board().debug.exec.contains(&a)).unwrap_or(false);
            let dot = RichText::new(if bp { "\u{25CF}" } else { "\u{25CB}" }).color(if bp { Color32::from_rgb(0xFF, 0x60, 0x50) } else { Color32::GRAY });
            if ui.add(egui::Button::new(dot).frame(false)).on_hover_text("breakpoint on / off").clicked() {
                toggle = lin;
            }
            let t = RichText::new(line).monospace();
            ui.label(if lin.map(|a| a & 0xFFFFF) == Some(pc & 0xFFFFF) { t.background_color(Color32::from_rgb(0x3D, 0x2E, 0x05)).color(HILITE) } else { t });
        });
    }
    if let Some(a) = toggle {
        let ex = &mut s.m.board_mut().debug.exec;
        if !ex.remove(&a) {
            ex.insert(a);
        }
    }
}

fn breakpoints(app: &mut App, ui: &mut egui::Ui) {
    let Some(s) = &mut app.sess else { return };
    let dbg = &mut s.m.board_mut().debug;
    let mut remove: Option<(u8, u32)> = None;
    for a in dbg.exec.iter() {
        ui.horizontal(|ui| {
            if ui.small_button("x").clicked() {
                remove = Some((0, *a));
            }
            ui.monospace(format!("code at {:04X}:{:04X} ({a:05X})", a >> 4, a & 15));
        });
    }
    for p in dbg.ports.iter() {
        ui.horizontal(|ui| {
            if ui.small_button("x").clicked() {
                remove = Some((1, *p as u32));
            }
            ui.monospace(format!("IN/OUT port {p:02X}h"));
        });
    }
    for n in dbg.irqs.iter() {
        ui.horizontal(|ui| {
            if ui.small_button("x").clicked() {
                remove = Some((2, *n as u32));
            }
            ui.monospace(format!("interrupt IR{n}{}", if *n == 3 { " (vertical sync)" } else { "" }));
        });
    }
    for (i, w) in dbg.watches.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui.small_button("x").clicked() {
                remove = Some((3, i as u32));
            }
            ui.monospace(format!("memory write {:05X}, {} byte(s)", w.addr, w.len));
        });
    }
    match remove {
        Some((0, a)) => {
            dbg.exec.remove(&a);
        }
        Some((1, p)) => {
            dbg.ports.remove(&(p as u16));
        }
        Some((2, n)) => {
            dbg.irqs.remove(&(n as u8));
        }
        Some((3, i)) => {
            dbg.watches.remove(i as usize);
        }
        _ => {}
    }
    let kinds = ["Code address", "Port", "Interrupt IR", "Memory write"];
    let mut add = None;
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("bpkind").selected_text(kinds[app.dbg.bp_kind]).show_ui(ui, |ui| {
            for (i, k) in kinds.iter().enumerate() {
                ui.selectable_value(&mut app.dbg.bp_kind, i, *k);
            }
        });
        let hint = ["SSSS:OOOO", "DFE4", "0-7", "SSSS:OOOO [len]"][app.dbg.bp_kind];
        ui.add(egui::TextEdit::singleline(&mut app.dbg.bp_text).hint_text(hint).desired_width(130.0));
        if ui.button("Add").clicked() && !app.dbg.bp_text.trim().is_empty() {
            add = Some(format!("{} {}", ["b", "bio", "bi", "bw"][app.dbg.bp_kind], app.dbg.bp_text.trim()));
        }
        if ui.button("Clear all").clicked() {
            add = Some("bc".into());
        }
    });
    if let Some(c) = add {
        app.dbg.bp_text.clear();
        command(app, &c);
    }
}

fn marks(app: &mut App, ui: &mut egui::Ui) {
    let Some(s) = &mut app.sess else { return };
    let b = s.m.board();
    let fc = b.frame_clocks();
    let mk = &b.marks;
    if mk.is_empty() {
        ui.label("No marks yet. In your program: OUT 0F0h,AL before a section and OUT 0F2h,AL after it (AL = section number), OUT 0F4h,AL when a frame is drawn.");
    }
    else {
        egui::Grid::new("marks").striped(true).num_columns(7).spacing([12.0, 2.0]).show(ui, |ui| {
            for h in ["sec", "count", "last", "min", "avg", "max", "% frame"] {
                ui.label(RichText::new(h).weak());
            }
            ui.end_row();
            for (id, st) in &mk.sections {
                ui.monospace(format!("{id}"));
                ui.monospace(format!("{}", st.count));
                ui.monospace(format!("{}", st.last));
                ui.monospace(format!("{}", st.min));
                ui.monospace(format!("{:.0}", st.avg()));
                ui.monospace(format!("{}", st.max));
                ui.monospace(format!("{:.1}", st.avg() * 100.0 / fc as f64));
                ui.end_row();
            }
        });
        if mk.frame.count > 0 {
            let p = mk.frame.avg() / fc as f64;
            ui.add(egui::ProgressBar::new(p as f32).text(format!(
                "frame {:.0}% used ({:.0}-{:.0}%), line {}, missed {}",
                p * 100.0,
                mk.frame.min as f64 * 100.0 / fc as f64,
                mk.frame.max as f64 * 100.0 / fc as f64,
                mk.frame_line,
                mk.frames_missed
            )));
        }
    }
    let mut clear = false;
    let mut save = None;
    ui.horizontal(|ui| {
        clear = ui.button("Clear (F9)").clicked();
        if ui.button("Save report...").clicked() {
            save = Some(mk.report(fc, screen::CPU_HZ));
        }
    });
    if clear {
        s.m.board_mut().marks.reset();
    }
    if let Some(r) = save {
        if let Some(p) = rfd::FileDialog::new().set_file_name("compass-marks.txt").save_file() {
            match std::fs::write(&p, r) {
                Ok(_) => app.say(format!("saved {}", p.display())),
                Err(e) => app.say(format!("{}: {e}", p.display())),
            }
        }
        app.resync();
    }
}

fn profiler(app: &mut App, ui: &mut egui::Ui) {
    let running = app.sess.as_ref().map(|s| s.m.board().debug.profile.is_some()).unwrap_or(false);
    ui.horizontal(|ui| {
        for n in [60, 300] {
            if ui.add_enabled(!running, egui::Button::new(format!("Profile {n} frames"))).clicked() {
                command(app, &format!("prof {n}"));
            }
        }
        if running {
            ui.spinner();
            ui.label("profiling...");
        }
    });
    if let Some(p) = &app.dbg.profile {
        egui::ScrollArea::both().max_height(260.0).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(p).monospace()).extend());
        });
    }
}

fn memory(app: &mut App, ui: &mut egui::Ui) {
    let Some(s) = &mut app.sess else { return };
    ui.horizontal(|ui| {
        ui.label("Address");
        ui.add(egui::TextEdit::singleline(&mut app.dbg.mem_addr).desired_width(110.0));
        if ui.small_button("DS:SI").clicked() {
            let (r, ..) = s.m.regs();
            app.dbg.mem_addr = format!("{:04X}:{:04X}", r[11], r[6]);
        }
        if ui.small_button("ES:DI").clicked() {
            let (r, ..) = s.m.regs();
            app.dbg.mem_addr = format!("{:04X}:{:04X}", r[8], r[7]);
        }
        if ui.small_button("SS:SP").clicked() {
            let (r, ..) = s.m.regs();
            app.dbg.mem_addr = format!("{:04X}:{:04X}", r[10], r[4]);
        }
    });
    match parse_addr(&app.dbg.mem_addr) {
        Ok(a) => {
            ui.add(egui::Label::new(RichText::new(s.m.dump(a, 256)).monospace()).extend());
        }
        Err(e) => {
            ui.label(e);
        }
    }
}

fn console(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Console").strong());
        ui.label(RichText::new("h = help; the same commands as the console debugger").weak());
        if ui.small_button("Clear").clicked() {
            app.log.clear();
        }
    });
    let mut run_cmd = None;
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut app.dbg.console).hint_text("command, e.g. b 1A3C:0142 or m 0040:0000").desired_width(f32::INFINITY));
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            run_cmd = Some(std::mem::take(&mut app.dbg.console));
            r.request_focus();
        }
    });
    egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        for l in &app.log {
            ui.monospace(l);
        }
    });
    if let Some(c) = run_cmd {
        if !c.trim().is_empty() {
            command(app, c.trim());
        }
    }
}

pub fn chips_window(app: &mut App, ctx: &egui::Context) {
    let mut open = app.show_chips;
    egui::Window::new("Chips").open(&mut open).default_size([520.0, 520.0]).show(ctx, |ui| {
        let Some(s) = &mut app.sess else {
            ui.label("The machine is off.");
            return;
        };
        egui::ScrollArea::vertical().show(ui, |ui| {
            let b = s.m.board_mut();
            section(ui, "8259 interrupt controller (80130)", true, |ui| {
                let st = b.pic().get_string_state();
                egui::Grid::new("pic").num_columns(2).show(ui, |ui| {
                    for (k, v) in [("IRR", &st.irr), ("ISR", &st.isr), ("IMR", &st.imr), ("IR lines", &st.ir), ("INTR", &st.intr), ("Trigger", &st.trigger_mode)] {
                        ui.label(k);
                        ui.monospace(v);
                        ui.end_row();
                    }
                });
                ui.label(RichText::new("IR0 serial/sound, IR3 vertical sync tick, IR4 keyboard; bits are IR7..IR0").weak().small());
                egui::Grid::new("picst").striped(true).num_columns(4).show(ui, |ui| {
                    for h in ["line", "IMR masked", "ISR masked", "serviced"] {
                        ui.label(RichText::new(h).weak());
                    }
                    ui.end_row();
                    for (i, (a, c, d)) in st.interrupt_stats.iter().enumerate() {
                        ui.monospace(format!("IR{i}"));
                        ui.monospace(a);
                        ui.monospace(c);
                        ui.monospace(d);
                        ui.end_row();
                    }
                });
            });
            section(ui, "80130 timers", true, |ui| {
                egui::Grid::new("pit").num_columns(4).show(ui, |ui| {
                    for h in ["timer", "count", "loaded", "mode"] {
                        ui.label(RichText::new(h).weak());
                    }
                    ui.end_row();
                    for (i, (c, l, m)) in b.timers().iter().enumerate() {
                        ui.monospace(format!("{i}"));
                        ui.monospace(format!("{c:04X}"));
                        ui.monospace(format!("{l:04X}"));
                        ui.monospace(format!("{m}"));
                        ui.end_row();
                    }
                });
                ui.label(RichText::new("Timer 2 is the 8274 baud clock; no timer reaches an IRQ on the Compass").weak().small());
            });
            section(ui, "GPIB (TMS9914) and drives", true, |ui| {
                for d in &b.gpib.drives {
                    ui.monospace(format!(
                        "address {}  {:?}  {} KB  {}{}",
                        d.kind.address(),
                        d.kind,
                        d.image.len() / 1024,
                        if d.busy() { "busy" } else { "idle" },
                        if d.dirty { ", changed" } else { "" }
                    ));
                }
                ui.checkbox(&mut b.gpib.trace_on, "Trace GPIB traffic");
                if !b.gpib.trace.is_empty() {
                    if ui.small_button("Clear trace").clicked() {
                        b.gpib.trace.clear();
                    }
                    egui::ScrollArea::vertical().id_salt("gpibtrace").max_height(160.0).stick_to_bottom(true).show(ui, |ui| {
                        let n = b.gpib.trace.len();
                        for l in &b.gpib.trace[n.saturating_sub(300)..] {
                            ui.monospace(l);
                        }
                    });
                }
            });
            section(ui, "8274 serial", false, |ui| {
                let sr = &b.serial;
                ui.monospace(format!("bit rate {:.0}", sr.bit_rate()));
                for (i, ch) in sr.ch.iter().enumerate() {
                    let wr: Vec<String> = ch.wr.iter().map(|v| format!("{v:02X}")).collect();
                    ui.monospace(format!("channel {}: WR0-7 {}  DCD {} CTS {} DTR {} RTS {}", ["A", "B"][i], wr.join(" "), ch.dcd as u8, ch.cts as u8, ch.dtr() as u8, ch.rts() as u8));
                }
                ui.monospace(format!("waiting to be received: {} bytes", sr.rx_in.len()));
            });
            section(ui, "8741 keyboard controller and sound", false, |ui| {
                if let Some(k) = &b.keyboard {
                    ui.monospace(format!("modifiers {:02X}  IRQ {}  DMA allowed {}", k.mods(), k.irq() as u8, k.dma_enabled() as u8));
                }
                match b.sound.dtmf() {
                    Some((lo, hi)) => ui.monospace(format!("DTMF tone {lo:.0} + {hi:.0} Hz")),
                    None => ui.monospace("DTMF off"),
                };
            });
        });
    });
    app.show_chips = open;
}
