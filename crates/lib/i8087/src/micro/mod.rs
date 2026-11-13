//! Microcode-level 8087: runs the chip's own 1648-word microcode ROM on a model of its
//! datapath. Work in progress, built beside the instruction-level core in lib.rs and checked
//! against it; nothing uses it yet. The ROM is not part of Compass-Emu (see rom.rs).

pub mod core;
pub mod decode;
pub mod rom;
#[cfg(test)]
mod tests;

pub use self::core::{Fault, Micro8087, Reg};
pub use rom::MicroRom;
