//! The 8087 as the CPU's coprocessor (BUSY on TEST, INT on IR6).

use std::any::Any;

use i8087::{FpuMem, I8087};
use marty_core::bus::coproc::{Coprocessor, CoprocMem};

pub struct Fpu(pub I8087);

struct Bridge<'a>(&'a mut dyn CoprocMem);

impl FpuMem for Bridge<'_> {
    fn read_u8(&mut self, a: u32) -> u8 {
        self.0.cp_read_u8(a)
    }
    fn write_u8(&mut self, a: u32, v: u8) {
        self.0.cp_write_u8(a, v)
    }
}

impl Coprocessor for Fpu {
    fn esc(&mut self, mem: &mut dyn CoprocMem, now: u64, opcode: u8, modrm: u8, ea: Option<u32>, ip: u32) -> u32 {
        self.0.esc(&mut Bridge(mem), now, opcode, modrm, ea, ip)
    }
    fn busy(&self, now: u64) -> bool {
        self.0.busy(now)
    }
    fn wait_min_cycles(&self) -> u32 {
        self.0.wait_min
    }
    fn irq(&self) -> bool {
        self.0.irq()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
