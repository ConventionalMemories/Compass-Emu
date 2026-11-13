//! Fields of a micro-instruction. Bits are named A..P, A = bit 15.
//!
//! ABC selects the type:
//!   000 transfer   DEFGH source, KLMNO destination, P modifier; with J set DEFGHI is an
//!                  immediate (loop counter value or constant number)
//!   001 shift      DEFGH source, I right/left, J count from a register (KLMNO), else KLMNOP
//!   010 adder      DEFGH first input, IJK variant, L result with rounding bits, MNO operation
//!   011 control    sign latch, condition codes, tags, stack pointer, rounding bits
//!   100 far jump   EFGHI entry of the jump table, JKLMNO condition, P inverts it
//!   101 far call   same
//!   110 jump       DEFGHI signed offset from the next address, JKLMNO condition, P inverts
//!   111 misc       return (conditional), exceptions, end of instruction, nop, swap, counter

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// source register -> destination register
    Move { src: u8, dst: u8, i: bool, p: bool },
    /// 6-bit immediate -> destination (0x15 loop counter, 0x1C constant select)
    Imm { imm: u8, dst: u8, p: bool },
    Shift { src: u8, right: bool, count: u8 },
    /// shift by a count held in a register
    ShiftVar { src: u8, right: bool, by: u8, p: bool },
    Adder { src: u8, ijk: u8, round_bits: bool, op: u8, p: bool },
    Control(u16),
    FarJump { target: u8, cond: u8, inv: bool },
    Call { target: u8, cond: u8, inv: bool },
    Jump { offset: i8, cond: u8, inv: bool },
    Return { cond: u8, inv: bool },
    /// end of the instruction (read next instruction); `alt` is the FEFE form of the
    /// FSAVE/FRSTOR routines
    End { alt: bool },
    Nop,
    Exception(u8),
    Misc(u16),
}

pub fn decode(w: u16) -> Op {
    let cond = ((w >> 1) & 0x3F) as u8;
    let inv = w & 1 != 0;
    match w >> 13 {
        0 => {
            let dst = ((w >> 1) & 0x1F) as u8;
            if w & 0x40 != 0 {
                Op::Imm { imm: ((w >> 7) & 0x3F) as u8, dst, p: inv }
            }
            else {
                Op::Move { src: ((w >> 8) & 0x1F) as u8, dst, i: w & 0x80 != 0, p: inv }
            }
        }
        1 => {
            let src = ((w >> 8) & 0x1F) as u8;
            let right = w & 0x80 != 0;
            if w & 0x40 != 0 {
                Op::ShiftVar { src, right, by: ((w >> 1) & 0x1F) as u8, p: inv }
            }
            else {
                Op::Shift { src, right, count: (w & 0x3F) as u8 }
            }
        }
        2 => Op::Adder {
            src: ((w >> 8) & 0x1F) as u8,
            ijk: ((w >> 5) & 7) as u8,
            round_bits: w & 0x10 != 0,
            op: ((w >> 1) & 7) as u8,
            p: inv,
        },
        3 => Op::Control(w),
        4 => Op::FarJump { target: ((w >> 7) & 0x1F) as u8, cond, inv },
        5 => Op::Call { target: ((w >> 7) & 0x1F) as u8, cond, inv },
        6 => {
            let o = ((w >> 7) & 0x3F) as i8;
            Op::Jump { offset: if o >= 32 { o - 64 } else { o }, cond, inv }
        }
        _ => match w {
            0xFFFE => Op::End { alt: false },
            0xFEFE => Op::End { alt: true },
            0xFFF8 => Op::Nop,
            _ if w & 0xFF80 == 0xE000 => Op::Return { cond, inv },
            _ if w & 0xFFF1 == 0xF000 => Op::Exception(((w >> 1) & 7) as u8),
            _ => Op::Misc(w),
        },
    }
}

/// Addresses the far jumps and calls go to (index 0 is a three-way switch on the
/// instruction: multiply 1, add/subtract 2, divide 3).
pub const JUMP_TABLE: [u16; 22] = [
    0, 359, 232, 410, 83, 1484, 122, 173, 439, 655, 534, 299, 1572, 1446, 859, 396, 318, 380, 779, 868, 522, 801,
];

/// Microcode entry for an instruction (`op` = low 3 bits of the first byte, `modrm`).
/// None: the instruction never enters the microcode (the bus interface unit runs FINIT,
/// FCLEX, FENI, FDISI, the control/status word and environment moves) or is not an 8087
/// instruction.
pub fn entry(op: u8, modrm: u8) -> Option<u16> {
    let reg = (modrm >> 3) & 7;
    if modrm < 0xC0 {
        return match (op, reg) {
            (0 | 2 | 4 | 6, 2 | 3) => Some(808),
            (0 | 2 | 4 | 6, _) => Some(1),
            (1, 0) | (3, 0) | (5, 0) | (7, 0) => Some(65),
            (3, 5) | (7, 4) | (7, 5) => Some(65),
            (1, 2 | 3) | (3, 2 | 3) | (5, 2 | 3) | (7, 2 | 3) => Some(217),
            (3, 7) | (7, 7) => Some(217),
            (7, 6) => Some(597),
            _ => None,
        };
    }
    match op {
        0 | 4 => Some(if reg == 2 || reg == 3 { 808 } else { 1 }),
        6 => match reg {
            2 => Some(808),
            3 => (modrm == 0xD9).then_some(808),
            _ => Some(1),
        },
        1 => match modrm {
            0xC0..=0xC7 => Some(65),
            0xC8..=0xCF => Some(200),
            0xD0 => Some(217),
            0xE0 | 0xE1 => Some(892),
            0xE4 => Some(812),
            0xE5 => Some(1468),
            0xE8..=0xEB => Some(1003),
            0xEC | 0xED => Some(1008),
            0xEE => Some(1020),
            0xF0 => Some(900),
            0xF1 | 0xF9 => Some(1257),
            0xF2 => Some(1039),
            0xF3 => Some(1138),
            0xF4 => Some(1028),
            0xF6 | 0xF7 => Some(1463),
            0xF8 => Some(1586),
            0xFA => Some(1527),
            0xFC => Some(710),
            0xFD => Some(748),
            _ => None,
        },
        5 => match reg {
            0 => Some(65),
            2 | 3 => Some(217),
            _ => None,
        },
        _ => None,
    }
}
