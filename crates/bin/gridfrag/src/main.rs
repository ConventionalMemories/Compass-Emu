//! gridfrag 0.0.1: run a raw 8086/8087 code fragment on the emulated Compass 1101 and count
//! CPU clocks. The fragment is loaded at 1000:0000 with DS=ES=2000h, SS:SP=3000:FFFE, IF=0, and
//! runs until it reaches a HLT (not executed). The board (display timing, devices) runs as
//! normal, but no PROM code runs.
//!   gridfrag code.bin [max-clocks]
//! Prints clocks used, instructions, and the 8087 state. Environment: FPU_EXTRA, FPU_WAITMIN
//! (8087 timing), DISP_CONTENTION (0 = off), DISP_FETCH_LEN, DISP_FETCH_PERIOD,
//! FRAG_PNG=file saves the last displayed frame, FRAG_DUMP=addr,len (hex) prints memory. FRAG_MODEL=1139 etc. picks the model,
//! RAM_EXTRA_WAIT the extra wait states on every RAM access. DISP_FETCH_OFFSET, DISP_CPU_LEN, DISP_CPU_WRITE_LEN (display contention, 15 MHz states),
//! DISP_LINE_CLOCKS, DISP_FRAME_LINES (geometry), DISP_ALL_LINES (0 = contention on visible lines only); JMP_EXTRA, GRID_REFRESH, GRID_REFRESH_WAIT, GRID_TRACE (see grid_core).

use grid_core::{GridMachine, GridModel};
use marty_core::cpu_common::{Cpu, CpuAddress, Register16};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 2 {
        eprintln!("gridfrag 0.0.1\nusage: gridfrag code.bin [max-clocks]");
        std::process::exit(1);
    }
    let code = std::fs::read(&a[1]).expect("code file");
    let max: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(100_000_000);
    // A PROM of HLTs: never executed, the reset vector points at the fragment.
    let prom = vec![0xF4u8; 8192];
    let model = std::env::var("FRAG_MODEL").ok().and_then(|s| GridModel::parse(&s)).unwrap_or(GridModel::Compass1101);
    let mut m = GridMachine::new(model, &prom, &prom);
    if let Some(v) = std::env::var("RAM_EXTRA_WAIT").ok().and_then(|v| v.parse().ok()) {
        m.board_mut().ram_extra_wait = v;
    }
    if let Some(f) = m.fpu_mut() {
        if let Ok(v) = std::env::var("FPU_EXTRA") {
            f.extra_busy = v.parse().unwrap();
        }
        if let Ok(v) = std::env::var("FPU_WAITMIN") {
            f.wait_min = v.parse().unwrap();
        }
    }
    {
        let d = &mut m.board_mut().display;
        let env = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<u32>().ok());
        if let Some(v) = env("DISP_CONTENTION") {
            d.contention = v != 0;
        }
        if let Some(v) = env("DISP_FETCH_LEN") {
            d.fetch_len = v;
        }
        if let Some(v) = env("DISP_FETCH_PERIOD") {
            d.fetch_period = v;
        }
        if let Some(v) = env("DISP_FETCH_OFFSET") {
            d.fetch_offset = v;
        }
        if let Some(v) = env("DISP_CPU_LEN") {
            d.cpu_len = v;
            d.cpu_write_len = v;
        }
        if let Some(v) = env("DISP_CPU_WRITE_LEN") {
            d.cpu_write_len = v;
        }
        if let Some(v) = env("DISP_ALL_LINES") {
            d.all_lines = v != 0;
        }
        if let Some(v) = env("DISP_LINE_CLOCKS") {
            d.line_clocks = v as u64;
        }
        if let Some(v) = env("DISP_FRAME_LINES") {
            d.frame_lines = v as u64;
        }
    }
    for (i, b) in code.iter().enumerate() {
        let _ = m.cpu.bus_mut().write_u8(0x10000 + i, *b, 0);
    }
    m.cpu.set_reset_vector(CpuAddress::Segmented(0x1000, 0));
    m.cpu.reset();
    m.cpu.set_register16(Register16::DS, 0x2000);
    m.cpu.set_register16(Register16::ES, 0x2000);
    m.cpu.set_register16(Register16::SS, 0x3000);
    m.cpu.set_register16(Register16::SP, 0xFFFE);
    let start = m.board().clocks;
    let mut instr = 0u64;
    loop {
        let cs = m.cpu.get_register16(Register16::CS);
        let ip = m.cpu.get_ip();
        let lin = ((cs as usize) << 4) + ip as usize;
        let op = m.cpu.bus().peek_u8(lin).unwrap_or(0);
        if op == 0xF4 || m.board().clocks - start > max {
            break;
        }
        m.step();
        instr += 1;
    }
    let clocks = m.board().clocks - start;
    if let Ok(path) = std::env::var("FRAG_PNG") {
        // The last complete frame as the beam showed it.
        let pix = m.render_display();
        let (w, h) = m.screen_size();
        let f = std::fs::File::create(&path).expect("png file");
        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        let data: Vec<u8> = pix.iter().map(|&p| if p != 0 { 0xFF } else { 0x10 }).collect();
        enc.write_header().unwrap().write_image_data(&data).unwrap();
    }
    println!("clocks {}", clocks);
    println!("instructions {}", instr);
    println!("frames {}", m.frames);
    {
        let b = m.board();
        let tx: Vec<u8> = b.serial.tx_out.iter().map(|x| x.1).collect();
        println!("sound dac {} volume {} port_b {:02X} samples {}", b.sound.dac, b.sound.volume, b.sound.port_b, b.sound.out.len());
        println!("serial tx {:?} rate {:.0}", String::from_utf8_lossy(&tx), b.serial.bit_rate());
    }
    // FRAG_DUMP=address,length (hex) prints that memory, 16 bytes a line.
    if let Some((a, n)) = std::env::var("FRAG_DUMP").ok().and_then(|v| {
        let (a, n) = v.split_once(',')?;
        Some((usize::from_str_radix(a, 16).ok()?, usize::from_str_radix(n, 16).ok()?))
    }) {
        let mem = m.ram();
        for l in (a..a + n).step_by(16) {
            let row: Vec<String> = (l..(l + 16).min(a + n)).map(|i| format!("{:02X}", mem.get(i).copied().unwrap_or(0xFF))).collect();
            println!("dump {l:05X}: {}", row.join(" "));
        }
    }
    if let Some(f) = m.fpu() {
        println!("fpu sw {:04X} cw {:04X} tw {:04X} busy_until {}", f.sw, f.cw, f.tw, f.busy_until());
        for i in 0..8 {
            if let (Some(v), Some(b)) = (f.st_f64(i), f.st_bits(i)) {
                println!("st{} {:020X} {}", i, b, v);
            }
        }
    }
}
