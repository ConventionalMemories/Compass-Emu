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

/// Clocks by kind of micro-instruction (the order of `core::KIND_NAMES`).
/// A least-squares fit to the clocks of
/// about 1950 tests of 8087 Bench on a real Compass 1101. The values are
/// costs that add up right, not the chip's own cycle counts.
pub const KIND_CLOCKS: [f32; core::KINDS] = [1.21, 0.16, 0.93, 0.77, 1.78, 0.61, 2.18, 1.82, 2.20, 2.20, 0.71, 2.04, 2.02, 1.97, 3.76, 0.98];

/// The part of the busy time that does not depend on the micro-instructions
/// run: decode, bus cycles and the hand-over to the next instruction. Fitted
/// per instruction form to 8087 Bench on a real 1101; forms the bench has
/// fewer than three tests of share one value.
fn fixed_clocks(first: u8, modrm: u8) -> f32 {
    if modrm < 0xC0 {
        return match (first, (modrm >> 3) & 7) {
            (0xD8, 0) => -2.5,
            (0xD8, 1) => -4.2,
            (0xD8, 6) => -5.2,
            (0xD9, 0) => -4.4,
            (0xD9, 2) => -2.3,
            (0xD9, 3) => -4.2,
            (0xDB, 0) => -3.7,
            (0xDB, 2) => 0.4,
            (0xDB, 5) => -13.8,
            (0xDB, 7) => 1.2,
            (0xDC, 0) => -7.3,
            (0xDC, 1) => -7.9,
            (0xDC, 6) => -8.6,
            (0xDD, 0) => -7.4,
            (0xDD, 2) => -1.1,
            (0xDD, 3) => -0.9,
            (0xDF, 0) => -6.9,
            (0xDF, 2) => -0.5,
            (0xDF, 4) => -14.3,
            (0xDF, 5) => -10.9,
            (0xDF, 6) => 0.7,
            (0xDF, 7) => 3.0,
            _ => -5.3,
        };
    }
    let key = if matches!(first, 0xD8 | 0xDC | 0xDE) || modrm < 0xE0 { modrm & 0xF8 } else { modrm };
    match (first as u16) << 8 | key as u16 {
        0xD8C0 => 9.3,
        0xD8C8 => 9.3,
        0xD8D0 => 9.3,
        0xD8D8 => 9.7,
        0xD8E0 => 8.7,
        0xD8F0 => 9.2,
        0xD8F8 => 9.4,
        0xD9C0 => 10.9,
        0xD9D0 => 4.5,
        0xD9E1 => 8.6,
        0xD9E4 => 9.1,
        0xD9E5 => 8.7,
        0xD9E8 => 10.1,
        0xD9E9 => 8.3,
        0xD9EA => 9.8,
        0xD9EB => 8.2,
        0xD9EC => 10.1,
        0xD9ED => 9.8,
        0xD9EE => 9.9,
        0xD9F0 => 8.0,
        0xD9F1 => 10.6,
        0xD9F2 => 10.7,
        0xD9F3 => 0.9,
        0xD9F4 => 9.2,
        0xD9F8 => 8.0,
        0xD9F9 => 4.7,
        0xD9FA => 8.3,
        0xD9FC => 8.6,
        0xD9FD => 9.4,
        0xDDD8 => 6.6,
        0xDED8 => 7.0,
        _ => 8.5,
    }
}

/// The busy time of an instruction the microcode ran.
pub fn clocks(run: &core::Run, first: u8, modrm: u8) -> u32 {
    let c: f32 = run.kinds.iter().zip(KIND_CLOCKS).map(|(n, c)| *n as f32 * c).sum::<f32>() + fixed_clocks(first, modrm);
    c.round().max(2.0) as u32
}

/// Clocks to add to the instruction-level core's table time of an instruction the chip does
/// in hardwired logic (the control instructions, FSAVE and FRSTOR), from 8087 Bench group M
/// on a real 1101. `op` is the low three bits of the first byte, `reg` the middle field of
/// the second.
pub fn control_fit(op: u8, reg: u8, mem: bool) -> i32 {
    match (mem, op, reg) {
        (true, 1, 4) => -6,  // FLDENV
        (true, 1, 5) => -10, // FLDCW
        (true, 1, 6) => -3,  // FSTENV
        (true, 1, 7) => -3,  // FSTCW
        (true, 5, 4) => 7,   // FRSTOR
        (true, 5, 6) => 8,   // FSAVE
        (true, 5, 7) => -3,  // FSTSW
        _ => 0,
    }
}
