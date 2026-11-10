//! Run a raw 8086/8087 code fragment on an otherwise idle Compass and count its clocks
//! (what gridfrag does, for the Claude Code hook). The fragment is loaded at 1000:0000 with
//! DS = ES = 2000h, SS:SP = 3000:FFFE, interrupts off, and runs until it reaches a HLT (not
//! executed). The display (contention, beam) runs as normal; no PROM code runs.

use marty_core::cpu_common::{Cpu, CpuAddress, Register16};

use crate::{GridMachine, GridModel};

pub struct FragmentResult {
    pub clocks: u64,
    pub instructions: u64,
    /// Stopped at the clock limit instead of a HLT.
    pub timed_out: bool,
    /// AX BX CX DX SI DI BP SP DS ES SS CS IP FLAGS at the end.
    pub registers: String,
    pub machine: GridMachine,
}

pub fn run(model: GridModel, code: &[u8], max_clocks: u64) -> FragmentResult {
    let prom = vec![0xF4u8; 8192];
    let mut m = GridMachine::new(model, &prom, &prom);
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
    let mut instructions = 0u64;
    let mut timed_out = false;
    loop {
        let lin = ((m.cpu.get_register16(Register16::CS) as usize) << 4) + m.cpu.get_ip() as usize;
        if m.cpu.bus().peek_u8(lin & 0xFFFFF).unwrap_or(0) == 0xF4 {
            break;
        }
        if m.board().clocks - start > max_clocks {
            timed_out = true;
            break;
        }
        m.step();
        instructions += 1;
    }
    let r = |x| m.cpu.get_register16(x);
    use Register16::*;
    let registers = format!(
        "AX={:04X} BX={:04X} CX={:04X} DX={:04X} SI={:04X} DI={:04X} BP={:04X} SP={:04X} DS={:04X} ES={:04X} SS={:04X} CS={:04X} IP={:04X} FLAGS={:04X}",
        r(AX), r(BX), r(CX), r(DX), r(SI), r(DI), r(BP), r(SP), r(DS), r(ES), r(SS), r(CS), m.cpu.get_ip(), m.cpu.get_flags()
    );
    FragmentResult { clocks: m.board().clocks - start, instructions, timed_out, registers, machine: m }
}
