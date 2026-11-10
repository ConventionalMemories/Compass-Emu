//! Compass-Emu 0.0.4: GRiD Compass 1101 in a window.
//!
//! compass [options]
//!   --prom-even F --prom-odd F   boot PROM chips (default roms/1101even.rev3.bin, roms/1101odd.rev3.bin)
//!   --kbd F                      8741 keyboard firmware (default roms/300067-01.bin)
//!   --hd F                       2101 hard disk image (GPIB 4)
//!   --flop F                     2102 floppy image (GPIB 6)
//!   --flop2101 F                 2101 floppy image (GPIB 5)
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
#[cfg(windows)]
mod winkeys;

use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};

const VERSION: &str = "0.0.4";
const AMBER: u32 = 0xFFB040;
const DARK: u32 = 0x1A0E00;
/// Shortest time a key stays down for the Compass (CPU clocks, 60 ms), so quick taps are
/// seen by the 8741 scan.
const MIN_HOLD: u64 = 300_000;

fn usage() -> ! {
    eprintln!(
        "Compass-Emu {VERSION}\nusage: compass [--hd disk.img] [--flop floppy.img] [--flop2101 floppy.img]\n       [--prom-even F --prom-odd F] [--kbd F] [--readonly] [--scale N]\nROM defaults are looked for in a roms folder next to the program."
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

fn save_png(path: &str, pix: &[u8]) {
    if let Ok(f) = fs::File::create(path) {
        let mut enc = png::Encoder::new(BufWriter::new(f), screen::SCREEN_W as u32, screen::SCREEN_H as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut w) = enc.write_header() {
            let data: Vec<u8> = pix.iter().map(|&p| if p != 0 { 0xFF } else { 0x00 }).collect();
            let _ = w.write_image_data(&data);
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut prom_even = default_path("1101even.rev3.bin");
    let mut prom_odd = default_path("1101odd.rev3.bin");
    let mut kbd = default_path("300067-01.bin");
    let mut drives: Vec<(DriveKind, PathBuf)> = Vec::new();
    let mut readonly = false;
    let mut scale = 4u32;
    while let Some(a) = args.next() {
        let mut val = || args.next().map(PathBuf::from).unwrap_or_else(|| usage());
        match a.as_str() {
            "--prom-even" => prom_even = val(),
            "--prom-odd" => prom_odd = val(),
            "--kbd" => kbd = val(),
            "--hd" => drives.push((DriveKind::Hdd2101, val())),
            "--flop" => drives.push((DriveKind::Floppy2102, val())),
            "--flop2101" => drives.push((DriveKind::Floppy2101, val())),
            "--readonly" => readonly = true,
            "--scale" => scale = val().to_string_lossy().parse().unwrap_or(4),
            _ => usage(),
        }
    }

    let even = read(&prom_even, "boot PROM (even)");
    let odd = read(&prom_odd, "boot PROM (odd)");
    let mut m = GridMachine::new(GridModel::Compass1101, &even, &odd);
    m.board_mut().keyboard = Some(Keyboard::new(read(&kbd, "keyboard firmware")));
    for (kind, path) in &drives {
        let img = read(path, "disk image");
        m.board_mut().gpib.drives.push(Drive::new(*kind, img));
    }

    let scale_opt = match scale {
        1 => Scale::X1,
        2 => Scale::X2,
        _ => Scale::X4,
    };
    let mut win = Window::new(
        &format!("Compass-Emu {VERSION} - GRiD Compass 1101"),
        screen::SCREEN_W,
        screen::SCREEN_H,
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

    let mut buf = vec![DARK; screen::SCREEN_W * screen::SCREEN_H];
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

        let pix = screen::render(&m.screen_ram());
        for (o, &p) in buf.iter_mut().zip(pix.iter()) {
            *o = if p != 0 { AMBER } else { DARK };
        }
        if win.is_key_pressed(Key::F11, KeyRepeat::No) {
            shot += 1;
            let name = format!("compass-{shot:03}.png");
            save_png(&name, &pix);
        }
        let _ = win.update_with_buffer(&buf, screen::SCREEN_W, screen::SCREEN_H);

        if last_title.elapsed() > Duration::from_secs(1) {
            let c = m.board().clocks;
            let speed = (c - last_clock) as f64 / screen::CPU_HZ / last_title.elapsed().as_secs_f64();
            win.set_title(&format!(
                "Compass-Emu {VERSION} - GRiD Compass 1101 - {:.0}%{}",
                speed * 100.0,
                if turbo { " (turbo, F12)" } else { "" }
            ));
            last_clock = c;
            last_title = Instant::now();
        }
    }

    if !readonly {
        let b = m.board();
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
