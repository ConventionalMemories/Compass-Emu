//! gridemu 0.0.1: headless GRiD Compass runner.
//! usage: gridemu <prom_even.bin> <prom_odd.bin> <seconds> <out.png>

use std::{fs, io::BufWriter, time::Instant};

use grid_core::{gpib::{Drive, DriveKind}, keyboard::{self, Keyboard}, screen, GridMachine, GridModel};
use marty_core::cpu_common::{Cpu, Register16};

fn save_png(path: &str, pix: &[u8], w: usize, h: usize) {
    let f = fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(BufWriter::new(f), w as u32, h as u32);
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
    // GRID_MODEL = 1101 (default), 1109, 1121, 1129, 1131, 1137, 1139.
    let model = std::env::var("GRID_MODEL").ok().and_then(|s| GridModel::parse(&s)).unwrap_or(GridModel::Compass1101);
    println!("model: Compass {}", model.name());
    let mut m = GridMachine::new(model, &even, &odd);
    // GRID_E0000_DMA=1: split board also answers the 1101 DMA window (1101 PROM stand-in).
    m.board_mut().e0000_dma = std::env::var("GRID_E0000_DMA").is_ok();
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
    if let Ok(p) = std::env::var("GRID_BUBBLE") {
        let img = fs::read(&p).expect("bubble image");
        println!("bubble: {} ({} bytes)", p, img.len());
        let mut bub = grid_core::bubble::Bubble::new(img);
        bub.log = std::env::var("GRID_BUBBLE_LOG").is_ok();
        m.board_mut().bubble = Some(bub);
    }
    // Serial: GRID_SERIAL_IN = file whose bytes are sent to the Compass (from 1 s on),
    // GRID_SERIAL_OUT = file for what the Compass sends.
    if let Ok(p) = std::env::var("GRID_SERIAL_IN") {
        let d = fs::read(&p).expect("serial input");
        m.board_mut().serial.rx_in.extend(d);
        m.board_mut().serial.set_modem_lines(true, true);
    }
    m.board_mut().serial.log = std::env::var("GRID_SERIAL_LOG").is_ok();
    let wav = std::env::var("GRID_WAV").ok();
    let mut samples: Vec<f32> = Vec::new();
    let mut serial_out: Vec<u8> = Vec::new();
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
        {
            let b = m.board_mut();
            if wav.is_some() {
                samples.extend(b.sound.out.drain(..));
            }
            else {
                b.sound.out.clear();
            }
            while let Some((_, c)) = b.serial.tx_out.pop_front() {
                serial_out.push(c);
            }
        }
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
    println!("sound samples queued: {}", b.sound.out.len());
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
    if let Some(p) = &wav {
        let rate = m.board().sound.rate;
        let mut d = Vec::new();
        let n = samples.len() as u32;
        d.extend_from_slice(b"RIFF");
        d.extend_from_slice(&(36 + n * 2).to_le_bytes());
        d.extend_from_slice(b"WAVEfmt ");
        d.extend_from_slice(&16u32.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&rate.to_le_bytes());
        d.extend_from_slice(&(rate * 2).to_le_bytes());
        d.extend_from_slice(&2u16.to_le_bytes());
        d.extend_from_slice(&16u16.to_le_bytes());
        d.extend_from_slice(b"data");
        d.extend_from_slice(&(n * 2).to_le_bytes());
        for s in &samples {
            d.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32000.0) as i16).to_le_bytes());
        }
        fs::write(p, d).unwrap();
        println!("sound: {} samples to {}", n, p);
    }
    if let Ok(p) = std::env::var("GRID_SERIAL_OUT") {
        fs::write(&p, &serial_out).unwrap();
        println!("serial: {} bytes sent by the Compass, in {}", serial_out.len(), p);
    }
    if let Some(bub) = &m.board().bubble {
        if bub.dirty {
            if let Ok(p) = std::env::var("GRID_BUBBLE_SAVE") {
                fs::write(&p, &bub.image).unwrap();
                println!("bubble written to {}", p);
            }
            else {
                println!("bubble changed (GRID_BUBBLE_SAVE=file to keep it)");
            }
        }
    }
    let (w, h) = m.screen_size();
    println!("screen mode {:?}, split regs {:02X?}", m.screen_mode(), m.board().split_regs);
    save_png(&a[4], &m.render_now(), w, h);
    println!("screen saved to {}", a[4]);
}
