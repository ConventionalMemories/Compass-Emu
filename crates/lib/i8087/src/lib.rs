//! Intel 8087 numeric data processor.
//!
//! Registers are real 80-bit values (rustc_apfloat's X87DoubleExtended, so add, subtract,
//! multiply, divide, compares, rounding and conversions are bit-exact in all four rounding
//! modes). Square root is within 1 ulp; FPTAN, FPATAN, F2XM1, FYL2X and FYL2XP1 are computed in
//! quad precision (113 bits) and rounded once, so they are the correctly rounded 80-bit result
//! (the real 8087 matches that in most cases, but is sometimes 1 ulp off). FPTAN follows the
//! chip's own CORDIC + rational method (fptan.rs, fitted to a real 1101): its partial Y/X pair
//! (X near 1.5, not 1) and a busy time that depends on the argument.
//!
//! Timing: each instruction keeps the 8087 busy for its typical execution time from the Intel
//! 8087 data sheet (FADD, FLD/FST m64, FILD/FIST m16 and the transcendentals adjusted to the
//! times GRiDBench measured on a real 1101, for its operands, where GRiD-OS runs every WAIT
//! as an INT 3) (clocks of the 5 MHz CPU clock it shares with the 8086). BUSY drives the
//! 8086 TEST pin, so FWAIT waits for it. Memory operands are read when the instruction starts
//! and results are written immediately (the real chip writes at the end of the instruction;
//! correct programs FWAIT before reading them, so the difference does not show).

use rustc_apfloat::ieee::{Double, Single, X87DoubleExtended as F80};
use rustc_apfloat::{Float, FloatConvert, Round, Status, StatusAnd};
use rustc_apfloat::ieee::Quad;

mod trans;
pub mod fptan;

fn to_q(x: F80) -> Quad {
    x.convert(&mut false).value
}

/// Memory as the 8087 sees it (linear addresses).
pub trait FpuMem {
    fn read_u8(&mut self, addr: u32) -> u8;
    fn write_u8(&mut self, addr: u32, v: u8);
}

// Status word bits
const SW_IE: u16 = 0x0001;
#[allow(dead_code)]
const SW_DE: u16 = 0x0002;
const SW_ZE: u16 = 0x0004;
const SW_OE: u16 = 0x0008;
const SW_UE: u16 = 0x0010;
const SW_PE: u16 = 0x0020;
const SW_IR: u16 = 0x0080;
const SW_C0: u16 = 0x0100;
const SW_C1: u16 = 0x0200;
const SW_C2: u16 = 0x0400;
const SW_C3: u16 = 0x4000;
const SW_CC: u16 = SW_C0 | SW_C1 | SW_C2 | SW_C3;

const TAG_VALID: u16 = 0;
const TAG_ZERO: u16 = 1;
const TAG_SPECIAL: u16 = 2;
const TAG_EMPTY: u16 = 3;

/// Real indefinite (negative quiet NaN, C000000000000000h significand).
fn indefinite() -> F80 {
    F80::from_bits(0xFFFF_C000_0000_0000_0000)
}

fn f80_to_f64(x: F80) -> f64 {
    let r: StatusAnd<Double> = x.convert_r(Round::NearestTiesToEven, &mut false);
    f64::from_bits(r.value.to_bits() as u64)
}

fn f64_to_f80(x: f64) -> F80 {
    let d = Double::from_bits(x.to_bits() as u128);
    let r: StatusAnd<F80> = d.convert(&mut false);
    r.value
}

/// Instruction counters, for profiling.
#[derive(Clone, Default, Debug, serde::Serialize, serde::Deserialize)]
pub struct FpuStats {
    pub instructions: u64,
    pub busy_clocks: u64,
    pub unknown: u64,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct I8087 {
    #[serde(with = "f80_regs")]
    st: [F80; 8],
    pub cw: u16,
    pub sw: u16,
    pub tw: u16,
    /// Instruction pointer / opcode / operand pointer for FSTENV/FSAVE (8087 real-mode format).
    ip: u32,
    opcode: u16,
    dp: u32,
    /// CPU clock at which the current instruction finishes.
    busy_until: u64,
    /// Interrupt request (unmasked exception with interrupts enabled).
    irq: bool,
    pub stats: FpuStats,
    /// Clocks added to every instruction's busy time (0 = pure data sheet, the default; for
    /// experiments).
    pub extra_busy: u32,
    /// Shortest WAIT in clocks (0 = none). A bare FWAIT costs ~100 clocks on a real Compass
    /// because GRiD-OS runs it as INT 3, not because of the chip.
    pub wait_min: u32,
}

/// Arithmetic operation selected by the ModR/M reg field of D8/DA/DC/DE.
#[derive(Copy, Clone)]
enum Arith {
    Add,
    Mul,
    Com,
    Comp,
    Sub,
    SubR,
    Div,
    DivR,
}

impl Arith {
    fn from(reg: u8) -> Arith {
        match reg & 7 {
            0 => Arith::Add,
            1 => Arith::Mul,
            2 => Arith::Com,
            3 => Arith::Comp,
            4 => Arith::Sub,
            5 => Arith::SubR,
            6 => Arith::Div,
            _ => Arith::DivR,
        }
    }
}

impl Default for I8087 {
    fn default() -> Self {
        Self::new()
    }
}

impl I8087 {
    pub fn new() -> I8087 {
        let mut f = I8087 {
            st: [F80::ZERO; 8],
            cw: 0x03FF,
            sw: 0,
            tw: 0xFFFF,
            ip: 0,
            opcode: 0,
            dp: 0,
            busy_until: 0,
            irq: false,
            stats: FpuStats::default(),
            extra_busy: 0,
            wait_min: 0,
        };
        f.finit();
        f
    }

    fn finit(&mut self) {
        self.cw = 0x03FF;
        self.sw = 0;
        self.tw = 0xFFFF;
        self.irq = false;
    }

    /// BUSY output (8086 TEST pin, active while executing).
    pub fn busy(&self, now: u64) -> bool {
        now < self.busy_until
    }

    pub fn busy_until(&self) -> u64 {
        self.busy_until
    }

    /// INT output (to IR6 on the Compass).
    pub fn irq(&self) -> bool {
        self.irq
    }

    pub fn top(&self) -> usize {
        ((self.sw >> 11) & 7) as usize
    }

    fn set_top(&mut self, t: usize) {
        self.sw = (self.sw & !0x3800) | ((t as u16 & 7) << 11);
    }

    fn phys(&self, i: usize) -> usize {
        (self.top() + i) & 7
    }

    fn tag(&self, phys: usize) -> u16 {
        (self.tw >> (phys * 2)) & 3
    }

    fn set_tag(&mut self, phys: usize, t: u16) {
        self.tw = (self.tw & !(3 << (phys * 2))) | (t << (phys * 2));
    }

    fn tag_for(v: F80) -> u16 {
        if v.is_zero() {
            TAG_ZERO
        }
        else if v.is_nan() || v.is_infinite() || v.is_denormal() {
            TAG_SPECIAL
        }
        else {
            TAG_VALID
        }
    }

    fn empty(&self, i: usize) -> bool {
        self.tag(self.phys(i)) == TAG_EMPTY
    }

    /// ST(i) as a value (stack underflow -> invalid, returns indefinite).
    pub fn st(&self, i: usize) -> F80 {
        self.st[self.phys(i)]
    }

    /// ST(i) as an f64, for debugging and tools.
    pub fn st_f64(&self, i: usize) -> Option<f64> {
        if self.empty(i) {
            None
        }
        else {
            Some(f80_to_f64(self.st(i)))
        }
    }

    /// ST(i) as raw 80-bit bits.
    pub fn st_bits(&self, i: usize) -> Option<u128> {
        if self.empty(i) {
            None
        }
        else {
            Some(self.st(i).to_bits())
        }
    }

    fn set_st(&mut self, i: usize, v: F80) {
        let p = self.phys(i);
        self.st[p] = v;
        self.set_tag(p, Self::tag_for(v));
    }

    fn push(&mut self, v: F80) {
        let t = (self.top() + 7) & 7;
        if self.tag(t) != TAG_EMPTY {
            // stack overflow: invalid; masked response pushes indefinite
            if self.exception(SW_IE) {
                return;
            }
            self.set_top(t);
            self.set_st(0, indefinite());
            return;
        }
        self.set_top(t);
        self.set_st(0, v);
    }

    fn pop(&mut self) {
        let p = self.phys(0);
        self.set_tag(p, TAG_EMPTY);
        self.set_top((self.top() + 1) & 7);
    }

    /// Quad -> 80-bit in the current rounding mode (precision control does not apply).
    fn from_q(&self, v: Quad) -> F80 {
        v.convert_r(self.round(), &mut false).value
    }

    fn round(&self) -> Round {
        match (self.cw >> 10) & 3 {
            0 => Round::NearestTiesToEven,
            1 => Round::TowardNegative,
            2 => Round::TowardPositive,
            _ => Round::TowardZero,
        }
    }

    /// Record exception flags; returns true when the exception is unmasked (the instruction
    /// must then not store its result).
    fn exception(&mut self, flags: u16) -> bool {
        self.sw |= flags;
        let unmasked = flags & !self.cw & 0x3F;
        if unmasked != 0 {
            self.sw |= SW_IR;
            if self.cw & 0x80 == 0 {
                self.irq = true;
            }
            true
        }
        else {
            false
        }
    }

    fn status_flags(s: Status) -> u16 {
        let mut f = 0;
        if s.contains(Status::INVALID_OP) {
            f |= SW_IE;
        }
        if s.contains(Status::DIV_BY_ZERO) {
            f |= SW_ZE;
        }
        if s.contains(Status::OVERFLOW) {
            f |= SW_OE;
        }
        if s.contains(Status::UNDERFLOW) {
            f |= SW_UE;
        }
        if s.contains(Status::INEXACT) {
            f |= SW_PE;
        }
        f
    }

    /// Round a result to the precision control setting (24, 53 or 64 bits).
    fn precision(&self, r: StatusAnd<F80>) -> StatusAnd<F80> {
        let rnd = self.round();
        match (self.cw >> 8) & 3 {
            0 => {
                let s: StatusAnd<Single> = r.value.convert_r(rnd, &mut false);
                let back: StatusAnd<F80> = s.value.convert(&mut false);
                StatusAnd { status: r.status | s.status, value: back.value }
            }
            2 => {
                let d: StatusAnd<Double> = r.value.convert_r(rnd, &mut false);
                let back: StatusAnd<F80> = d.value.convert(&mut false);
                StatusAnd { status: r.status | d.status, value: back.value }
            }
            _ => r,
        }
    }

    /// Finish an arithmetic result: flags, masked/unmasked handling. Returns Some(value) to store.
    fn result(&mut self, r: StatusAnd<F80>) -> Option<F80> {
        let r = self.precision(r);
        let mut flags = Self::status_flags(r.status);
        if r.value.is_denormal() {
            flags |= SW_UE;
        }
        if flags != 0 && self.exception(flags) && flags & !SW_PE & !self.cw & 0x3F != 0 {
            return None;
        }
        if r.status.contains(Status::INVALID_OP) && r.value.is_nan() {
            return Some(indefinite());
        }
        Some(r.value)
    }

    fn arith(&mut self, op: Arith, a: F80, b: F80) -> Option<F80> {
        let rnd = self.round();
        let r = match op {
            Arith::Add => a.add_r(b, rnd),
            Arith::Sub => a.sub_r(b, rnd),
            Arith::SubR => b.sub_r(a, rnd),
            Arith::Mul => a.mul_r(b, rnd),
            Arith::Div => a.div_r(b, rnd),
            Arith::DivR => b.div_r(a, rnd),
            Arith::Com | Arith::Comp => {
                self.compare(a, b);
                return None;
            }
        };
        self.result(r)
    }

    fn compare(&mut self, a: F80, b: F80) {
        self.sw &= !SW_CC;
        if a.is_nan() || b.is_nan() {
            self.sw |= SW_C0 | SW_C2 | SW_C3;
            self.exception(SW_IE);
            return;
        }
        match a.partial_cmp(&b) {
            Some(std::cmp::Ordering::Less) => self.sw |= SW_C0,
            Some(std::cmp::Ordering::Equal) => self.sw |= SW_C3,
            Some(std::cmp::Ordering::Greater) => {}
            None => self.sw |= SW_C0 | SW_C2 | SW_C3,
        }
    }

    // ---- memory operand helpers ----
    fn rd(mem: &mut dyn FpuMem, a: u32, n: usize) -> u128 {
        let mut v = 0u128;
        for i in 0..n {
            v |= (mem.read_u8((a + i as u32) & 0xFFFFF) as u128) << (8 * i);
        }
        v
    }

    fn wr(mem: &mut dyn FpuMem, a: u32, n: usize, v: u128) {
        for i in 0..n {
            mem.write_u8((a + i as u32) & 0xFFFFF, (v >> (8 * i)) as u8);
        }
    }

    fn load_real32(mem: &mut dyn FpuMem, a: u32) -> F80 {
        let s = Single::from_bits(Self::rd(mem, a, 4));
        let r: StatusAnd<F80> = s.convert(&mut false);
        r.value
    }

    fn load_real64(mem: &mut dyn FpuMem, a: u32) -> F80 {
        let d = Double::from_bits(Self::rd(mem, a, 8));
        let r: StatusAnd<F80> = d.convert(&mut false);
        r.value
    }

    fn load_int(mem: &mut dyn FpuMem, a: u32, bytes: usize) -> F80 {
        let raw = Self::rd(mem, a, bytes);
        let bits = bytes * 8;
        let v = ((raw << (128 - bits)) as i128) >> (128 - bits);
        F80::from_i128(v).value
    }

    fn load_bcd(mem: &mut dyn FpuMem, a: u32) -> F80 {
        let raw = Self::rd(mem, a, 10);
        let mut v: i128 = 0;
        for d in (0..18).rev() {
            v = v * 10 + ((raw >> (d * 4)) & 0xF) as i128;
        }
        if raw >> 79 & 1 != 0 {
            v = -v;
        }
        F80::from_i128(v).value
    }

    fn store_int(&mut self, mem: &mut dyn FpuMem, a: u32, bytes: usize, v: F80) {
        let bits = bytes * 8;
        let r = v.to_i128_r(bits, self.round(), &mut false);
        let mut out = r.value;
        let mut flags = 0;
        if r.status.contains(Status::INVALID_OP) || v.is_nan() {
            flags |= SW_IE;
            out = -1i128 << (bits - 1); // integer indefinite
        }
        if r.status.contains(Status::INEXACT) {
            flags |= SW_PE;
        }
        if flags != 0 && self.exception(flags) && flags & SW_IE != 0 {
            return;
        }
        Self::wr(mem, a, bytes, out as u128);
    }

    fn store_bcd(&mut self, mem: &mut dyn FpuMem, a: u32, v: F80) {
        let r = v.to_i128_r(64, self.round(), &mut false);
        let n = r.value;
        if r.status.contains(Status::INVALID_OP) || v.is_nan() || n.unsigned_abs() >= 1_000_000_000_000_000_000 {
            if self.exception(SW_IE) {
                return;
            }
            Self::wr(mem, a, 10, 0xFFFF_C000_0000_0000_0000);
            return;
        }
        let mut m = n.unsigned_abs();
        let mut out: u128 = 0;
        for d in 0..18 {
            out |= ((m % 10) as u128) << (d * 4);
            m /= 10;
        }
        if n < 0 || v.is_negative() {
            out |= 1 << 79;
        }
        Self::wr(mem, a, 10, out);
    }

    fn store_real32(&mut self, mem: &mut dyn FpuMem, a: u32, v: F80) {
        let r: StatusAnd<Single> = v.convert_r(self.round(), &mut false);
        let f = Self::status_flags(r.status);
        if f != 0 && self.exception(f) && f & !SW_PE & !self.cw & 0x3F != 0 {
            return;
        }
        Self::wr(mem, a, 4, r.value.to_bits());
    }

    fn store_real64(&mut self, mem: &mut dyn FpuMem, a: u32, v: F80) {
        let r: StatusAnd<Double> = v.convert_r(self.round(), &mut false);
        let f = Self::status_flags(r.status);
        if f != 0 && self.exception(f) && f & !SW_PE & !self.cw & 0x3F != 0 {
            return;
        }
        Self::wr(mem, a, 8, r.value.to_bits());
    }

    /// Check ST(i) is not empty; on underflow raise invalid and return None (masked: indefinite).
    fn src(&mut self, i: usize) -> Option<F80> {
        if self.empty(i) {
            if self.exception(SW_IE) {
                return None;
            }
            return Some(indefinite());
        }
        Some(self.st(i))
    }

    fn env_words(&self) -> [u16; 7] {
        // 8087 real-mode environment layout
        [
            self.cw,
            self.sw,
            self.tw,
            self.ip as u16,
            (((self.ip >> 16) as u16 & 0xF) << 12) | (self.opcode & 0x7FF),
            self.dp as u16,
            ((self.dp >> 16) as u16 & 0xF) << 12,
        ]
    }

    fn store_env(&self, mem: &mut dyn FpuMem, a: u32) {
        for (i, w) in self.env_words().iter().enumerate() {
            Self::wr(mem, a + i as u32 * 2, 2, *w as u128);
        }
    }

    fn load_env(&mut self, mem: &mut dyn FpuMem, a: u32) {
        let w: Vec<u16> = (0..7).map(|i| Self::rd(mem, a + i * 2, 2) as u16).collect();
        self.cw = w[0];
        self.sw = w[1];
        self.tw = w[2];
        self.ip = w[3] as u32 | ((w[4] as u32 >> 12) << 16);
        self.opcode = w[4] & 0x7FF;
        self.dp = w[5] as u32 | ((w[6] as u32 >> 12) << 16);
        self.irq = self.sw & SW_IR != 0 && self.cw & 0x80 == 0;
    }

    /// Execute an ESC instruction. `opcode` is D8-DF, `modrm` the ModR/M byte, `ea` the linear
    /// address of the memory operand (None for register forms), `ip` the linear address of the
    /// instruction, `now` the CPU clock. Returns the extra CPU clocks the 8087's own memory
    /// cycles take from the bus (operand words beyond the first, which the 8086 reads itself).
    pub fn esc(&mut self, mem: &mut dyn FpuMem, now: u64, opcode: u8, modrm: u8, ea: Option<u32>, ip: u32) -> u32 {
        let start = now.max(self.busy_until);
        let reg = (modrm >> 3) & 7;
        let rm = (modrm & 7) as usize;
        let op = opcode & 7;
        let (clocks, words) = match ea {
            Some(a) => self.esc_mem(mem, op, reg, a),
            None => self.esc_reg(op, reg, rm),
        };
        // FNINIT/FNCLEX/FSTENV etc. keep the last-instruction pointers; ordinary ones update them.
        if !(op == 1 && ea.is_some() && reg >= 4) && !(op == 3 && ea.is_none() && reg == 4) {
            self.ip = ip;
            self.opcode = (((op as u16) << 8) | modrm as u16) & 0x7FF;
            if let Some(a) = ea {
                self.dp = a;
            }
        }
        self.busy_until = start + clocks as u64 + self.extra_busy as u64;
        self.stats.instructions += 1;
        self.stats.busy_clocks += clocks as u64;
        // 4 clocks per extra word transfer, plus the request/grant handover (about 2+2).
        if words > 1 {
            (words - 1) * 4 + 4
        }
        else {
            0
        }
    }

    /// Memory forms. Returns (execution clocks, words transferred).
    fn esc_mem(&mut self, mem: &mut dyn FpuMem, op: u8, reg: u8, a: u32) -> (u32, u32) {
        match op {
            0 | 2 | 4 | 6 => {
                // arithmetic with m32real / m32int / m64real / m16int
                let (v, base, words) = match op {
                    0 => (Self::load_real32(mem, a), [105, 118, 65, 68, 105, 105, 220, 221], 2),
                    2 => (Self::load_int(mem, a, 4), [125, 136, 85, 87, 125, 125, 236, 237], 2),
                    4 => (Self::load_real64(mem, a), [110, 161, 70, 72, 110, 110, 225, 226], 4),
                    _ => (Self::load_int(mem, a, 2), [120, 130, 80, 82, 120, 120, 230, 231], 1),
                };
                let ar = Arith::from(reg);
                if let Some(st0) = self.src(0) {
                    match ar {
                        Arith::Com => self.compare(st0, v),
                        Arith::Comp => {
                            self.compare(st0, v);
                            self.pop();
                        }
                        _ => {
                            if let Some(r) = self.arith(ar, st0, v) {
                                self.set_st(0, r);
                            }
                        }
                    }
                }
                (base[reg as usize], words)
            }
            1 => match reg {
                0 => {
                    let v = Self::load_real32(mem, a);
                    self.push(v);
                    (43, 2)
                }
                2 | 3 => {
                    if let Some(v) = self.src(0) {
                        self.store_real32(mem, a, v);
                    }
                    if reg == 3 {
                        self.pop();
                    }
                    (if reg == 2 { 87 } else { 89 }, 2)
                }
                4 => {
                    self.load_env(mem, a);
                    (40, 7)
                }
                5 => {
                    self.cw = Self::rd(mem, a, 2) as u16;
                    (10, 1)
                }
                6 => {
                    self.store_env(mem, a);
                    self.cw |= 0x3F; // FSTENV masks all exceptions afterwards
                    (45, 7)
                }
                7 => {
                    Self::wr(mem, a, 2, self.cw as u128);
                    (15, 1)
                }
                _ => {
                    self.stats.unknown += 1;
                    (10, 1)
                }
            },
            3 => match reg {
                0 => {
                    let v = Self::load_int(mem, a, 4);
                    self.push(v);
                    (56, 2)
                }
                2 | 3 => {
                    if let Some(v) = self.src(0) {
                        self.store_int(mem, a, 4, v);
                    }
                    if reg == 3 {
                        self.pop();
                    }
                    (if reg == 2 { 88 } else { 90 }, 2)
                }
                5 => {
                    let v = F80::from_bits(Self::rd(mem, a, 10));
                    self.push(v);
                    (57, 5)
                }
                7 => {
                    if let Some(v) = self.src(0) {
                        Self::wr(mem, a, 10, v.to_bits());
                    }
                    self.pop();
                    (55, 5)
                }
                _ => {
                    self.stats.unknown += 1;
                    (10, 1)
                }
            },
            5 => match reg {
                0 => {
                    let v = Self::load_real64(mem, a);
                    self.push(v);
                    (39, 4)
                }
                2 | 3 => {
                    if let Some(v) = self.src(0) {
                        self.store_real64(mem, a, v);
                    }
                    if reg == 3 {
                        self.pop();
                    }
                    (if reg == 2 { 85 } else { 87 }, 4)
                }
                4 => {
                    // FRSTOR: environment + 8 registers in stack order
                    self.load_env(mem, a);
                    for i in 0..8 {
                        let v = F80::from_bits(Self::rd(mem, a + 14 + i * 10, 10));
                        let p = self.phys(i as usize);
                        self.st[p] = v;
                    }
                    (205, 47)
                }
                6 => {
                    self.store_env(mem, a);
                    for i in 0..8 {
                        Self::wr(mem, a + 14 + i * 10, 10, self.st(i as usize).to_bits());
                    }
                    self.finit();
                    (205, 47)
                }
                7 => {
                    Self::wr(mem, a, 2, self.sw as u128);
                    (15, 1)
                }
                _ => {
                    self.stats.unknown += 1;
                    (10, 1)
                }
            },
            _ => match reg {
                // DF
                0 => {
                    let v = Self::load_int(mem, a, 2);
                    self.push(v);
                    (42, 1)
                }
                2 | 3 => {
                    if let Some(v) = self.src(0) {
                        self.store_int(mem, a, 2, v);
                    }
                    if reg == 3 {
                        self.pop();
                    }
                    (if reg == 2 { 72 } else { 74 }, 1)
                }
                4 => {
                    let v = Self::load_bcd(mem, a);
                    self.push(v);
                    (300, 5)
                }
                5 => {
                    let v = Self::load_int(mem, a, 8);
                    self.push(v);
                    (64, 4)
                }
                6 => {
                    if let Some(v) = self.src(0) {
                        self.store_bcd(mem, a, v);
                    }
                    self.pop();
                    (530, 5)
                }
                7 => {
                    if let Some(v) = self.src(0) {
                        self.store_int(mem, a, 8, v);
                    }
                    self.pop();
                    (100, 4)
                }
                _ => {
                    self.stats.unknown += 1;
                    (10, 1)
                }
            },
        }
    }

    /// Register forms. Returns (execution clocks, 0).
    fn esc_reg(&mut self, op: u8, reg: u8, i: usize) -> (u32, u32) {
        let c = match op {
            0 => {
                // D8: ST = ST op ST(i)
                let ar = Arith::from(reg);
                let short = Self::short_mul(self.st(0), self.st(i));
                if let (Some(a), Some(b)) = (self.src(0), self.src(i)) {
                    match ar {
                        Arith::Com => self.compare(a, b),
                        Arith::Comp => {
                            self.compare(a, b);
                            self.pop();
                        }
                        _ => {
                            if let Some(r) = self.arith(ar, a, b) {
                                self.set_st(0, r);
                            }
                        }
                    }
                }
                if reg == 1 && !short {
                    138
                }
                else {
                    [77, 97, 45, 47, 77, 77, 198, 199][reg as usize]
                }
            }
            4 | 6 => {
                // DC: ST(i) = ST(i) op ST ; DE: same then pop. Intel's DC/DE sub/div are reversed.
                let ar = match reg {
                    4 => Arith::SubR,
                    5 => Arith::Sub,
                    6 => Arith::DivR,
                    7 => Arith::Div,
                    r => Arith::from(r),
                };
                if op == 6 && reg == 3 && i == 1 {
                    // FCOMPP
                    if let (Some(a), Some(b)) = (self.src(0), self.src(1)) {
                        self.compare(a, b);
                    }
                    self.pop();
                    self.pop();
                    return (50, 0);
                }
                if let (Some(a), Some(b)) = (self.src(i), self.src(0)) {
                    match ar {
                        Arith::Com | Arith::Comp => {
                            self.compare(b, a);
                            if matches!(ar, Arith::Comp) {
                                self.pop();
                            }
                        }
                        _ => {
                            if let Some(r) = self.arith(ar, a, b) {
                                self.set_st(i, r);
                            }
                        }
                    }
                }
                if op == 6 && !matches!(ar, Arith::Com | Arith::Comp) {
                    self.pop();
                }
                if op == 6 {
                    [90, 100, 45, 47, 90, 90, 202, 202][reg as usize]
                }
                else {
                    [85, 97, 45, 47, 85, 85, 198, 198][reg as usize]
                }
            }
            1 => self.d9_reg(reg, i),
            3 => match (reg, i) {
                (4, 0) => {
                    // FENI
                    self.cw &= !0x80;
                    5
                }
                (4, 1) => {
                    // FDISI
                    self.cw |= 0x80;
                    self.irq = false;
                    5
                }
                (4, 2) => {
                    // FCLEX
                    self.sw &= !(0x80FF);
                    self.irq = false;
                    5
                }
                (4, 3) => {
                    self.finit();
                    5
                }
                _ => {
                    self.stats.unknown += 1;
                    10
                }
            },
            5 => match reg {
                0 => {
                    // FFREE
                    let p = self.phys(i);
                    self.set_tag(p, TAG_EMPTY);
                    12
                }
                1 => {
                    // undocumented FXCH
                    self.fxch(i);
                    12
                }
                2 | 3 => {
                    if let Some(v) = self.src(0) {
                        self.set_st(i, v);
                    }
                    if reg == 3 {
                        self.pop();
                    }
                    if reg == 2 { 18 } else { 20 }
                }
                _ => {
                    self.stats.unknown += 1;
                    10
                }
            },
            _ => {
                // DA (no register forms on the 8087), DF
                if op == 7 && reg == 0 {
                    // undocumented FFREEP
                    let p = self.phys(i);
                    self.set_tag(p, TAG_EMPTY);
                    self.pop();
                    12
                }
                else {
                    self.stats.unknown += 1;
                    10
                }
            }
        };
        (c, 0)
    }

    /// FMUL is fast (90-105 clocks) when one significand has at most 24 significant bits,
    /// else 130-145.
    fn short_mul(a: F80, b: F80) -> bool {
        let lo = |v: F80| (v.to_bits() as u64) & 0xFF_FFFF_FFFF == 0;
        lo(a) || lo(b)
    }

    fn fxch(&mut self, i: usize) {
        let (a, b) = (self.src(0), self.src(i));
        if let (Some(a), Some(b)) = (a, b) {
            self.set_st(0, b);
            self.set_st(i, a);
        }
    }

    fn d9_reg(&mut self, reg: u8, i: usize) -> u32 {
        match reg {
            0 => {
                // FLD ST(i)
                if let Some(v) = self.src(i) {
                    self.push(v);
                }
                20
            }
            1 => {
                self.fxch(i);
                12
            }
            2 => 13, // FNOP (D9 D0)
            3 => {
                // undocumented FSTP1
                if let Some(v) = self.src(0) {
                    self.set_st(i, v);
                }
                self.pop();
                20
            }
            4 => match i {
                0 => {
                    if let Some(v) = self.src(0) {
                        self.set_st(0, -v);
                    }
                    15
                }
                1 => {
                    if let Some(v) = self.src(0) {
                        self.set_st(0, v.abs());
                    }
                    14
                }
                4 => {
                    if let Some(v) = self.src(0) {
                        self.compare(v, F80::ZERO);
                    }
                    42
                }
                5 => {
                    self.fxam();
                    17
                }
                _ => {
                    self.stats.unknown += 1;
                    10
                }
            },
            5 => {
                // constants
                let (v, c) = match i {
                    0 => (F80::from_u128(1).value, 18),
                    1 => (F80::from_bits(0x4000_D49A_784B_CD1B_8AFE), 19), // log2(10)
                    2 => (F80::from_bits(0x3FFF_B8AA_3B29_5C17_F0BC), 18), // log2(e)
                    3 => (F80::from_bits(0x4000_C90F_DAA2_2168_C235), 19), // pi
                    4 => (F80::from_bits(0x3FFD_9A20_9A84_FBCF_F799), 21), // log10(2)
                    5 => (F80::from_bits(0x3FFE_B172_17F7_D1CF_79AC), 20), // ln(2)
                    6 => (F80::ZERO, 14),
                    _ => {
                        self.stats.unknown += 1;
                        (indefinite(), 10)
                    }
                };
                self.push(v);
                c
            }
            6 => match i {
                0 => {
                    // F2XM1. The real 8087 takes -x for x < 0 (outside the documented
                    // 0 .. 0.5) and negates: F2XM1(-0.5) = -(2^0.5 - 1). Exact for 0.
                    let mut c = 618;
                    if let Some(x) = self.src(0) {
                        let neg = x.is_negative() && !x.is_zero();
                        let ax = if neg { -x } else { x };
                        let mut r = self.from_q(trans::exp2m1(to_q(ax)));
                        if neg {
                            r = -r;
                        }
                        self.set_st(0, r);
                        if x.is_zero() {
                            c = 45;
                        }
                        else {
                            self.sw |= SW_PE;
                            c = busy_from_c(interp(ax.ilogb(), &F2XM1_C), 618, 707.9);
                        }
                    }
                    c
                }
                1 => {
                    // FYL2X: ST1 = ST1 * log2(ST0), pop. A power of two is a short path on
                    // the real chip: exact, no precision flag, ~130 clocks.
                    let mut c = 1078;
                    if let (Some(x), Some(y)) = (self.src(0), self.src(1)) {
                        let xf = f80_to_f64(x);
                        if xf <= 0.0 {
                            self.exception(if xf == 0.0 { SW_ZE } else { SW_IE });
                        }
                        let pow2 = xf > 0.0 && x.is_finite() && x.to_bits() as u64 == 1 << 63;
                        let r = if pow2 {
                            c = 175;
                            let p = y.mul_r(F80::from_i128(x.ilogb() as i128).value, self.round());
                            if p.status.contains(Status::INEXACT) {
                                self.sw |= SW_PE;
                            }
                            p.value
                        }
                        else {
                            self.sw |= SW_PE;
                            if xf > 0.0 && xf.is_finite() {
                                // time by how far the mantissa (folded to 0.707 .. 1.414) is from 1
                                let mut m = (x.to_bits() as u64) as f64 / 2f64.powi(63);
                                if m > std::f64::consts::SQRT_2 {
                                    m /= 2.0;
                                }
                                let e = (m - 1.0).abs().log2().floor().max(-60.0) as i32;
                                let t: &[(i32, f64)] = if m >= 1.0 { &FYL2X_ABOVE_C } else { &FYL2X_BELOW_C };
                                c = busy_from_c(interp(e, t), 1078, 1110.9);
                            }
                            self.from_q(trans::mul_q(to_q(y), trans::log2(to_q(x))))
                        };
                        self.set_st(1, r);
                    }
                    self.pop();
                    c
                }
                2 => {
                    // FPTAN: replace ST with Y, push X; Y/X = tan(ST). 0 < |ST| < 2 the way
                    // the chip computes it (fptan.rs: its partial pair and time), zero exact,
                    // anything larger (outside the documented range) as tan with X = 1.
                    let mut clocks = 527;
                    if let Some(x) = self.src(0) {
                        if x.is_zero() {
                            self.push(F80::from_u128(1).value);
                            clocks = fptan::CLK_ZERO;
                        }
                        else if let Some((y, xr, c)) = fptan::fptan(x.to_bits()) {
                            self.set_st(0, F80::from_bits(y));
                            self.push(F80::from_bits(xr));
                            self.sw |= SW_PE;
                            clocks = c;
                        }
                        else {
                            self.set_st(0, self.from_q(trans::tan(to_q(x))));
                            self.push(F80::from_u128(1).value);
                            self.sw |= SW_PE;
                        }
                    }
                    clocks
                }
                3 => {
                    // FPATAN: ST1 = atan(ST1/ST0), pop. With 0 < x < y (outside the documented
                    // 0 <= y < x) the real 8087 leaves y as the result in ~130 clocks.
                    let mut c = 715;
                    if let (Some(x), Some(y)) = (self.src(0), self.src(1)) {
                        let (xf, yf) = (f80_to_f64(x), f80_to_f64(y));
                        if xf > 0.0 && yf > xf && x.is_finite() && y.is_finite() {
                            c = 183;
                        }
                        else {
                            let r = self.from_q(trans::atan2(to_q(y), to_q(x)));
                            self.set_st(1, r);
                            if xf != 0.0 && yf != 0.0 && xf.is_finite() && yf.is_finite() {
                                let e = (yf / xf).abs().log2().floor() as i32;
                                c = busy_from_c(interp(e, &FPATAN_C), 715, 713.0);
                            }
                        }
                        self.sw |= SW_PE;
                    }
                    self.pop();
                    c
                }
                4 => {
                    // FXTRACT: ST = exponent, push significand
                    if let Some(x) = self.src(0) {
                        let e = x.ilogb();
                        let sig = x.scalbn(-e);
                        self.set_st(0, F80::from_i128(e as i128).value);
                        self.push(sig);
                    }
                    50
                }
                6 => {
                    // FDECSTP
                    self.set_top((self.top() + 7) & 7);
                    9
                }
                7 => {
                    self.set_top((self.top() + 1) & 7);
                    9
                }
                _ => {
                    self.stats.unknown += 1;
                    10
                }
            },
            _ => match i {
                // reg 7
                0 => {
                    // FPREM: partial remainder (chopped quotient)
                    if let (Some(a), Some(b)) = (self.src(0), self.src(1)) {
                        let ea = a.ilogb();
                        let eb = b.ilogb();
                        self.sw &= !SW_CC;
                        if ea - eb >= 64 {
                            // incomplete reduction: reduce by 2^(d-63) steps like the real chip
                            let shift = ea - eb - 63;
                            let bs = b.scalbn(shift);
                            let r = a.c_fmod(bs).value;
                            self.set_st(0, r);
                            self.sw |= SW_C2;
                        }
                        else {
                            let q = a.div_r(b, Round::TowardZero).value.round_to_integral(Round::TowardZero).value;
                            let r = a.c_fmod(b).value;
                            let qi = q.abs().c_fmod(F80::from_u128(8).value).value.to_i128(8).value;
                            if qi & 1 != 0 {
                                self.sw |= SW_C1;
                            }
                            if qi & 2 != 0 {
                                self.sw |= SW_C3;
                            }
                            if qi & 4 != 0 {
                                self.sw |= SW_C0;
                            }
                            self.set_st(0, r);
                        }
                    }
                    125
                }
                1 => {
                    // FYL2XP1
                    let mut c = 790;
                    if let (Some(x), Some(y)) = (self.src(0), self.src(1)) {
                        let r = self.from_q(trans::mul_q(to_q(y), trans::log2_1p(to_q(x))));
                        self.set_st(1, r);
                        self.sw |= SW_PE;
                        let xf = f80_to_f64(x);
                        if xf != 0.0 && xf.is_finite() {
                            let e = xf.abs().log2().floor().max(-60.0) as i32;
                            let neg = if xf < 0.0 { 155.0 } else { 0.0 };
                            c = busy_from_c(interp(e, &FYL2XP1_C) - neg, 790, 793.7);
                        }
                    }
                    self.pop();
                    c
                }
                2 => {
                    // FSQRT
                    if let Some(x) = self.src(0) {
                        let r = self.sqrt(x);
                        if let Some(v) = r {
                            self.set_st(0, v);
                        }
                    }
                    183
                }
                4 => {
                    // FRNDINT
                    if let Some(x) = self.src(0) {
                        let r = x.round_to_integral(self.round());
                        if let Some(v) = self.result(r) {
                            self.set_st(0, v);
                        }
                    }
                    45
                }
                5 => {
                    // FSCALE: ST = ST * 2^trunc(ST1)
                    if let (Some(x), Some(y)) = (self.src(0), self.src(1)) {
                        let n = y.to_i128_r(32, Round::TowardZero, &mut false).value;
                        let n = n.clamp(-70000, 70000) as i32;
                        self.set_st(0, x.scalbn_r(n, self.round()));
                    }
                    35
                }
                _ => {
                    self.stats.unknown += 1;
                    10
                }
            },
        }
    }

    fn sqrt(&mut self, x: F80) -> Option<F80> {
        if x.is_zero() || x.is_nan() || x.is_pos_infinity() {
            return Some(x);
        }
        if x.is_negative() {
            if self.exception(SW_IE) {
                return None;
            }
            return Some(indefinite());
        }
        // Start from the double result (scaled into range), then one Newton step in 80 bits.
        let e = x.ilogb() & !1;
        let xs = x.scalbn(-e);
        let y0 = f64_to_f80(f80_to_f64(xs).sqrt());
        let half = F80::from_bits(0x3FFE_8000_0000_0000_0000);
        let q = xs.div_r(y0, Round::NearestTiesToEven).value;
        let y1 = y0.add_r(q, Round::NearestTiesToEven).value.mul_r(half, Round::NearestTiesToEven).value;
        let r = y1.scalbn(e / 2);
        let mut f = SW_PE;
        let sq = y1.mul_r(y1, Round::NearestTiesToEven);
        if sq.status == Status::OK && sq.value.bitwise_eq(xs) {
            f = 0;
        }
        if f != 0 {
            self.exception(f);
        }
        Some(r)
    }

    fn fxam(&mut self) {
        self.sw &= !SW_CC;
        let p = self.phys(0);
        let v = self.st[p];
        if v.is_negative() {
            self.sw |= SW_C1;
        }
        if self.tag(p) == TAG_EMPTY {
            self.sw |= SW_C3 | SW_C0;
            return;
        }
        let bits = v.to_bits();
        let exp = (bits >> 64) & 0x7FFF;
        let int_bit = (bits >> 63) & 1;
        if v.is_nan() {
            self.sw |= SW_C0;
        }
        else if v.is_infinite() {
            self.sw |= SW_C2 | SW_C0;
        }
        else if v.is_zero() {
            self.sw |= SW_C3;
        }
        else if exp == 0 {
            self.sw |= SW_C3 | SW_C2; // denormal
        }
        else if int_bit == 0 {
            // unnormal: C3=C2=C0=0
        }
        else {
            self.sw |= SW_C2;
        }
    }
}

/// Times GRiDBench 2.0 measured on a real 1101 ("c": clocks scaled by AAM = 83, the way its
/// report prints them), by the exponent of the argument (F2XM1) or of y/x (FPATAN).
const F2XM1_C: [(i32, f64); 7] = [(-16, 380.0), (-12, 461.0), (-10, 505.0), (-7, 575.0), (-4, 650.0), (-2, 704.0), (-1, 705.0)];
/// FYL2X by the exponent of |m - 1| (m = mantissa folded to 0.707 .. 1.414), above and
/// below 1; FYL2XP1 by the exponent of x (x < 0 about 155 less).
const FYL2X_ABOVE_C: [(i32, f64); 3] = [(-24, 706.0), (-3, 1079.0), (-2, 1105.0)];
const FYL2X_BELOW_C: [(i32, f64); 4] = [(-24, 549.0), (-6, 864.0), (-4, 923.0), (-2, 947.0)];
const FYL2XP1_C: [(i32, f64); 7] = [(-60, 665.0), (-16, 675.0), (-12, 764.0), (-10, 812.0), (-7, 905.0), (-4, 969.0), (-2, 1023.0)];
const FPATAN_C: [(i32, f64); 7] = [(-16, 380.0), (-12, 466.0), (-10, 510.0), (-4, 695.0), (-2, 760.0), (-1, 713.0), (0, 713.0)];

/// Piecewise linear in e, flat outside the table.
fn interp(e: i32, t: &[(i32, f64)]) -> f64 {
    if e <= t[0].0 {
        return t[0].1;
    }
    for w in t.windows(2) {
        let ((e0, c0), (e1, c1)) = (w[0], w[1]);
        if e <= e1 {
            return c0 + (c1 - c0) * (e - e0) as f64 / (e1 - e0) as f64;
        }
    }
    t[t.len() - 1].1
}

/// Busy clocks that make GRiDBench print `c` in the emulator, from one known pair (busy
/// `b0` prints `c0`); about one c per busy clock for the long instructions.
fn busy_from_c(c: f64, b0: u32, c0: f64) -> u32 {
    (b0 as f64 + (c - c0)).round().max(20.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ram(Vec<u8>);
    impl FpuMem for Ram {
        fn read_u8(&mut self, a: u32) -> u8 {
            self.0[a as usize]
        }
        fn write_u8(&mut self, a: u32, v: u8) {
            self.0[a as usize] = v;
        }
    }

    fn put_f64(m: &mut Ram, a: usize, v: f64) {
        m.0[a..a + 8].copy_from_slice(&v.to_le_bytes());
    }

    fn get_f64(m: &Ram, a: usize) -> f64 {
        f64::from_le_bytes(m.0[a..a + 8].try_into().unwrap())
    }

    #[test]
    fn arithmetic_and_store() {
        let mut m = Ram(vec![0; 4096]);
        let mut f = I8087::new();
        put_f64(&mut m, 0x100, 1.5);
        put_f64(&mut m, 0x108, 2.25);
        f.esc(&mut m, 0, 0xDD, 0x06, Some(0x100), 0); // FLD qword [100]
        f.esc(&mut m, 0, 0xDC, 0x0E, Some(0x108), 0); // FMUL qword [108]
        f.esc(&mut m, 0, 0xDD, 0x1E, Some(0x110), 0); // FSTP qword [110]
        assert_eq!(get_f64(&m, 0x110), 3.375);
        // FLDPI, FSQRT
        f.esc(&mut m, 0, 0xD9, 0xEB, None, 0);
        f.esc(&mut m, 0, 0xD9, 0xFA, None, 0);
        assert!((f.st_f64(0).unwrap() - std::f64::consts::PI.sqrt()).abs() < 1e-15);
        // FILD word, FISTP word with rounding
        m.0[0x200] = 7;
        m.0[0x201] = 0;
        f.esc(&mut m, 0, 0xDF, 0x06, Some(0x200), 0); // FILD
        f.esc(&mut m, 0, 0xD8, 0xC9, None, 0); // FMUL ST,ST(1) -> 7*sqrt(pi)
        f.esc(&mut m, 0, 0xDF, 0x1E, Some(0x202), 0); // FISTP word
        assert_eq!(i16::from_le_bytes([m.0[0x202], m.0[0x203]]), 12); // 12.407 -> 12
    }

    #[test]
    fn compare_and_status() {
        let mut m = Ram(vec![0; 4096]);
        let mut f = I8087::new();
        f.esc(&mut m, 0, 0xD9, 0xE8, None, 0); // FLD1
        f.esc(&mut m, 0, 0xD9, 0xEE, None, 0); // FLDZ
        f.esc(&mut m, 0, 0xDE, 0xD9, None, 0); // FCOMPP: 0 vs 1 -> less, C0
        f.esc(&mut m, 0, 0xDD, 0x3E, Some(0x300), 0); // FSTSW
        let sw = u16::from_le_bytes([m.0[0x300], m.0[0x301]]);
        assert_eq!(sw & 0x4500, 0x0100);
        assert_eq!(f.tw, 0xFFFF);
    }

    #[test]
    fn busy_timing() {
        let mut m = Ram(vec![0; 64]);
        let mut f = I8087::new();
        f.esc(&mut m, 1000, 0xD9, 0xE8, None, 0); // FLD1, 18 clocks
        assert!(f.busy(1017));
        assert!(!f.busy(1018));
        f.esc(&mut m, 1005, 0xD9, 0xE8, None, 0); // queued behind the first
        assert_eq!(f.busy_until(), 1036);
    }
}

/// The register stack as raw 80-bit patterns (for saved machine states).
mod f80_regs {
    use super::F80;
    use rustc_apfloat::Float;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(st: &[F80; 8], s: S) -> Result<S::Ok, S::Error> {
        let bits: Vec<u128> = st.iter().map(|f| f.to_bits()).collect();
        bits.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[F80; 8], D::Error> {
        let bits = Vec::<u128>::deserialize(d)?;
        let mut st = [F80::ZERO; 8];
        for (r, b) in st.iter_mut().zip(bits) {
            *r = F80::from_bits(b);
        }
        Ok(st)
    }
}
