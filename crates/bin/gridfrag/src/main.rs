//! gridfrag 0.0.1: run a raw 8086/8087 code fragment on the emulated Compass 1101 and count
//! CPU clocks. The fragment is loaded at 1000:0000 with DS=ES=2000h, SS:SP=3000:FFFE, IF=0, and
//! runs until it reaches a HLT (not executed). The board (display timing, devices) runs as
//! normal, but no PROM code runs.
//!   gridfrag code.bin [max-clocks]
//! Prints clocks used, instructions, and the 8087 state.

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
    let mut m = GridMachine::new(GridModel::Compass1101, &prom, &prom);
    if let Some(f) = m.fpu_mut() {
        if let Ok(v) = std::env::var("FPU_EXTRA") {
            f.extra_busy = v.parse().unwrap();
        }
        if let Ok(v) = std::env::var("FPU_WAITMIN") {
            f.wait_min = v.parse().unwrap();
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
    println!("clocks {}", clocks);
    println!("instructions {}", instr);
    if let Some(f) = m.fpu() {
        println!("fpu sw {:04X} cw {:04X} tw {:04X} busy_until {}", f.sw, f.cw, f.tw, f.busy_until());
        for i in 0..8 {
            if let (Some(v), Some(b)) = (f.st_f64(i), f.st_bits(i)) {
                println!("st{} {:020X} {}", i, b, v);
            }
        }
    }
}
