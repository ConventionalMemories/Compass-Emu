//! Compass-Emu 0.4.0: a GRiD Compass in a window.
//!
//! compass [options]
//!   --model M                    1101 (default), 1109, 1121, 1129, 1131 or 1139
//!   --prom-even F --prom-odd F   boot PROM chips (default: see default_prom)
//!   --prom F                     boot PROM as one 16K file (even/odd bytes interleaved)
//!   --kbd F                      8741 keyboard firmware (default roms/300067-01.bin; the
//!                                Compass II models use roms/300001-02.bin when it is there)
//!   --hd F                       2101 hard disk image (GPIB 4)
//!   --flop F                     2102 floppy image (GPIB 6)
//!   --flop2101 F                 2101 floppy image (GPIB 5)
//!   --folder D                   a PC folder as the 2101 "Floppy Disk" (GPIB 5): its files and
//!                                sub-folders (= subjects) are copied in at start, and files GRiD
//!                                made, changed or erased are written back when you quit
//!   --rom0 F .. --rom3 F         application ROM chip in socket 0-3 (Compass II models only; a
//!                                32K, 64K or 128K dump, e.g. from Rom Toolbox)
//!   --bubble F                   bubble memory image (384K; a new blank one is made if F is missing)
//!   --serial S                   serial port: tcp:PORT (listen on localhost), tcp:HOST:PORT,
//!                                connect:[HOST:]PORT (link to another emulator) or a port name (COM3)
//!   --mute                       no sound
//!   --readonly                   never write disk changes back to the image files
//!   --scale N                    window scale 1, 2 or 4 (default 4)
//!   --overlay                    start with the demo tools overlay on (F10)
//! Keys: PC keys map to the Compass keys by position; Alt = CODE, Ctrl = CTRL.
//!   Debugger: type commands in the console window (h = help), F8 = pause / go on.
//!   F9 = clear the cycle counter marks, F10 = demo tools overlay (marks, see
//!   grid_core::marks), F11 = save screenshot, F12 = turbo on/off, Esc = Compass ESC. Close the window to quit
//!   (changed disks are written back unless --readonly).

use std::{
    fs,
    io::BufWriter,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use grid_core::{
    gpib::{Drive, DriveKind},
    keyboard::{self, Keyboard},
    screen,
    GridMachine,
    GridModel,
};
mod headless;
mod mcp;
mod host;
#[cfg(windows)]
mod winkeys;

use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};

pub const VERSION: &str = "0.4.0";
const AMBER: u32 = 0xFFB040;
const DARK: u32 = 0x1A0E00;
/// Overlay colours (demo tools): text box and the beam tick of the last frame mark.
const OVL_FG: u32 = 0xFFFFFF;
const OVL_BG: u32 = 0x003040;
const BEAM_TICK: u32 = 0x40D0FF;
/// 8x8 font for the overlay (MartyPC's CGA font: row r of glyph g at r * 256 + g).
const FONT: &[u8] = include_bytes!("../../../lib/marty_core/assets/cga_8by8.bin");

/// A breakpoint was met: pause and show where.
fn stop(m: &mut GridMachine, why: &str, paused: &mut bool) {
    if why.starts_with("profile done") {
        println!("{why}");
        return;
    }
    *paused = true;
    let (cs, ip) = m.cs_ip();
    println!("{why}\n{}\n{}", m.reg_dump(), m.disassemble(cs, ip, 3));
}

fn draw_text(buf: &mut [u32], w: usize, x: usize, y: usize, text: &str) {
    for (i, ch) in text.bytes().enumerate() {
        let x0 = x + i * 8;
        if x0 + 8 > w {
            break;
        }
        for r in 0..8 {
            let bits = FONT[r * 256 + ch as usize];
            for c in 0..8 {
                let o = (y + r) * w + x0 + c;
                if o < buf.len() {
                    buf[o] = if bits & (0x80 >> c) != 0 { OVL_FG } else { OVL_BG };
                }
            }
        }
    }
}

/// Demo tools overlay: the cycle counter table at the top and a tick at the left edge on
/// the display line where the last frame mark was reached.
fn draw_overlay(buf: &mut [u32], w: usize, h: usize, marks: &grid_core::marks::Marks, frame_clocks: u64) {
    let mut lines = marks.lines(frame_clocks);
    if marks.is_empty() {
        lines = vec!["no marks: OUT 0F0h/0F2h,AL".into(), "F9 = clear  F10 = hide".into()];
    }
    for (i, l) in lines.iter().take(h / 8).enumerate() {
        draw_text(buf, w, 0, i * 8, l);
    }
    if marks.frame.count > 0 && (marks.frame_line as usize) < h {
        let y = marks.frame_line as usize;
        for x in 0..12.min(w) {
            buf[y * w + x] = BEAM_TICK;
        }
    }
}
/// Shortest time a key stays down for the Compass (CPU clocks, 60 ms), so quick taps are
/// seen by the 8741 scan.
const MIN_HOLD: u64 = 300_000;

fn usage() -> ! {
    eprintln!(
        "Compass-Emu {VERSION}\nusage: compass [--model 1101|1109|1121|1129|1131|1139] [--hd disk.img] [--flop floppy.img] [--flop2101 floppy.img] [--folder DIR] [--rom0..3 chip.bin] [--bubble bubble.img]\n       [--serial tcp:PORT|connect:[HOST:]PORT|COMn] [--mute] [--prom-even F --prom-odd F | --prom F] [--kbd F] [--readonly] [--scale N] [--overlay]\nROM defaults are looked for in a roms folder next to the program.\n       compass do [--model M] --hd disk.img [--shot F] [--max-seconds N] \"Do build\"   (headless, see headless.rs)"
    );
    std::process::exit(1)
}

fn default_path(name: &str) -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    for base in [exe_dir, Some(PathBuf::from("."))].into_iter().flatten() {
        let p = base.join("roms").join(name);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("roms").join(name)
}

fn read(path: &Path, what: &str) -> Vec<u8> {
    match fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cannot read {} {}: {}", what, path.display(), e);
            std::process::exit(1)
        }
    }
}

/// PC key -> (column, bit) in the Compass matrix.
#[cfg_attr(windows, allow(dead_code))]
fn matrix_pos(k: Key) -> Option<(usize, u8)> {
    let c = match k {
        Key::A => 'a', Key::B => 'b', Key::C => 'c', Key::D => 'd', Key::E => 'e', Key::F => 'f',
        Key::G => 'g', Key::H => 'h', Key::I => 'i', Key::J => 'j', Key::K => 'k', Key::L => 'l',
        Key::M => 'm', Key::N => 'n', Key::O => 'o', Key::P => 'p', Key::Q => 'q', Key::R => 'r',
        Key::S => 's', Key::T => 't', Key::U => 'u', Key::V => 'v', Key::W => 'w', Key::X => 'x',
        Key::Y => 'y', Key::Z => 'z',
        Key::Key0 => '0', Key::Key1 => '1', Key::Key2 => '2', Key::Key3 => '3', Key::Key4 => '4',
        Key::Key5 => '5', Key::Key6 => '6', Key::Key7 => '7', Key::Key8 => '8', Key::Key9 => '9',
        Key::Space => ' ', Key::Enter | Key::NumPadEnter => '\r', Key::Tab => '\t',
        Key::Escape => '\x1b', Key::Backspace => '\x08',
        Key::Minus => '-', Key::Equal => '=', Key::Comma => ',', Key::Period => '.',
        Key::Slash => '/', Key::Semicolon => ';', Key::Apostrophe => '\'',
        Key::Left => '\u{e000}', Key::Down => '\u{e001}', Key::Right => '\u{e002}', Key::Up => '\u{e003}',
        _ => return None,
    };
    keyboard::key_for_char(c).map(|(col, bit, _)| (col, bit))
}

fn save_png(path: &str, pix: &[u8], w: usize, h: usize) {
    if let Ok(f) = fs::File::create(path) {
        let mut enc = png::Encoder::new(BufWriter::new(f), w as u32, h as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut w) = enc.write_header() {
            let data: Vec<u8> = pix.iter().map(|&p| if p != 0 { 0xFF } else { 0x00 }).collect();
            let _ = w.write_image_data(&data);
        }
    }
}

fn main() {
    let all: Vec<String> = std::env::args().skip(1).collect();
    if all.first().map(|a| a == "do").unwrap_or(false) {
        std::process::exit(headless::run(all[1..].to_vec(), default_path("")));
    }
    if all.first().map(|a| a == "mcp").unwrap_or(false) {
        std::process::exit(mcp::run(default_path("")));
    }
    let mut args = std::env::args().skip(1);
    let mut model = GridModel::Compass1101;
    let mut prom_even: Option<PathBuf> = None;
    let mut prom_odd: Option<PathBuf> = None;
    let mut prom: Option<PathBuf> = None;
    let mut kbd: Option<PathBuf> = None;
    let mut drives: Vec<(DriveKind, PathBuf)> = Vec::new();
    let mut folder: Option<PathBuf> = None;
    let mut rom_files: Vec<(usize, PathBuf)> = Vec::new();
    let mut readonly = false;
    let mut overlay_on = false;
    let mut bubble: Option<PathBuf> = None;
    let mut serial_spec: Option<String> = None;
    let mut mute = false;
    let mut scale = 4u32;
    while let Some(a) = args.next() {
        let mut val = || args.next().map(PathBuf::from).unwrap_or_else(|| usage());
        match a.as_str() {
            "--model" => {
                let v = val();
                model = GridModel::parse(&v.to_string_lossy()).unwrap_or_else(|| {
                    eprintln!("unknown model {} (1101, 1109, 1121, 1129, 1131, 1139)", v.display());
                    std::process::exit(1)
                })
            }
            "--prom-even" => prom_even = Some(val()),
            "--prom-odd" => prom_odd = Some(val()),
            "--prom" => prom = Some(val()),
            "--kbd" => kbd = Some(val()),
            "--hd" => drives.push((DriveKind::Hdd2101, val())),
            "--flop" => drives.push((DriveKind::Floppy2102, val())),
            "--flop2101" => drives.push((DriveKind::Floppy2101, val())),
            "--folder" => folder = Some(val()),
            "--rom0" | "--rom1" | "--rom2" | "--rom3" => rom_files.push(((a.as_bytes()[5] - b'0') as usize, val())),
            "--readonly" => readonly = true,
            "--overlay" => overlay_on = true,
            "--bubble" => bubble = Some(val()),
            "--serial" => serial_spec = Some(val().to_string_lossy().to_string()),
            "--mute" => mute = true,
            "--scale" => scale = val().to_string_lossy().parse().unwrap_or(4),
            _ => usage(),
        }
    }

    let roms = grid_core::roms::find(model, &default_path(""));
    let (even, odd) = if let Some(p) = &prom {
        let d = read(p, "boot PROM");
        (d.iter().step_by(2).copied().collect(), d.iter().skip(1).step_by(2).copied().collect())
    }
    else {
        if prom_even.is_none() {
            if let Some(n) = &roms.note {
                eprintln!("{n}");
            }
        }
        (
            read(prom_even.as_ref().unwrap_or(&roms.prom_even), "boot PROM (even)"),
            read(&prom_odd.unwrap_or(roms.prom_odd.clone()), "boot PROM (odd)"),
        )
    };
    let kbd = kbd.unwrap_or(roms.keyboard.clone());
    let mut m = GridMachine::new(model, &even, &odd);
    // A 1101 PROM standing in on a split board needs the 1101's DMA window too (also on for a
    // PROM given on the command line: it does no harm there).
    m.board_mut().e0000_dma = roms.e0000_dma || (model.split_board() && !model.wide_screen() && (prom.is_some() || prom_even.is_some()));
    m.board_mut().keyboard = Some(Keyboard::new(read(&kbd, "keyboard firmware")));
    let (sw, sh) = m.screen_size();
    let title = format!("Compass-Emu {VERSION} - GRiD Compass {}", model.name());
    for (kind, path) in &drives {
        let img = read(path, "disk image");
        m.board_mut().gpib.drives.push(Drive::new(*kind, img));
    }
    let hostdir = folder.as_ref().map(|dir| {
        if drives.iter().any(|d| d.0 == DriveKind::Floppy2101) {
            eprintln!("--folder and --flop2101 both use the 2101 floppy (GPIB 5); pick one");
            std::process::exit(1)
        }
        match grid_core::hostdir::HostDir::import(dir) {
            Ok((hd, vol)) => {
                println!("folder {} is the Floppy Disk ({} KB free space)", dir.display(), vol.pages() / 2);
                m.board_mut().gpib.drives.push(Drive::new(DriveKind::Floppy2101, vol.img));
                hd
            }
            Err(e) => {
                eprintln!("cannot use folder: {e}");
                std::process::exit(1)
            }
        }
    });

    m.board_mut().gpib.add_2101_floppy_slot();
    for (slot, path) in &rom_files {
        let chip = read(path, "ROM chip");
        if let Err(e) = m.board_mut().insert_rom(*slot, chip) {
            eprintln!("{}: {e}", path.display());
            std::process::exit(1)
        }
    }
    if let Some(p) = &bubble {
        let img = fs::read(p).unwrap_or_else(|_| vec![0u8; 3 * grid_core::bubble::MBM_SIZE]);
        m.board_mut().bubble = Some(grid_core::bubble::Bubble::new(img));
    }
    let mut serial = serial_spec.as_deref().map(|sp| match host::SerialLink::open(sp) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot open serial {sp}: {e}");
            std::process::exit(1)
        }
    });
    let audio = if mute { None } else { host::Audio::open() };
    match &audio {
        Some(a) => m.board_mut().sound = grid_core::sound::Sound::new(screen::CPU_HZ, a.rate),
        None => m.board_mut().sound.enabled = false,
    }
    let mut serial_out: Vec<u8> = Vec::new();

    let scale_opt = match scale {
        1 => Scale::X1,
        2 => Scale::X2,
        _ => Scale::X4,
    };
    let mut win = Window::new(
        &title,
        sw,
        sh,
        WindowOptions { scale: scale_opt, resize: true, ..WindowOptions::default() },
    )
    .unwrap_or_else(|e| {
        eprintln!("cannot open window: {e}");
        std::process::exit(1)
    });
    win.set_target_fps(60);
    #[cfg(windows)]
    {
        winkeys::install(win.get_window_handle());
        for &(_, vk) in winkeys::KEYS {
            let _ = winkeys::vk(vk); // clear stale "pressed since" bits
        }
    }
    // key position -> (press clock, still held)
    let mut held: std::collections::HashMap<(usize, u8), (u64, bool)> = Default::default();

    let mut buf = vec![DARK; sw * sh];
    let mut turbo = false;
    // Real-time pacing: emulated clock `base_clock` matched wall time `base_wall`.
    let mut base_wall = Instant::now();
    let mut base_clock = 0u64;
    let mut last_title = Instant::now();
    let mut last_clock = 0u64;
    let mut shot = 0;
    let mut overlay = overlay_on;
    // Debugger console: commands typed in the console window (h for help), F8 = pause/go.
    let (dbg_tx, dbg_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut line = String::new();
        while std::io::stdin().read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
            if dbg_tx.send(line.trim().to_string()).is_err() {
                break;
            }
            line.clear();
        }
    });
    let mut paused = false;

    while win.is_open() {
        // Keyboard: keys held now plus keys tapped since the last frame, each kept down at
        // least MIN_HOLD so the 8741 scan sees it.
        let now = m.board().clocks;
        let mut mods = 0u8;
        let mut down: Vec<(usize, u8)> = Vec::new();
        let mut tapped: Vec<(usize, u8)> = Vec::new();
        // On Windows the keys come only from GetAsyncKeyState (winkeys): minifb's own key
        // list misses key-up messages (focus lost while a key is down, Alt+Esc sending the
        // window back, Alt combinations), so a key could stay "down" for good: Shift stuck
        // on, or three keys held so the 8741 stops reporting keys at all.
        #[cfg(not(windows))]
        for k in win.get_keys() {
            match k {
                Key::LeftShift | Key::RightShift => mods |= keyboard::MOD_SHIFT,
                Key::LeftCtrl | Key::RightCtrl => mods |= keyboard::MOD_CTRL,
                Key::LeftAlt | Key::RightAlt => mods |= keyboard::MOD_CODE,
                _ => {
                    if let Some(p) = matrix_pos(k) {
                        down.push(p);
                    }
                }
            }
        }
        #[cfg(not(windows))]
        for k in win.get_keys_pressed(KeyRepeat::No) {
            if let Some(p) = matrix_pos(k) {
                tapped.push(p);
            }
        }
        #[cfg(windows)]
        {
            for &(c, vk) in winkeys::KEYS {
                let (d, t) = winkeys::vk(vk);
                if let Some((col, bit, _)) = keyboard::key_for_char(c) {
                    if d {
                        down.push((col, bit));
                    }
                    if t {
                        tapped.push((col, bit));
                    }
                }
            }
            if winkeys::vk(winkeys::VK_SHIFT).0 {
                mods |= keyboard::MOD_SHIFT;
            }
            if winkeys::vk(winkeys::VK_CONTROL).0 {
                mods |= keyboard::MOD_CTRL;
            }
            if winkeys::vk(winkeys::VK_MENU).0 {
                mods |= keyboard::MOD_CODE;
            }
        }
        for p in tapped.iter().chain(down.iter()) {
            held.entry(*p).or_insert((now, true));
        }
        for (p, st) in held.iter_mut() {
            st.1 = down.contains(p);
        }
        held.retain(|_, st| st.1 || now.saturating_sub(st.0) < MIN_HOLD);
        let mut cols = [0u8; 8];
        for (c, b) in held.keys() {
            cols[*c] |= *b;
        }
        if let Some(k) = &mut m.board_mut().keyboard {
            k.set_matrix(cols, mods);
        }
        if win.is_key_pressed(Key::F12, KeyRepeat::No) {
            turbo = !turbo;
            base_wall = Instant::now();
            base_clock = m.board().clocks;
        }
        let was_paused = paused;
        if win.is_key_pressed(Key::F8, KeyRepeat::No) {
            let (out, p) = m.debug_command(if paused { "g" } else { "p" }, paused);
            print!("{out}");
            paused = p;
        }
        while let Ok(line) = dbg_rx.try_recv() {
            let (out, p) = m.debug_command(&line, paused);
            print!("{out}");
            paused = p;
        }
        if was_paused && !paused {
            base_wall = Instant::now();
            base_clock = m.board().clocks;
        }

        // Run the emulation: real time, or as much as fits in ~15 ms in turbo.
        if paused {
            // stopped in the debugger: only the window and the console run
        }
        else if turbo {
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(15) {
                if let Some(why) = m.run_debug(50_000) {
                    stop(&mut m, &why, &mut paused);
                    break;
                }
            }
        }
        else {
            let target = base_clock + (base_wall.elapsed().as_secs_f64() * screen::CPU_HZ) as u64;
            let now = m.board().clocks;
            let max_step = (0.1 * screen::CPU_HZ) as u64;
            let behind = target.saturating_sub(now);
            if behind > max_step || now > target + max_step {
                // Too far behind (slow PC, window dragged) or ahead: start pacing afresh.
                base_wall = Instant::now();
                base_clock = now + behind.min(max_step);
            }
            let behind = behind.min(max_step);
            if behind > 0 {
                if let Some(why) = m.run_debug(behind) {
                    stop(&mut m, &why, &mut paused);
                }
            }
        }

        // Sound and serial port.
        {
            let b = m.board_mut();
            if let Some(a) = &audio {
                a.push(b.sound.out.drain(..));
            }
            if let Some(link) = &mut serial {
                serial_out.clear();
                serial_out.extend(b.serial.tx_out.drain(..).map(|(_, c)| c));
                let rate = b.serial.bit_rate();
                link.pump(&serial_out, &mut b.serial.rx_in, rate);
                let up = link.connected();
                b.serial.set_modem_lines(up, up);
            }
            else {
                b.serial.tx_out.clear();
            }
        }

        let pix = m.render_display();
        for (o, &p) in buf.iter_mut().zip(pix.iter()) {
            *o = if p != 0 { AMBER } else { DARK };
        }
        if win.is_key_pressed(Key::F10, KeyRepeat::No) {
            overlay = !overlay;
            let b = m.board();
            print!("{}", b.marks.report(b.frame_clocks(), screen::CPU_HZ));
        }
        if win.is_key_pressed(Key::F9, KeyRepeat::No) {
            m.board_mut().marks.reset();
        }
        if overlay {
            let b = m.board();
            draw_overlay(&mut buf, sw, sh, &b.marks, b.frame_clocks());
        }
        if win.is_key_pressed(Key::F11, KeyRepeat::No) {
            shot += 1;
            let name = format!("compass-{shot:03}.png");
            save_png(&name, &pix, sw, sh);
        }
        let _ = win.update_with_buffer(&buf, sw, sh);

        if last_title.elapsed() > Duration::from_secs(1) {
            let c = m.board().clocks;
            let speed = (c - last_clock) as f64 / screen::CPU_HZ / last_title.elapsed().as_secs_f64();
            let b = m.board();
            let marks = b.marks.summary(b.frame_clocks());
            win.set_title(&format!(
                "{title} - {:.0}%{}{}{}",
                speed * 100.0,
                if turbo { " (turbo, F12)" } else { "" },
                if marks.is_empty() { "" } else { " - " },
                marks
            ));
            last_clock = c;
            last_title = Instant::now();
        }
    }

    {
        let b = m.board();
        if !b.marks.is_empty() {
            let r = b.marks.report(b.frame_clocks(), screen::CPU_HZ);
            print!("{r}");
            match fs::write("compass-marks.txt", &r) {
                Ok(_) => println!("saved compass-marks.txt"),
                Err(e) => eprintln!("could not save compass-marks.txt: {e}"),
            }
        }
    }
    if !readonly {
        let b = m.board();
        if let (Some(p), Some(bub)) = (&bubble, &b.bubble) {
            if bub.dirty {
                match fs::write(p, &bub.image) {
                    Ok(_) => println!("saved {}", p.display()),
                    Err(e) => eprintln!("could not save {}: {}", p.display(), e),
                }
            }
        }
        if let Some(hd) = &hostdir {
            if let Some(d) = b.gpib.drives.get(drives.len()) {
                if d.dirty {
                    match hd.export(&d.image) {
                        Ok(log) => log.iter().for_each(|l| println!("folder: {l}")),
                        Err(e) => eprintln!("could not write the folder back: {e}"),
                    }
                }
            }
        }
        for ((_, path), d) in drives.iter().zip(b.gpib.drives.iter()) {
            if d.dirty {
                match fs::write(path, &d.image) {
                    Ok(_) => println!("saved {}", path.display()),
                    Err(e) => eprintln!("could not save {}: {}", path.display(), e),
                }
            }
        }
    }
}
