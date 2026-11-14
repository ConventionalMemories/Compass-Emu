//! Microcode-level 8087: runs the chip's own 1648-word microcode ROM on a model of its
//! datapath. Built beside the instruction-level core in lib.rs and checked against it;
//! `I8087::use_microcode` makes the emulator run on it. The ROM is not part of Compass-Emu (see rom.rs).

pub mod core;
pub mod decode;
pub mod rom;
#[cfg(test)]
mod bench;
#[cfg(test)]
mod real;
#[cfg(test)]
mod tests;

pub use self::core::{Fault, Micro8087, Reg};
pub use rom::MicroRom;

/// Whether the microcode ROM has a routine for this instruction (`op` = low three bits of
/// the opcode byte). The control instructions have none: the chip does them in hardwired
/// logic.
pub fn has_entry(op: u8, modrm: u8) -> bool {
    decode::entry(op & 7, modrm).is_some()
}
