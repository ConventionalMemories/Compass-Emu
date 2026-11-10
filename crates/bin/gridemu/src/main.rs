//! gridemu 0.0.1: headless GRiD Compass runner.
//! usage: gridemu <prom_even.bin> <prom_odd.bin> <seconds> <out.png>

use std::{fs, io::BufWriter, time::Instant};

use grid_core::{gpib::{Drive, DriveKind}, screen, GridMachine, GridModel};
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
    m.board_mut().gpib.trace_on = std::env::var("GRID_GPIB_TRACE").is_ok();
    let t = Instant::now();
    let total = (secs * screen::CPU_HZ) as u64;
    let mut next_report = 0u64;
    while m.board().clocks < total {
        m.run_clocks(500_000);
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
