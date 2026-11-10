//! gridemu 0.0.1: headless GRiD Compass runner.
//! usage: gridemu <prom_even.bin> <prom_odd.bin> <seconds> <out.png>

use std::{fs, io::BufWriter, time::Instant};

use grid_core::{gpib::{Drive, DriveKind}, keyboard::{self, Keyboard}, screen, GridMachine, GridModel};
use marty_core::cpu_common::{Cpu, Register16};

fn save_png(path: &str, pix: &[u8]) {
    let f = fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(BufWriter::new(f), screen::SCREEN_W as u32, screen::SCREEN_H as u32);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().unwrap();
    let data: Vec<u8> = pix.iter().map(|&p| if p != 0 { 0xFF } else { 0x10 }).collect();
    w.write_image_data(&data).unwrap();
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("gridemu 0.0.1\nusage: gridemu <prom_even.bin> <prom_odd.bin> <seconds> <out.png>");
        std::process::exit(1);
    }
    let even = fs::read(&a[1]).unwrap();
    let odd = fs::read(&a[2]).unwrap();
    let secs: f64 = a[3].parse().unwrap();
    let mut m = GridMachine::new(GridModel::Compass1101, &even, &odd);
    // Drives: GRID_FLOP (2102, addr 6), GRID_FLOP2101 (addr 5), GRID_HD (2101 hard disk, addr 4).
    for (var, kind) in [
        ("GRID_HD", DriveKind::Hdd2101),
        ("GRID_FLOP2101", DriveKind::Floppy2101),
        ("GRID_FLOP", DriveKind::Floppy2102),
    ] {
        if let Ok(path) = std::env::var(var) {
            let img = fs::read(&path).expect("drive image");
            println!("drive {:?} at GPIB {}: {} ({} bytes)", kind, kind.address(), path, img.len());
            m.board_mut().gpib.drives.push(Drive::new(kind, img));
        }
    }
    if let Ok(p) = std::env::var("GRID_KBD_ROM") {
        let rom = fs::read(&p).expect("keyboard ROM");
        println!("keyboard: 8741 LLE with {}", p);
        m.board_mut().keyboard = Some(Keyboard::new(rom));
    }
    // Key script: GRID_KEYS="seconds:text|seconds:text", text may hold {ret} {esc} {tab} {bs}
    // {left} {right} {up} {down} and {code+x} {ctrl+x} {shift+x}.
    let mut events: Vec<(u64, Option<(usize, u8)>, u8)> = Vec::new(); // (clock, key, mods); key None = release all
    if let Ok(script) = std::env::var("GRID_KEYS") {
        for part in script.split('|') {
            let Some((t, text)) = part.split_once(':') else { continue };
            let mut clk = (t.parse::<f64>().unwrap() * screen::CPU_HZ) as u64;
            let hold = (0.08 * screen::CPU_HZ) as u64;
            let mut chars = text.chars().peekable();
            while let Some(c) = chars.next() {
                let mut mods = 0u8;
                let mut ch = c;
                if c == '{' {
                    let mut tok = String::new();
                    while let Some(&n) = chars.peek() { chars.next(); if n == '}' { break; } tok.push(n); }
                    let (m, k) = match tok.split_once('+') { Some((m, k)) => (m.to_string(), k.to_string()), None => (String::new(), tok.clone()) };
                    mods = match m.as_str() { "code" => keyboard::MOD_CODE, "ctrl" => keyboard::MOD_CTRL, "shift" => keyboard::MOD_SHIFT, _ => 0 };
                    ch = match k.as_str() { "ret" => '\r', "esc" => '\x1b', "tab" => '\t', "bs" => '\x08', "left" => '\u{e000}', "down" => '\u{e001}', "right" => '\u{e002}', "up" => '\u{e003}', "space" => ' ', o => o.chars().next().unwrap_or('\0') };
                }
                if let Some((col, bit, shift)) = keyboard::key_for_char(ch) {
                    if shift { mods |= keyboard::MOD_SHIFT; }
                    events.push((clk, Some((col, bit)), mods));
                    events.push((clk + hold, None, 0));
                    clk += hold * 2;
                } else {
                    eprintln!("no key for {:?}", ch);
                }
            }
        }
        events.sort_by_key(|e| e.0);
    }
    let mut ev = 0usize;
    m.board_mut().gpib.trace_on = std::env::var("GRID_GPIB_TRACE").is_ok();
    let t = Instant::now();
    let total = (secs * screen::CPU_HZ) as u64;
    let mut next_report = 0u64;
    while m.board().clocks < total {
        let chunk = if ev < events.len() { 10_000 } else { 500_000 };
        m.run_clocks(chunk);
        while ev < events.len() && events[ev].0 <= m.board().clocks {
            let (_, key, mods) = events[ev];
            if let Some(k) = &mut m.board_mut().keyboard {
                match key {
                    Some((col, bit)) => { k.set_mods(mods); k.set_key(col, bit, true); }
                    None => k.release_all(),
                }
            }
            ev += 1;
        }
        if m.board().clocks >= next_report {
            let cs = m.cpu.get_register16(Register16::CS);
            let ip = m.cpu.get_ip();
            let r = |x| m.cpu.get_register16(x);
            println!("t={:.2}s CS:IP={:04X}:{:04X} DS={:04X} ES={:04X} DI={:04X} SS:SP={:04X}:{:04X} BP={:04X} instr={}", m.board().clocks as f64 / screen::CPU_HZ, cs, ip, r(Register16::DS), r(Register16::ES), r(Register16::DI), r(Register16::SS), r(Register16::SP), r(Register16::BP), m.instructions);
            next_report += 5_000_000;
        }
    }
    let el = t.elapsed().as_secs_f64();
    println!("ran {:.1} emulated s in {:.2} s ({:.1}x real time)", secs, el, secs / el);
    let b = m.board();
    println!("keyboard writes: {:?}", &b.kbd_commands.iter().take(20).collect::<Vec<_>>());
    println!("DAC writes: {}", b.dac_writes.len());
    let mut uniq: std::collections::BTreeMap<(usize, bool), usize> = Default::default();
    for l in &b.unhandled {
        *uniq.entry((l.address, l.write)).or_default() += 1;
    }
    for ((addr, w), n) in uniq.iter().take(40) {
        println!("unhandled {} {:05X} x{}", if *w { "W" } else { "R" }, addr, n);
    }
    let pic = b.pic().get_string_state();
    println!("PIC imr={} isr={} irr={}", pic.imr, pic.isr, pic.irr);
    for d in &b.gpib.drives {
        println!("drive {:?}: {} commands", d.kind, d.log.len());
        for l in d.log.iter().take(30) {
            println!("  {}", l);
        }
    }
    if b.gpib.trace_on {
        fs::write("gpib_trace.txt", b.gpib.trace.join("\n")).unwrap();
        println!("GPIB trace: {} lines in gpib_trace.txt", b.gpib.trace.len());
    }
    if let Ok(p) = std::env::var("GRID_DUMP") {
        fs::write(&p, m.ram()).unwrap();
        println!("RAM dumped to {}", p);
    }
    save_png(&a[4], &screen::render(&m.ram()));
    println!("screen saved to {}", a[4]);
}
