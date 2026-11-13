//! The 8087 as its microcode sees it: registers, adder, shifter, sequencer.
//!
//! Sources: Ken Shirriff's 8087 articles (righto.com, 2018-2026) and the microcode dump by
//! the "Opcode Collective". Where neither says what a micro-instruction does, the meaning
//! here was worked out by running the routine against the IEEE result (see tests.rs); such
//! places are marked "inferred".
//!
//! A micro-instruction this model does not know yet stops the instruction with a `Fault`
//! that names the address, so the gaps show up one at a time instead of as wrong results.

use super::decode::{decode, entry, Op, JUMP_TABLE};
use super::rom::MicroRom;

pub const TAG_VALID: u8 = 0;
pub const TAG_ZERO: u8 = 1;
pub const TAG_SPECIAL: u8 = 2;
pub const TAG_EMPTY: u8 = 3;

// status word bits
pub const SW_IE: u16 = 0x0001;
pub const SW_DE: u16 = 0x0002;
pub const SW_ZE: u16 = 0x0004;
pub const SW_OE: u16 = 0x0008;
pub const SW_UE: u16 = 0x0010;
pub const SW_PE: u16 = 0x0020;
pub const SW_C0: u16 = 0x0100;
pub const SW_C1: u16 = 0x0200;
pub const SW_C2: u16 = 0x0400;
pub const SW_C3: u16 = 0x4000;

/// A stack register or tmpA/tmpB: sign, 16-bit exponent register, 64-bit fraction, tag.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct Reg {
    pub sign: bool,
    pub exp: u16,
    pub frac: u64,
    pub tag: u8,
}

impl Reg {
    pub fn from_bits(bits: u128) -> Reg {
        let mut r = Reg { sign: bits >> 79 & 1 != 0, exp: (bits >> 64) as u16 & 0x7FFF, frac: bits as u64, tag: 0 };
        r.tag = r.class();
        r
    }

    pub fn bits(&self) -> u128 {
        ((self.sign as u128) << 79) | (((self.exp & 0x7FFF) as u128) << 64) | self.frac as u128
    }

    /// The tag the value itself calls for.
    pub fn class(&self) -> u8 {
        let e = self.exp & 0x7FFF;
        if e == 0 && self.frac == 0 {
            TAG_ZERO
        }
        else if e == 0 || e == 0x7FFF {
            TAG_SPECIAL
        }
        else {
            TAG_VALID
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub addr: u16,
    pub word: u16,
    pub what: String,
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "8087 microcode #{:04} ({:04X}): {}", self.addr, self.word, self.what)
    }
}

/// What one instruction did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Run {
    /// micro-instructions executed
    pub steps: u32,
}

// Positions in the 68-bit adder / sum register value: three rounding bits (guard, round,
// sticky) below the 64-bit fraction, and one bit above it.
const RND: u32 = 3;
const FRAC_MASK: u128 = (u64::MAX as u128) << RND;
const BIT64: u128 = 1 << (64 + RND);
const WIDE_MASK: u128 = (BIT64 << 1) - 1;

/// What a source puts on the buses.
#[derive(Copy, Clone)]
struct Val {
    /// fraction bus with rounding bits and bit 64 (layout above)
    wide: u128,
    sign: bool,
    exp: u16,
    tag: u8,
    /// the source is a whole register (sign, exponent, fraction and tag)
    whole: bool,
}

impl Val {
    fn frac(v: u64) -> Val {
        Val { wide: (v as u128) << RND, sign: false, exp: v as u16, tag: TAG_VALID, whole: false }
    }
    fn exp(v: u16) -> Val {
        Val::frac(v as u64)
    }
    fn wide(w: u128) -> Val {
        Val { wide: w & WIDE_MASK, sign: false, exp: (w >> RND) as u16, tag: TAG_VALID, whole: false }
    }
    fn reg(r: Reg) -> Val {
        Val { wide: (r.frac as u128) << RND, sign: r.sign, exp: r.exp, tag: r.tag, whole: true }
    }
    fn f64(&self) -> u64 {
        (self.wide >> RND) as u64
    }
}

pub struct Micro8087 {
    rom: MicroRom,
    pub st: [Reg; 8],
    pub top: u8,
    pub cw: u16,
    /// Status word without the stack-top field (that is `top`).
    pub sw: u16,

    // --- state inside one instruction ---
    opcode: u16,
    /// stack top when the instruction started: ST(i) is addressed from it (inferred: FLD
    /// ST(i) pushes before it reads ST(i))
    top0: u8,
    tmp: [Reg; 2],
    /// tmpA and tmpB exchanged (a flip-flop renames them)
    swap: bool,
    tmpc: u128,
    breg: u128,
    sum: u128,
    carry: bool,
    shl: u128,
    shr: u128,
    shift_count: u16,
    /// rounding bits made by the last complement shift
    round_h: u8,
    exp_conv: u16,
    counter: u8,
    /// quotient / multiplier / radicand register (65 bits used by the square root)
    quotient: u128,
    exp_adj: u8,
    sign_latch: bool,
    flag: bool,
    const_sel: u8,
    /// exception flip-flops (status word bit positions), copied to the status word at the end
    exc: u16,
    /// the register a push is about to overwrite was not empty
    overflow: bool,
    stack: Vec<u16>,
    /// memory operand as the bus interface delivers it: 16-bit words, lowest first
    bus_in: [u16; 5],
    bus_next: usize,
    bus_out: Vec<u16>,
    /// what the exponent converter was last given, and how it reads it
    conv_fmt: Fmt,
    /// what the adder's loop hardware is armed for
    loop_mode: u8,
    loop_grs: u8,
    loop_steps: u32,
    pub trace: bool,
}

const LOOP_MUL: u8 = 1;
const LOOP_DIV: u8 = 2;
const LOOP_SQRT: u8 = 3;
const LOOP_REM: u8 = 4;

/// How the exponent converter treats the value it holds.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Fmt {
    /// a 15-bit temporary real exponent
    X80,
    /// an exponent field read from a short (8-bit) or long (11-bit) real
    Load(u16, u16),
    /// a temporary real exponent (plus one) on its way to a short or long real
    Store(u16, u16),
    /// a temporary's sign and exponent as one 16-bit value
    Raw,
}

impl Micro8087 {
    pub fn new(rom: MicroRom) -> Micro8087 {
        Micro8087 {
            rom,
            st: [Reg { tag: TAG_EMPTY, ..Reg::default() }; 8],
            top: 0,
            cw: 0x03FF,
            sw: 0,
            opcode: 0,
            top0: 0,
            tmp: [Reg::default(); 2],
            swap: false,
            tmpc: 0,
            breg: 0,
            sum: 0,
            carry: false,
            shl: 0,
            shr: 0,
            shift_count: 0,
            round_h: 0,
            exp_conv: 0,
            counter: 0,
            quotient: 0,
            exp_adj: 1,
            sign_latch: false,
            flag: false,
            const_sel: 0,
            exc: 0,
            overflow: false,
            stack: Vec::new(),
            bus_in: [0; 5],
            bus_next: 0,
            bus_out: Vec::new(),
            conv_fmt: Fmt::X80,
            loop_mode: 0,
            loop_grs: 0,
            loop_steps: 0,
            trace: false,
        }
    }

    pub fn tag_word(&self) -> u16 {
        let mut t = 0;
        for i in 0..8 {
            let tag = match self.st[i].tag {
                TAG_VALID => 0,
                TAG_ZERO => 1,
                TAG_SPECIAL => 2,
                _ => 3,
            };
            t |= tag << (2 * i);
        }
        t
    }

    pub fn status_word(&self) -> u16 {
        let mut s = (self.sw & !0x3800) | ((self.top as u16) << 11);
        if s & !self.cw & 0x3F != 0 {
            s |= 0x8080;
        }
        s
    }

    fn a(&mut self) -> &mut Reg {
        &mut self.tmp[self.swap as usize]
    }
    fn b(&mut self) -> &mut Reg {
        &mut self.tmp[!self.swap as usize]
    }
    fn ra(&self) -> Reg {
        self.tmp[self.swap as usize]
    }
    fn rb(&self) -> Reg {
        self.tmp[!self.swap as usize]
    }

    fn sti(&self) -> usize {
        ((self.top0 as u16 + (self.opcode & 7)) & 7) as usize
    }

    fn modrm(&self) -> u8 {
        self.opcode as u8
    }
    fn op(&self) -> u8 {
        (self.opcode >> 8) as u8
    }
    fn reg_form(&self) -> bool {
        self.modrm() >= 0xC0
    }
    /// Size of the memory operand in 16-bit words (0 for register forms).
    fn operand_words(&self) -> u8 {
        if self.reg_form() {
            return 0;
        }
        let reg = (self.modrm() >> 3) & 7;
        match self.op() {
            0 | 1 | 2 => 2,
            3 => if reg >= 4 { 5 } else { 2 },
            4 | 5 => 4,
            6 => 1,
            _ => match reg {
                4 | 6 => 5,
                5 | 7 => 4,
                _ => 1,
            },
        }
    }

    /// Short or long real memory operand: (exponent bias difference, largest field value).
    fn real_format(&self) -> Option<(u16, u16)> {
        if self.reg_form() {
            return None;
        }
        match self.op() {
            0 | 1 => Some((0x3FFF - 0x7F, 0xFF)),
            4 | 5 => Some((0x3FFF - 0x3FF, 0x7FF)),
            _ => None,
        }
    }

    /// Exponent field a store would write, and the first field value that overflows.
    fn conv_field(&self) -> i32 {
        match self.conv_fmt {
            Fmt::Store(diff, _) => self.exp_conv as i16 as i32 - diff as i32,
            _ => self.exp_conv as i32,
        }
    }
    fn conv_max(&self) -> i32 {
        match self.conv_fmt {
            Fmt::Store(_, max) | Fmt::Load(_, max) => max as i32,
            Fmt::X80 | Fmt::Raw => 0x7FFF,
        }
    }

    /// The instruction stores to memory (FST/FSTP/FIST/FISTP/FBSTP).
    fn store_class(&self) -> bool {
        !self.reg_form() && !self.arith_class() && matches!((self.modrm() >> 3) & 7, 2 | 3 | 6 | 7)
    }

    /// Bits of precision results are rounded to: the destination's for a store, else the
    /// control word's.
    fn precision_bits(&self) -> u32 {
        if self.store_class() {
            return match self.op() {
                1 => 24,
                5 => 53,
                _ => 64,
            };
        }
        match (self.cw >> 8) & 3 {
            0 => 24,
            2 => 53,
            _ => 64,
        }
    }

    fn arith_class(&self) -> bool {
        self.op() & 1 == 0
    }

    /// Register form of an instruction.
    pub fn exec(&mut self, op: u8, modrm: u8) -> Result<Run, Fault> {
        self.exec_mem(op, modrm, None)
    }

    /// Runs one instruction (`op` = low three bits of the first byte) from decode to the
    /// end-of-instruction word. `mem` is the memory operand's address for memory forms.
    pub fn exec_mem(&mut self, op: u8, modrm: u8, mem: Option<(&mut dyn crate::FpuMem, u32)>) -> Result<Run, Fault> {
        let mut mem = mem;
        self.bus_next = 0;
        self.bus_out.clear();
        self.conv_fmt = Fmt::X80;
        self.loop_mode = 0;
        if let Some((m, a)) = mem.as_mut() {
            for i in 0..5 {
                let lo = m.read_u8(*a + 2 * i as u32) as u16;
                let hi = m.read_u8(*a + 2 * i as u32 + 1) as u16;
                self.bus_in[i] = lo | hi << 8;
            }
        }
        let r = self.run(op, modrm);
        if let Some((m, a)) = mem.as_mut() {
            for (i, w) in self.bus_out.iter().enumerate() {
                m.write_u8(*a + 2 * i as u32, *w as u8);
                m.write_u8(*a + 2 * i as u32 + 1, (*w >> 8) as u8);
            }
        }
        r
    }

    fn run(&mut self, op: u8, modrm: u8) -> Result<Run, Fault> {
        let start = entry(op & 7, modrm).ok_or(Fault { addr: 0, word: 0, what: format!("no microcode entry for {:02X} {:02X}", 0xD8 | op, modrm) })?;
        self.opcode = ((op as u16 & 7) << 8) | modrm as u16;
        self.top0 = self.top;
        self.swap = false;
        self.tmp = [Reg::default(); 2];
        self.exp_adj = 1;
        self.exc = 0;
        self.overflow = false;
        self.flag = false;
        self.stack.clear();
        let mut pc = start;
        let mut run = Run::default();
        loop {
            let w = self.rom.0[pc as usize];
            let here = pc;
            pc += 1;
            run.steps += 1;
            if run.steps > 20_000 {
                return Err(Fault { addr: here, word: w, what: "more than 20000 micro-instructions".into() });
            }
            let o = decode(w);
            if self.trace {
                eprintln!("#{:04} {:04X} {:?}", here, w, o);
            }
            let fault = |what: String| Fault { addr: here, word: w, what };
            match o {
                Op::End { .. } => break,
                Op::Nop => {}
                Op::Jump { offset, cond, inv } => {
                    if self.cond(cond).map_err(&fault)? != inv {
                        pc = (pc as i32 + offset as i32) as u16;
                    }
                }
                Op::FarJump { target, cond, inv } => {
                    if self.cond(cond).map_err(&fault)? != inv {
                        pc = self.far(target).map_err(&fault)?;
                    }
                }
                Op::Call { target, cond, inv } => {
                    if self.cond(cond).map_err(&fault)? != inv {
                        if self.stack.len() == 6 {
                            return Err(fault("call deeper than the six-level micro-stack".into()));
                        }
                        self.stack.push(pc);
                        pc = self.far(target).map_err(&fault)?;
                    }
                }
                Op::Return { cond, inv } => {
                    if self.cond(cond).map_err(&fault)? != inv {
                        pc = self.stack.pop().ok_or_else(|| fault("return with an empty micro-stack".into()))?;
                    }
                }
                Op::Exception(n) => match n {
                    0 => self.exc |= SW_DE,
                    1 => self.exc |= SW_IE,
                    2 => self.exc |= SW_PE,
                    3 => self.exc |= SW_ZE,
                    4 => self.flag = true,
                    _ => return Err(fault(format!("exception/flag word {}", n))),
                },
                Op::Misc(m) => self.misc(m).map_err(&fault)?,
                Op::Control(c) => self.control(c).map_err(&fault)?,
                Op::Move { src, dst, i, p } => {
                    if i {
                        return Err(fault("transfer with I set".into()));
                    }
                    let mut v = self.read(src).map_err(&fault)?;
                    if p {
                        v.wide |= 1u128 << (63 + RND);
                    }
                    self.write(dst, v, src).map_err(&fault)?;
                }
                Op::Imm { imm, dst, .. } => match dst {
                    0x15 => self.counter = imm,
                    0x1C => self.const_sel = imm,
                    _ => return Err(fault(format!("immediate to register {:02X}", dst))),
                },
                Op::Shift { src, right, count } => {
                    let v = self.read(src).map_err(&fault)?;
                    self.shift(v, right, count as u32);
                }
                Op::ShiftVar { src, right, by, p } => {
                    if p {
                        return Err(fault("variable shift with P".into()));
                    }
                    let v = self.read(src).map_err(&fault)?;
                    self.shift_var(v, right, by).map_err(&fault)?;
                }
                Op::Adder { src, ijk, round_bits, op, p } => {
                    if ijk != 0 {
                        self.hw_loop(ijk, op).map_err(&fault)?;
                        continue;
                    }
                    if p {
                        return Err(fault("adder variant".into()));
                    }
                    let v = self.read(src).map_err(&fault)?;
                    self.adder(v, round_bits, op).map_err(&fault)?;
                }
            }
        }
        // end of instruction: the exception flip-flops go to the status word
        self.sw |= self.exc;
        Ok(run)
    }

    fn far(&self, target: u8) -> Result<u16, String> {
        if target == 0 {
            if !self.arith_class() {
                return Err("instruction switch outside the arithmetic instructions".into());
            }
            return Ok(match (self.modrm() >> 3) & 7 {
                1 => JUMP_TABLE[1],
                6 | 7 => JUMP_TABLE[3],
                _ => JUMP_TABLE[2],
            });
        }
        JUMP_TABLE.get(target as usize).copied().ok_or_else(|| format!("jump table entry {}", target))
    }

    fn unmasked(&self) -> bool {
        self.exc & !self.cw & 0x3F != 0
    }

    fn cond(&mut self, c: u8) -> Result<bool, String> {
        let a = self.ra();
        let b = self.rb();
        let reg = (self.modrm() >> 3) & 7;
        Ok(match c {
            0x00 => !self.reg_form(),
            0x01 => self.op() & 4 != 0 && self.reg_form(),
            0x02 => self.op() & 4 == 0 && self.reg_form(),
            // pop at the end (inferred from what each instruction class must do)
            0x06 => {
                if self.arith_class() {
                    self.op() == 6 && self.reg_form()
                }
                else if self.op() == 7 && ((self.reg_form() && self.modrm() & 0x20 == 0) || (!self.reg_form() && reg == 6)) {
                    true // (FBSTP too)
                }
                else {
                    self.modrm() & 0x08 != 0
                }
            }
            0x03 => self.op() & 2 == 0,
            // operand sizes: 04 two words (short real, short integer), 0E one word, 05 not
            // five words, 07 temporary real, 08 packed decimal
            0x04 => self.operand_words() == 2,
            0x05 => self.operand_words() != 5,
            0x0E => self.operand_words() == 1,
            0x07 => self.op() == 3 && !self.reg_form() && (reg == 5 || reg == 7),
            0x0A => self.modrm() & 1 != 0,
            0x0B => self.modrm() & 8 != 0,
            0x0C => self.arith_class() && reg == 1,
            0x0D => self.arith_class() && reg >= 6,
            0x0F => (self.arith_class() && (reg == 2 || reg == 3)) || self.opcode == 0x1E4,
            0x08 => self.op() == 7 && !self.reg_form() && (reg == 4 || reg == 6),
            // the exponent converter's view of the last value sent to it: a normal
            // exponent, bit 0, small enough for a shift count (bits 6-14 clear), all ones,
            // overflow
            0x18 => match self.conv_fmt {
                Fmt::X80 => (1..=0x7FFE).contains(&self.exp_conv),
                Fmt::Raw => (1..=0x7FFE).contains(&(self.exp_conv & 0x7FFF)),
                Fmt::Load(_, max) => (1..max).contains(&self.exp_conv),
                Fmt::Store(..) => (1..self.conv_max()).contains(&self.conv_field()),
            },
            0x19 => self.exp_conv & 1 != 0,
            0x1A => self.exp_conv & 0x7FC0 == 0,
            0x1B => match self.conv_fmt {
                Fmt::Load(_, max) => self.exp_conv == max,
                _ => self.exp_conv & 0x7FFF == 0x7FFF,
            },
            0x1C => self.precision_bits() == 64,
            0x1D => self.round_up(),
            0x1F => match self.conv_fmt {
                Fmt::Store(..) => self.conv_field() >= self.conv_max() && self.exp_conv < 0xC000,
                Fmt::Raw => self.exp_conv & 0x8000 != 0,
                _ => (0x7FFF..0xC000).contains(&self.exp_conv),
            },
            0x1E => self.sum & FRAC_MASK == 0,
            0x20 => a.tag == TAG_ZERO,
            0x21 => a.tag == TAG_SPECIAL,
            0x22 => a.tag == TAG_EMPTY,
            // tmpA or tmpB anomalous: empty or special, a push onto a full register
            0x23 => {
                self.overflow
                    || matches!(a.tag, TAG_EMPTY | TAG_SPECIAL)
                    || matches!(b.tag, TAG_EMPTY | TAG_SPECIAL)
                    || (self.arith_class() && reg >= 6 && (a.tag == TAG_ZERO || b.tag == TAG_ZERO))
            }
            0x24 => b.tag == TAG_ZERO,
            0x25 => b.tag == TAG_SPECIAL,
            0x26 => b.tag == TAG_EMPTY,
            0x27 => self.overflow || a.tag == TAG_EMPTY || b.tag == TAG_EMPTY,
            0x28 => a.sign,
            0x29 => b.frac >> 63 != 0,
            0x2B => a.frac >> 63 != 0,
            // the low 40 bits of tmpA's fraction are zero: a short multiplier
            0x2A => a.frac & 0xFF_FFFF_FFFF == 0,
            0x2C => self.cw & 0x1000 != 0,
            // "round up/down": a directed rounding mode that rounds this sign toward zero
            // (down for +, up for -). Inferred: with chop left out, an exact zero difference
            // is -0 only when rounding down, as Intel documents; it also means a masked
            // overflow gives infinity when chopping. To be checked on a real chip.
            0x2D => match (self.cw >> 10) & 3 {
                1 => !a.sign,
                2 => a.sign,
                _ => false,
            },
            0x2E => self.unmasked(),
            0x2F => self.exc & !self.cw & SW_DE != 0,
            0x30 => self.carry,
            0x32 => self.sum & BIT64 != 0,
            0x33 => self.sum >> (63 + RND) & 1 != 0,
            // the loop counter has run out; testing it counts it down otherwise
            0x38 => {
                let done = self.counter == 0;
                if !done {
                    self.counter -= 1;
                }
                done
            }
            0x39 => a.sign ^ b.sign ^ (self.arith_class() && (reg == 4 || reg == 5)),
            0x3A => self.flag,
            0x3B => a.sign != b.sign,
            0x3E => true,
            _ => return Err(format!("condition {:02X}", c)),
        })
    }

    /// Condition 1D: the rounding logic says "add one". It looks at the sum register's
    /// rounding bits and lowest fraction bit, the rounding mode and tmpA's sign; as side
    /// effects C1 gets the answer and nonzero rounding bits set the precision exception
    /// (the second is inferred).
    fn round_up(&mut self) -> bool {
        let grs = (self.sum & 7) as u8;
        let lsb = self.sum >> RND & 1 != 0;
        let sign = self.ra().sign;
        let up = match (self.cw >> 10) & 3 {
            0 => grs > 4 || (grs == 4 && lsb),
            1 => sign && grs != 0,
            2 => !sign && grs != 0,
            _ => false,
        };
        if grs != 0 {
            self.exc |= SW_PE;
        }
        if up {
            self.sw |= SW_C1
        }
        else {
            self.sw &= !SW_C1
        }
        up
    }

    fn set_cc(&mut self, c3: bool, c0: bool) {
        self.sw &= !(SW_C3 | SW_C0);
        if c3 {
            self.sw |= SW_C3;
        }
        if c0 {
            self.sw |= SW_C0;
        }
    }

    fn control(&mut self, w: u16) -> Result<(), String> {
        match w {
            0x7800 => self.top = (self.top + 1) & 7,
            0x7802 => self.top = (self.top + 7) & 7,
            0x7C00 => self.exp_adj = (self.exp_adj + 1) & 3,
            0x7C02 => self.exp_adj = (self.exp_adj + 3) & 3,
            0x6340 => self.st[self.top as usize].tag = TAG_EMPTY,
            0x6342 => {
                let i = self.sti();
                self.st[i].tag = TAG_EMPTY
            }
            // the tag of the new top after a push: not empty = stack overflow (inferred)
            0x600A => self.overflow = self.st[self.top as usize].tag != TAG_EMPTY,
            // tmpA's tag to C3/C0 for FXAM (inferred): zero C3, special C0, empty both
            0x6008 => {
                let t = self.ra().tag;
                self.set_cc(t == TAG_ZERO || t == TAG_EMPTY, t == TAG_SPECIAL || t == TAG_EMPTY)
            }
            0x6B02 => {
                if self.ra().sign {
                    self.sw |= SW_C1
                }
                else {
                    self.sw &= !SW_C1
                }
            }
            0x6044 => self.a().tag = TAG_VALID,
            0x6144 => self.a().tag = TAG_ZERO,
            0x6244 => self.a().tag = TAG_SPECIAL,
            0x604C => self.b().tag = TAG_VALID,
            0x614C => self.b().tag = TAG_ZERO,
            0x624C => self.b().tag = TAG_SPECIAL,
            0x6048 => self.set_cc(false, false),
            0x6148 => self.set_cc(true, false),
            0x6248 => self.set_cc(false, true),
            0x6348 => self.set_cc(true, true),
            0x60CE => self.sw |= SW_C2,
            0x61CE => self.sw &= !SW_C2,
            0x6084 => self.sign_latch = self.ra().sign,
            0x6280 => self.a().sign = self.sign_latch,
            0x6380 => self.a().sign = !self.sign_latch,
            // sign transfers: the high byte picks the source (60 A, 62 latch, 63 not latch,
            // 64 B, 65 A xor B), the low byte the destination (80 A, 84 latch, 88 B);
            // C0/C8 write a constant
            0x6480 => self.a().sign = self.rb().sign,
            0x6580 => self.a().sign = self.ra().sign != self.rb().sign,
            0x6088 => self.b().sign = self.ra().sign,
            0x60C8 => self.b().sign = false,
            0x61C8 => self.b().sign = true,
            // arm the adder's loop hardware: multiply, divide
            0x6948 => self.loop_mode = LOOP_MUL,
            0x6A48 => self.loop_mode = LOOP_DIV,
            0x6C48 => self.loop_mode = LOOP_SQRT,
            0x6B48 => {
                self.loop_mode = LOOP_REM;
                self.loop_steps = 0;
                self.quotient = 0;
            }
            // the low three quotient bits (shifted down into the rounding bits) to C0, C3, C1
            0x6A06 => {
                self.sw &= !(SW_C0 | SW_C1 | SW_C3);
                if self.sum & 4 != 0 {
                    self.sw |= SW_C0
                }
                if self.sum & 2 != 0 {
                    self.sw |= SW_C3
                }
                if self.sum & 1 != 0 {
                    self.sw |= SW_C1
                }
            }
            0x60C0 => self.a().sign = false,
            0x61C0 => self.a().sign = true,
            0x6384 => self.sign_latch = !self.sign_latch,
            0x6584 => self.sign_latch = self.ra().sign != self.rb().sign,
            // leading zero bytes of the sum register to the byte part of the shift count,
            // then leading zero bits (0-7) to its bit part
            0x6E80 => {
                let f = (self.sum >> RND) as u64;
                self.shift_count = (f.leading_zeros().min(56) & 0x38) as u16;
            }
            0x6D80 => {
                let f = (self.sum >> RND) as u64;
                self.shift_count = (self.shift_count & 0x38) | (f.leading_zeros().min(7)) as u16;
            }
            0x680A => self.breg = (self.breg & !7) | self.round_h as u128,
            0x6904 | 0x6804 => self.sum = (self.sum & !7) | self.round_h as u128,
            // precision control to the shift count: 23 or 52 (one less than the bits kept)
            0x6880 => self.shift_count = if self.precision_bits() == 24 { 23 } else { 52 },
            _ => return Err("control word".into()),
        }
        Ok(())
    }

    fn misc(&mut self, w: u16) -> Result<(), String> {
        match w {
            0xEC00 => self.swap = !self.swap,
            0xE802 => self.counter = self.counter.wrapping_add(1) & 0x3F,
            0xE800 => self.counter = self.counter.wrapping_sub(1) & 0x3F,
            0xF408 => self.flag = false,
            // seen before the last fraction word of a memory operand is read; nothing the
            // model needs (inferred)
            0xF40E => {}
            // part of the loop words (see hw_loop)
            0xF40C | 0xE400 if self.loop_mode != 0 => {}
            // the exponent converter's verdict becomes the overflow or underflow exception
            0xE402 => {
                if self.cond(0x1F)? {
                    self.exc |= SW_OE
                }
                else if !self.cond(0x18)? {
                    self.exc |= SW_UE
                }
            }
            _ => return Err("misc word".into()),
        }
        Ok(())
    }

    /// Exponent constants (number selected by an immediate transfer). Inferred from use.
    fn exp_const(&self) -> Result<u16, String> {
        Ok(match self.const_sel {
            // exponent of an integer left-aligned in the fraction, by operand size
            0x0 => match self.operand_words() {
                1 => 0x400E,
                2 => 0x401E,
                _ => 0x403E,
            },
            // exponent of the smallest normal short or long real
            0x2 => match self.real_format() {
                Some((diff, _)) => diff + 1,
                None => return Err("exponent constant 2 without a real operand".into()),
            },
            0x4 => 0x403A, // 10^18
            0x5 => 0x3FFF,
            0x6 => 0x403E,
            0x7 => 0x3FF0,
            0xB => 0x3FFE,
            0xD => 0x6000,
            n => return Err(format!("exponent constant {:X}", n)),
        })
    }

    /// Fraction constants selected by the instruction (the constant ROM's first entries,
    /// 64 bits each).
    fn frac_const(&self) -> Result<u64, String> {
        if self.const_sel == 4 {
            return Ok(0xDE0B_6B3A_7640_0000); // 10^18 (with exponent constant 4)
        }
        Ok(match self.opcode {
            0x1E8 => 0x8000_0000_0000_0000, // 1
            0x1E9 => 0xD49A_784B_CD1B_8AFE, // log2(10)
            0x1EA => 0xB8AA_3B29_5C17_F0BC, // log2(e)
            0x1EB => 0xC90F_DAA2_2168_C235, // pi
            0x1EC => 0x9A20_9A84_FBCF_F799, // log10(2)
            0x1ED => 0xB172_17F7_D1CF_79AC, // ln(2)
            n => return Err(format!("fraction constant for opcode {:03X}", n)),
        })
    }

    fn read(&mut self, src: u8) -> Result<Val, String> {
        Ok(match src {
            0x00 => Val::exp(self.shift_count & 0x3F),
            // the exponent converter's output
            0x01 => Val::exp(match self.conv_fmt {
                // FSQRT's use: the exponent with a second bias, so that half of it is a
                // biased exponent again (inferred)
                Fmt::X80 => self.exp_conv.wrapping_add(0x4000),
                Fmt::Raw => self.exp_conv,
                Fmt::Load(diff, _) => self.exp_conv.max(1) + diff,
                Fmt::Store(diff, _) => self.exp_conv.wrapping_sub(diff),
            }),
            // the next word of the memory operand
            0x04 => {
                let w = *self.bus_in.get(self.bus_next).ok_or("more than five operand words read")?;
                self.bus_next += 1;
                Val::frac(w as u64)
            }
            0x02 => Val::reg(self.st[self.top as usize]),
            0x03 => Val::reg(self.st[self.sti()]),
            0x05 => Val::exp(self.exp_const()?),
            0x06 => Val::reg(Reg { sign: false, exp: 0, frac: 0, tag: TAG_ZERO }),
            0x08 => Val::exp(self.ra().exp),
            0x09 => Val::exp(((self.ra().sign as u16) << 15) | (self.ra().exp & 0x7FFF)),
            0x0A => Val::frac(self.ra().frac),
            0x0B => Val::reg(self.ra()),
            0x0C => Val::exp(self.rb().exp),
            0x0D => Val::exp(((self.rb().sign as u16) << 15) | (self.rb().exp & 0x7FFF)),
            0x0E => Val::frac(self.rb().frac),
            0x0F => Val::reg(self.rb()),
            0x10 => Val::wide(self.sum & FRAC_MASK),
            0x11 => Val::wide(self.sum & (FRAC_MASK | 7)),
            0x12 => Val::wide(self.sum & (FRAC_MASK | BIT64)),
            0x13 => Val::wide(self.sum),
            0x14 => Val::wide(self.shl),
            // the shifter's output merged with the sum register
            0x1C => Val::wide(self.shl | (self.sum & FRAC_MASK)),
            0x15 => Val::wide(self.shr),
            0x16 => Val::frac(self.quotient as u64),
            0x17 => Val::wide(self.tmpc),
            0x1A => Val::frac(self.frac_const()?),
            0x1B => Val::frac(1),
            // the exponent adjustment; reading it puts it back to its resting value
            // (inferred: FBSTP runs an add and then a divide, each with its own adjustment)
            0x1E => {
                let v = self.exp_adj as u16;
                self.exp_adj = 1;
                Val::exp(v)
            }
            0x1F => Val::reg(Reg { sign: true, exp: 0x7FFF, frac: 0xC000_0000_0000_0000, tag: TAG_SPECIAL }),
            _ => return Err(format!("source register {:02X}", src)),
        })
    }

    fn write(&mut self, dst: u8, v: Val, src: u8) -> Result<(), String> {
        match dst {
            0x00 => self.shift_count = v.exp,
            0x01 => {
                self.exp_conv = v.exp;
                // the converter knows the operand format: a field cut out of a short or
                // long real on a load, a result exponent on its way to one on a store
                self.conv_fmt = match self.real_format() {
                    Some((diff, max)) if src == 0x15 => Fmt::Load(diff, max),
                    Some((diff, max)) if src == 0x10 && self.store_class() => Fmt::Store(diff, max),
                    // sign and exponent together: the sign is what overflow reports
                    _ if src == 0x09 || src == 0x0D => Fmt::Raw,
                    _ => Fmt::X80,
                };
            }
            // a word for the bus interface to write
            0x04 => self.bus_out.push(v.exp),
            0x02 | 0x03 => {
                if !v.whole {
                    return Err("part of a register to the stack".into());
                }
                let mut r = Reg { sign: v.sign, exp: v.exp & 0x7FFF, frac: v.f64(), tag: 0 };
                // inferred: the stack tag follows the value written
                r.tag = r.class();
                let i = if dst == 2 { self.top as usize } else { self.sti() };
                self.st[i] = r;
            }
            0x06 => self.breg = v.wide & FRAC_MASK,
            0x07 => self.breg = v.wide & (FRAC_MASK | 7),
            0x08 => self.a().exp = v.exp,
            0x09 => {
                self.a().sign = v.exp & 0x8000 != 0;
                self.a().exp = v.exp & 0x7FFF;
            }
            0x0A => self.a().frac = v.f64(),
            0x0B | 0x0F => {
                let r = if v.whole {
                    Reg { sign: v.sign, exp: v.exp, frac: v.f64(), tag: v.tag }
                }
                else {
                    return Err("part of a register to a whole temporary".into());
                };
                if dst == 0x0B {
                    *self.a() = r
                }
                else {
                    *self.b() = r
                }
            }
            0x0C => self.b().exp = v.exp,
            0x0D => {
                self.b().sign = v.exp & 0x8000 != 0;
                self.b().exp = v.exp & 0x7FFF;
            }
            0x0E => self.b().frac = v.f64(),
            0x10 => self.sum = (self.sum & 7) | (v.wide & FRAC_MASK),
            0x11 => self.sum = v.wide & (FRAC_MASK | 7),
            0x12 => self.sum = v.wide & (FRAC_MASK | BIT64),
            0x13 => self.sum = v.wide,
            0x15 => self.counter = (v.f64() & 0x3F) as u8,
            0x16 => self.quotient = (v.wide >> RND) & 0x1_FFFF_FFFF_FFFF_FFFF,
            0x17 => self.tmpc = v.wide,
            _ => return Err(format!("destination register {:02X}", dst)),
        }
        Ok(())
    }

    /// The adder's loop hardware. A control word arms it (multiply or divide), then an
    /// adder word with I and J set runs the loop counter down; the adder words that follow
    /// (final correction and remainder steps) are folded into the result here.
    ///
    /// Inferred from the microcode around the loops, not from the circuit: the multiply
    /// takes two multiplier bits per step (counter + 1 steps) and leaves the product's top
    /// 64 bits in the sum register with guard, round and sticky; the divide makes one
    /// quotient bit per step and leaves the same three bits from the remainder.
    fn hw_loop(&mut self, ijk: u8, _op: u8) -> Result<(), String> {
        if self.loop_mode == LOOP_REM {
            // FPREM: the loop word only sets the number of steps (it is skipped when the
            // exponents are equal); the subtract word after it finishes the division of
            // the sum register (shifted left once per step) by the B register
            if ijk == 6 {
                self.loop_steps = self.counter as u32 + 1;
                self.counter = 0;
            }
            else if _op == 5 {
                let a = (self.sum >> RND) as u64 as u128;
                let b = (self.breg >> RND) as u64 as u128;
                let n = a << self.loop_steps;
                let (q, r) = if b == 0 { (0, a) } else { (n / b, n % b) };
                self.quotient = q as u64 as u128;
                self.sum = (r << RND) & FRAC_MASK;
            }
            return Ok(());
        }
        if ijk != 6 {
            // the words after the loop
            return if self.loop_mode != 0 { Ok(()) } else { Err("adder variant".into()) };
        }
        let steps = self.counter as u32 + 1;
        self.counter = 0;
        let b = (self.breg >> RND) as u64 as u128;
        match self.loop_mode {
            LOOP_MUL => {
                let n = 2 * steps;
                let p = (self.quotient as u64 as u128) * b;
                let lost = p & ((1u128 << n) - 1);
                let grs = ((lost >> (n - 2)) << 1) | (lost & ((1u128 << (n - 2)) - 1) != 0) as u128;
                self.sum = ((p >> n) << RND | grs) & WIDE_MASK;
            }
            LOOP_DIV => {
                let a = (self.sum >> RND) as u64 as u128;
                let (q, grs) = if b == 0 {
                    (u64::MAX as u128, 7)
                }
                else {
                    let num = a << (steps - 1);
                    let rem = (num % b) << 2;
                    (num / b, (rem / b) << 1 | (rem % b != 0) as u128)
                };
                self.quotient = q as u64 as u128;
                self.sum = grs & 7;
            }
            LOOP_SQRT => {
                // the radicand (1 <= x < 4, 63 fraction bits) is in the quotient register;
                // the root (1 <= r < 2) builds up in the B register, one bit per step
                let n = self.quotient << 63;
                let (mut root, mut rem) = (0u128, 0u128);
                for i in (0..steps + 2).rev() {
                    let two = if i >= 2 { (n >> (2 * (i - 2))) & 3 } else { 0 };
                    rem = rem << 2 | two;
                    let trial = root << 2 | 1;
                    root <<= 1;
                    if rem >= trial {
                        rem -= trial;
                        root |= 1;
                    }
                }
                self.breg = (root >> 2) << RND & FRAC_MASK;
                self.loop_grs = ((root & 3) << 1 | (rem != 0) as u128) as u8;
            }
            _ => return Err("loop without a mode".into()),
        }
        Ok(())
    }

    /// MNO of an adder word: M inverts the B register, N replaces it with zero, O is the
    /// carry in.
    fn adder(&mut self, a: Val, round_bits: bool, op: u8) -> Result<(), String> {
        let keep = if round_bits { FRAC_MASK | 7 } else { FRAC_MASK };
        let x = a.wide & keep;
        let mut y = if op & 2 != 0 { 0 } else { self.breg & keep };
        if op & 4 != 0 {
            y = !y & keep;
        }
        // the carry enters at the lowest bit in use
        let cin = if op & 1 != 0 { if round_bits { 1 } else { 1 << RND } } else { 0 };
        let s = x + y + cin;
        self.carry = s & BIT64 != 0;
        self.sum = s & WIDE_MASK;
        if self.loop_mode == LOOP_SQRT && !round_bits {
            // the root leaves the loop with its guard, round and sticky bits (inferred)
            self.sum |= self.loop_grs as u128;
        }
        Ok(())
    }

    /// Shift by the shift count register. The KLMNO field picks what part of the count is
    /// used: 1x11x whole count, 1x10x its byte part, 1x01x its bit part, 1x00x the
    /// complement of the count (63 - count); bit O keeps the sticky bit in place. 11001 is
    /// a right shift by one that keeps the sticky bit.
    fn shift_var(&mut self, v: Val, right: bool, by: u8) -> Result<(), String> {
        let c = (self.shift_count & 0x3F) as u32;
        let n = match by & !1 {
            0x16 => c,
            0x14 => c & 0x38,
            0x12 => c & 7,
            0x10 => 63 - c,
            0x18 => 1,
            _ => return Err(format!("variable shift mode {:02X}", by)),
        };
        let sticky = by & 1 != 0 && v.wide & 1 != 0;
        if right {
            let lost = v.wide & ((1u128 << n) - 1) != 0;
            self.shr = (v.wide >> n) & WIDE_MASK;
            if by & 1 != 0 && lost {
                self.shr |= 1;
            }
        }
        else {
            self.shl_out(v.wide << n, sticky);
        }
        Ok(())
    }

    /// A left shift's output. The rounding bits a later "rounding -> ..." word takes come
    /// from it: the fraction's top bit is the last bit kept, the two below are guard and
    /// round, everything under them (and what was shifted out below) is sticky.
    fn shl_out(&mut self, w: u128, sticky: bool) {
        self.shl = (w & WIDE_MASK) | sticky as u128;
        let f = (w >> RND) as u64;
        self.round_h = ((f >> 60) as u8 & 6) | ((f << 3 != 0) || w & 7 != 0 || sticky) as u8;
    }

    fn cond_small_shift(&self) -> bool {
        self.exp_conv & 0x7FC0 == 0
    }

    fn shift(&mut self, v: Val, right: bool, count: u32) {
        if right {
            self.shr = (v.wide >> count) & WIDE_MASK;
        }
        else {
            self.shl_out(v.wide << count, false);
            if count == 0 && !self.cond_small_shift() {
                // CREATE_DENORM's path for a shift of 64 or more: everything is tail.
                // Assumed to give the true rounding bits (to be checked on a real chip).
                let n = self.exp_conv as u32;
                let f = v.f64();
                self.round_h = match n {
                    64 => ((f >> 61) as u8 & 6) | (f << 2 != 0) as u8,
                    65 => ((f >> 62) as u8 & 2) | (f << 1 != 0) as u8,
                    _ => (f != 0) as u8,
                };
            }
        }
    }
}
