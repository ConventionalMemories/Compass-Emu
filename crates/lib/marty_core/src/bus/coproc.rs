//! Numeric coprocessor (8087) hook: the CPU hands ESC instructions to it and WAIT polls BUSY.

use std::any::Any;

/// Memory as a coprocessor sees it (linear addresses, no wait states).
pub trait CoprocMem {
    fn cp_read_u8(&mut self, addr: u32) -> u8;
    fn cp_write_u8(&mut self, addr: u32, v: u8);
}

pub trait Coprocessor: Any {
    /// The CPU executed ESC `opcode` (D8-DF) with `modrm`; `ea` is the linear address of the
    /// memory operand (None for register forms), `ip` the linear address of the instruction,
    /// `now` the CPU cycle count. Returns CPU cycles lost to the coprocessor's bus cycles.
    fn esc(&mut self, mem: &mut dyn CoprocMem, now: u64, opcode: u8, modrm: u8, ea: Option<u32>, ip: u32) -> u32;
    /// BUSY output, wired to the 8086 TEST pin.
    fn busy(&self, now: u64) -> bool;
    /// Shortest time a WAIT takes on this machine, in CPU clocks (0 = as the 8086 data sheet).
    fn wait_min_cycles(&self) -> u32 {
        0
    }
    /// Interrupt output.
    fn irq(&self) -> bool;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
