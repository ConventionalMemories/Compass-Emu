//! The classic window (minifb): just the screen, keys and the console debugger. Used with
//! --classic, or when the GUI can't start (no OpenGL 2 on the PC).

use std::time::{Duration, Instant};

use grid_core::{keyboard, screen};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};

use crate::{display, runner::Runner, session::Session, VERSION};

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

pub fn run(mut s: Session, scale: u32, overlay_on: bool) {
    let (sw, sh) = s.m.screen_size();
    let title = format!("Compass-Emu {VERSION} - GRiD Compass {}", s.cfg.model().name());
    for n in &s.notes {
        println!("{n}");
    }
    let scale_opt = match scale {
        1 => Scale::X1,
        2 => Scale::X2,
        _ => Scale::X4,
    };
    let mut win = Window::new(&title, sw, sh, WindowOptions { scale: scale_opt, resize: true, ..WindowOptions::default() })
        .unwrap_or_else(|e| {
            eprintln!("cannot open window: {e}");
            std::process::exit(1)
        });
    win.set_target_fps(60);
    #[cfg(windows)]
    {
        crate::winkeys::install(win.get_window_handle());
        crate::winkeys::clear();
    }
    let mut run = Runner::new(&s.m);
    let mut last_title = Instant::now();
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

    while win.is_open() {
        let mut mods = 0u8;
        let mut down: Vec<(usize, u8)> = Vec::new();
        let mut tapped: Vec<(usize, u8)> = Vec::new();
        #[allow(unused_mut)]
        // On Windows the keys come only from GetAsyncKeyState (winkeys): minifb's own key
        // list misses key-up messages (focus lost while a key is down, Alt+Esc sending the
        // window back, Alt combinations), so a key could stay "down" for good.
        #[cfg(not(windows))]
        {
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
        }
        #[cfg(windows)]
        crate::winkeys::poll(&mut down, &mut tapped, &mut mods);
        let tapped: Vec<((usize, u8), u8)> = tapped.into_iter().map(|p| (p, mods)).collect();
        run.set_keys(&mut s.m, &down, &tapped, mods);
        if win.is_key_pressed(Key::F12, KeyRepeat::No) {
            run.set_turbo(&s.m, !run.turbo);
        }
        if win.is_key_pressed(Key::F8, KeyRepeat::No) {
            let c = if run.paused { "g" } else { "p" };
            print!("{}", run.command(&mut s.m, c));
        }
        while let Ok(line) = dbg_rx.try_recv() {
            print!("{}", run.command(&mut s.m, &line));
        }
        if let Some(msg) = run.run_frame(&mut s.m) {
            println!("{msg}");
        }
        s.pump_io();

        let pix = s.m.render_display();
        let mut buf = display::colorize(&pix, display::AMBER, display::DARK);
        if win.is_key_pressed(Key::F10, KeyRepeat::No) {
            overlay = !overlay;
            let b = s.m.board();
            print!("{}", b.marks.report(b.frame_clocks(), screen::CPU_HZ));
        }
        if win.is_key_pressed(Key::F9, KeyRepeat::No) {
            s.m.board_mut().marks.reset();
        }
        if overlay {
            let b = s.m.board();
            display::draw_overlay(&mut buf, sw, sh, &b.marks, b.frame_clocks());
        }
        if win.is_key_pressed(Key::F11, KeyRepeat::No) {
            let name = display::next_shot_name();
            match display::save_png(&name, &pix, sw, sh) {
                Ok(_) => println!("saved {name}"),
                Err(e) => eprintln!("{e}"),
            }
        }
        let _ = win.update_with_buffer(&buf, sw, sh);

        if last_title.elapsed() > Duration::from_secs(1) {
            let b = s.m.board();
            let marks = b.marks.summary(b.frame_clocks());
            win.set_title(&format!(
                "{title} - {:.0}%{}{}{}",
                run.speed * 100.0,
                if run.turbo { " (turbo, F12)" } else { "" },
                if marks.is_empty() { "" } else { " - " },
                marks
            ));
            last_title = Instant::now();
        }
    }
    crate::finish(&mut s);
}
