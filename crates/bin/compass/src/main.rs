//! Compass-Emu 0.3.1: a GRiD Compass in a window.
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
//!   --bubble F                   bubble memory image (384K; a new blank one is made if F is missing)
//!   --serial S                   serial port: tcp:PORT (listen on localhost) or a port name (COM3)
//!   --mute                       no sound
//!   --readonly                   never write disk changes back to the image files
//!   --scale N                    window scale 1, 2 or 4 (default 4)
//! Keys: PC keys map to the Compass keys by position; Alt = CODE, Ctrl = CTRL.
//!   F11 = save screenshot, F12 = turbo on/off, Esc = Compass ESC. Close the window to quit
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

pub const VERSION: &str = "0.3.1";
const AMBER: u32 = 0xFFB040;
const DARK: u32 = 0x1A0E00;
/// Shortest time a key stays down for the Compass (CPU clocks, 60 ms), so quick taps are
/// seen by the 8741 scan.
const MIN_HOLD: u64 = 300_000;

fn usage() -> ! {
    eprintln!(
        "Compass-Emu {VERSION}\nusage: compass [--model 1101|1109|1121|1129|1131|1139] [--hd disk.img] [--flop floppy.img] [--flop2101 floppy.img] [--bubble bubble.img]\n       [--serial tcp:PORT|COMn] [--mute] [--prom-even F --prom-odd F | --prom F] [--kbd F] [--readonly] [--scale N]\nROM defaults are looked for in a roms folder next to the program.\n       compass do [--model M] --hd disk.img [--shot F] [--max-seconds N] \"Do build\"   (headless, see headless.rs)"
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
    let mut readonly = false;
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
            "--readonly" => readonly = true,
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

    while win.is_open() {
        // Keyboard: keys held now plus keys tapped since the last frame, each kept down at
        // least MIN_HOLD so the 8741 scan sees it.
        let now = m.board().clocks;
        let mut mods = 0u8;
        let mut down: Vec<(usize, u8)> = Vec::new();
        let mut tapped: Vec<(usize, u8)> = Vec::new();
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

        // Run the emulation: real time, or as much as fits in ~15 ms in turbo.
        if turbo {
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(15) {
                m.run_clocks(50_000);
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
                m.run_clocks(behind);
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
        if win.is_key_pressed(Key::F11, KeyRepeat::No) {
            shot += 1;
            let name = format!("compass-{shot:03}.png");
            save_png(&name, &pix, sw, sh);
        }
        let _ = win.update_with_buffer(&buf, sw, sh);

        if last_title.elapsed() > Duration::from_secs(1) {
            let c = m.board().clocks;
            let speed = (c - last_clock) as f64 / screen::CPU_HZ / last_title.elapsed().as_secs_f64();
            win.set_title(&format!(
                "{title} - {:.0}%{}",
                speed * 100.0,
                if turbo { " (turbo, F12)" } else { "" }
            ));
            last_clock = c;
            last_title = Instant::now();
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
