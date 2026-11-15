//! A fast 8086 interpreter for build mode: whole instructions at a time, with approximate
//! clock counts (no bus cycles, prefetch queue or wait states). Registers, flags and memory
//! behave like a real 8086, including the undocumented opcodes and quirks the SingleStepTests
//! 8086 suite checks (aliases, POP CS, SALC, SETMO, IDIV limits, REP + MUL/IDIV sign flip).
//! Used by `GridMachine` when fast mode is on; never for timing measurements.

/// Polls of TEST (5 clocks each) one pass of a WAIT makes before it starts over. No 8087
/// instruction is busy that long; only a held BUSY gets there.
const WAIT_POLLS: u32 = 4096;

/// What the core needs from the machine around it.
pub trait Bus86 {
    fn read8(&mut self, addr: u32) -> u8;
    fn write8(&mut self, addr: u32, v: u8);
    fn io_in8(&mut self, port: u16) -> u8;
    fn io_out8(&mut self, port: u16, v: u8);
    /// The interrupt controller's INTR line.
    fn intr(&mut self) -> bool {
        false
    }
    /// Interrupt acknowledge: the vector.
    fn inta(&mut self) -> u8 {
        0
    }
    /// ESC (D8-DF): `ea` = linear address of the memory operand. Returns extra clocks.
    fn esc(&mut self, _now: u64, _opcode: u8, _modrm: u8, _ea: Option<u32>, _ip: u32) -> u32 {
        0
    }
    /// Coprocessor BUSY (TEST pin) at clock `now`.
    fn fpu_busy(&mut self, _now: u64) -> bool {
        false
    }
    /// Shortest WAIT, in clocks.
    fn wait_min(&mut self) -> u64 {
        0
    }
    /// The bus can hold an access (READY low): see `stalled`.
    const WAITS: bool = false;
    /// The last access could not complete yet (READY low). The core then puts the
    /// instruction back (a REP string instruction keeps the iterations it has done) so it runs
    /// again, after any pending interrupt, once the bus is ready.
    fn stalled(&mut self) -> bool {
        false
    }
}

// register numbers (ModRM order)
pub const AX: usize = 0;
pub const CX: usize = 1;
pub const DX: usize = 2;
pub const BX: usize = 3;
pub const SP: usize = 4;
pub const BP: usize = 5;
pub const SI: usize = 6;
pub const DI: usize = 7;
pub const ES: usize = 0;
pub const CS: usize = 1;
pub const SS: usize = 2;
pub const DS: usize = 3;

const CF: u16 = 0x0001;
const PF: u16 = 0x0004;
const AF: u16 = 0x0010;
const ZF: u16 = 0x0040;
const SF: u16 = 0x0080;
const TF: u16 = 0x0100;
const IF: u16 = 0x0200;
const DF: u16 = 0x0400;
const OF: u16 = 0x0800;

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Fast86 {
    pub r: [u16; 8],
    pub s: [u16; 4],
    pub ip: u16,
    pub flags: u16,
    /// Clock count (approximate, 5 MHz clocks).
    pub clocks: u64,
    pub halted: bool,
    pub instructions: u64,
    /// No interrupt after this instruction (MOV/POP to a segment register, prefixes).
    inhibit: bool,
    /// A REP string instruction stopped on a held access, keeping its progress.
    #[serde(default)]
    paused: bool,
}

#[derive(Clone, Copy)]
enum Rm {
    Reg(usize),
    Mem(u16, u16), // segment value, offset
}

struct Ctx {
    seg: Option<usize>,
    rep: u8, // 0, 0xF2 (REPNE), 0xF3 (REP/REPE)
    start_ip: u16,
    lin_start: u32,
}

#[inline]
fn lin(seg: u16, off: u16) -> u32 {
    (((seg as u32) << 4) + off as u32) & 0xFFFFF
}

fn parity(v: u8) -> bool {
    v.count_ones() % 2 == 0
}

impl Fast86 {
    pub fn new() -> Fast86 {
        Fast86 { flags: 0xF002, s: [0, 0xFFFF, 0, 0], ..Default::default() }
    }

    #[inline]
    fn r8(&self, n: usize) -> u8 {
        if n < 4 {
            self.r[n] as u8
        }
        else {
            (self.r[n - 4] >> 8) as u8
        }
    }
    #[inline]
    fn set_r8(&mut self, n: usize, v: u8) {
        if n < 4 {
            self.r[n] = (self.r[n] & 0xFF00) | v as u16;
        }
        else {
            self.r[n - 4] = (self.r[n - 4] & 0x00FF) | ((v as u16) << 8);
        }
    }
    #[inline]
    fn f(&self, m: u16) -> bool {
        self.flags & m != 0
    }
    #[inline]
    fn setf(&mut self, m: u16, on: bool) {
        if on {
            self.flags |= m
        }
        else {
            self.flags &= !m
        }
    }

    fn fetch8<B: Bus86>(&mut self, b: &mut B) -> u8 {
        let v = b.read8(lin(self.s[CS], self.ip));
        self.ip = self.ip.wrapping_add(1);
        v
    }
    fn fetch16<B: Bus86>(&mut self, b: &mut B) -> u16 {
        let lo = self.fetch8(b) as u16;
        let hi = self.fetch8(b) as u16;
        lo | hi << 8
    }

    fn rd16<B: Bus86>(&self, b: &mut B, seg: u16, off: u16) -> u16 {
        b.read8(lin(seg, off)) as u16 | (b.read8(lin(seg, off.wrapping_add(1))) as u16) << 8
    }
    fn wr16<B: Bus86>(&self, b: &mut B, seg: u16, off: u16, v: u16) {
        b.write8(lin(seg, off), v as u8);
        b.write8(lin(seg, off.wrapping_add(1)), (v >> 8) as u8);
    }

    fn push<B: Bus86>(&mut self, b: &mut B, v: u16) {
        self.r[SP] = self.r[SP].wrapping_sub(2);
        let (ss, sp) = (self.s[SS], self.r[SP]);
        self.wr16(b, ss, sp, v);
    }
    fn pop<B: Bus86>(&mut self, b: &mut B) -> u16 {
        let v = self.rd16(b, self.s[SS], self.r[SP]);
        self.r[SP] = self.r[SP].wrapping_add(2);
        v
    }

    /// Decode a ModRM byte: (reg field, operand).
    fn modrm<B: Bus86>(&mut self, b: &mut B, c: &Ctx, m: u8) -> (usize, Rm) {
        let md = m >> 6;
        let reg = ((m >> 3) & 7) as usize;
        let rm = (m & 7) as usize;
        if md == 3 {
            return (reg, Rm::Reg(rm));
        }
        let (base, def_ss) = match rm {
            0 => (self.r[BX].wrapping_add(self.r[SI]), false),
            1 => (self.r[BX].wrapping_add(self.r[DI]), false),
            2 => (self.r[BP].wrapping_add(self.r[SI]), true),
            3 => (self.r[BP].wrapping_add(self.r[DI]), true),
            4 => (self.r[SI], false),
            5 => (self.r[DI], false),
            6 => {
                if md == 0 {
                    (0, false)
                }
                else {
                    (self.r[BP], true)
                }
            }
            _ => (self.r[BX], false),
        };
        let disp = match md {
            0 if rm == 6 => self.fetch16(b),
            0 => 0,
            1 => self.fetch8(b) as i8 as i16 as u16,
            _ => self.fetch16(b),
        };
        let seg = c.seg.unwrap_or(if def_ss { SS } else { DS });
        self.clocks += 7;
        (reg, Rm::Mem(self.s[seg], base.wrapping_add(disp)))
    }

    fn get8<B: Bus86>(&self, b: &mut B, o: Rm) -> u8 {
        match o {
            Rm::Reg(n) => self.r8(n),
            Rm::Mem(s, a) => b.read8(lin(s, a)),
        }
    }
    fn put8<B: Bus86>(&mut self, b: &mut B, o: Rm, v: u8) {
        match o {
            Rm::Reg(n) => self.set_r8(n, v),
            Rm::Mem(s, a) => b.write8(lin(s, a), v),
        }
    }
    fn get16<B: Bus86>(&self, b: &mut B, o: Rm) -> u16 {
        match o {
            Rm::Reg(n) => self.r[n],
            Rm::Mem(s, a) => self.rd16(b, s, a),
        }
    }
    fn put16<B: Bus86>(&mut self, b: &mut B, o: Rm, v: u16) {
        match o {
            Rm::Reg(n) => self.r[n] = v,
            Rm::Mem(s, a) => self.wr16(b, s, a, v),
        }
    }

    // ---- flags helpers ----
    fn szp(&mut self, v: u32, w: bool) {
        let (mask, sign) = if w { (0xFFFF, 0x8000) } else { (0xFF, 0x80) };
        let v = v & mask;
        self.setf(ZF, v == 0);
        self.setf(SF, v & sign != 0);
        self.setf(PF, parity(v as u8));
    }

    /// The eight ALU operations (ADD OR ADC SBB AND SUB XOR CMP); returns the result.
    fn alu(&mut self, op: usize, a: u32, b: u32, w: bool) -> u32 {
        let (mask, sign) = if w { (0xFFFFu32, 0x8000u32) } else { (0xFF, 0x80) };
        let cin = self.f(CF) as u32;
        let r = match op {
            0 | 2 => {
                let c = if op == 2 { cin } else { 0 };
                let r = a + b + c;
                self.setf(CF, r > mask);
                self.setf(AF, (a ^ b ^ r) & 0x10 != 0);
                self.setf(OF, (r ^ a) & (r ^ b) & sign != 0);
                r & mask
            }
            3 | 5 | 7 => {
                let c = if op == 3 { cin } else { 0 };
                let r = a.wrapping_sub(b).wrapping_sub(c);
                self.setf(CF, (b + c) > a);
                self.setf(AF, (a ^ b ^ r) & 0x10 != 0);
                self.setf(OF, (a ^ b) & (a ^ r) & sign != 0);
                r & mask
            }
            1 | 4 | 6 => {
                let r = match op {
                    1 => a | b,
                    4 => a & b,
                    _ => a ^ b,
                };
                self.setf(CF, false);
                self.setf(OF, false);
                self.setf(AF, false);
                r & mask
            }
            _ => unreachable!(),
        };
        self.szp(r, w);
        r
    }

    fn inc_dec(&mut self, v: u32, dec: bool, w: bool) -> u32 {
        let cf = self.f(CF);
        let r = self.alu(if dec { 5 } else { 0 }, v, 1, w);
        self.setf(CF, cf);
        r
    }

    /// Shifts and rotates (D0-D3), `op` = reg field; count as the 8086 takes it (not masked).
    fn shift(&mut self, op: usize, v: u32, count: u8, w: bool) -> u32 {
        if count == 0 {
            return v;
        }
        let (bits, sign) = if w { (16u32, 0x8000u32) } else { (8, 0x80) };
        let mask = (1u32 << bits) - 1;
        let mut v = v & mask;
        let mut cf = self.f(CF);
        for _ in 0..count {
            match op {
                0 => {
                    // ROL
                    cf = v & sign != 0;
                    v = ((v << 1) | cf as u32) & mask;
                }
                1 => {
                    // ROR
                    cf = v & 1 != 0;
                    v = (v >> 1) | if cf { sign } else { 0 };
                }
                2 => {
                    // RCL
                    let c = v & sign != 0;
                    v = ((v << 1) | cf as u32) & mask;
                    cf = c;
                }
                3 => {
                    // RCR
                    let c = v & 1 != 0;
                    v = (v >> 1) | if cf { sign } else { 0 };
                    cf = c;
                }
                4 | 6 => {
                    // SHL / SETMO-alias handled by caller
                    cf = v & sign != 0;
                    v = (v << 1) & mask;
                }
                5 => {
                    cf = v & 1 != 0;
                    v >>= 1;
                }
                _ => {
                    // SAR
                    cf = v & 1 != 0;
                    v = (v >> 1) | (v & sign);
                }
            }
        }
        self.setf(CF, cf);
        let msb = v & sign != 0;
        match op {
            0 | 2 | 4 | 6 => self.setf(OF, msb != cf),
            1 | 3 => self.setf(OF, ((v << 1) ^ v) & sign != 0),
            5 => {
                // OF = msb of the operand before the last step
                let before = (v << 1) | cf as u32;
                self.setf(OF, before & sign != 0 && msb != (before & sign != 0) || (before & sign != 0) != msb);
            }
            _ => self.setf(OF, false),
        }
        if op >= 4 {
            self.szp(v, w);
            self.setf(AF, op != 7 && false);
        }
        v
    }

    /// Non-maskable interrupt (vector 2).
    pub fn nmi<B: Bus86>(&mut self, b: &mut B) {
        self.interrupt(b, 2);
    }

    fn interrupt<B: Bus86>(&mut self, b: &mut B, n: u8) {
        let f = self.flags;
        self.push(b, f);
        self.setf(IF, false);
        self.setf(TF, false);
        let cs = self.s[CS];
        self.push(b, cs);
        let ip = self.ip;
        self.push(b, ip);
        let v = (n as u16) * 4;
        self.ip = self.rd16(b, 0, v);
        self.s[CS] = self.rd16(b, 0, v + 2);
        self.clocks += 51;
        self.halted = false;
    }

    /// Run one instruction (with its prefixes), then take a pending interrupt.
    pub fn step<B: Bus86>(&mut self, b: &mut B) {
        if self.halted {
            if self.f(IF) && b.intr() {
                self.halted = false;
            }
            else {
                self.clocks += 4;
                return;
            }
        }
        else {
            let tf = self.f(TF);
            let saved = if B::WAITS { Some(*self) } else { None };
            self.exec(b);
            if B::WAITS && b.stalled() {
                // READY low: undo the instruction (a REP string instruction keeps what it did),
                // then take a pending interrupt as the real CPU would between iterations.
                if self.paused {
                    self.paused = false;
                }
                else if let Some(s) = saved {
                    *self = s;
                }
            }
            else {
                self.paused = false;
                self.instructions += 1;
                if tf && self.f(TF) {
                    self.interrupt(b, 1);
                }
            }
        }
        if self.inhibit {
            self.inhibit = false;
            return;
        }
        if self.f(IF) && b.intr() {
            let v = b.inta();
            self.interrupt(b, v);
        }
    }

    fn exec<B: Bus86>(&mut self, b: &mut B) {
        let mut c = Ctx { seg: None, rep: 0, start_ip: self.ip, lin_start: lin(self.s[CS], self.ip) };
        let mut op;
        loop {
            op = self.fetch8(b);
            match op {
                0x26 => c.seg = Some(ES),
                0x2E => c.seg = Some(CS),
                0x36 => c.seg = Some(SS),
                0x3E => c.seg = Some(DS),
                0xF0 | 0xF1 => {}
                0xF2 | 0xF3 => c.rep = op,
                _ => break,
            }
            self.clocks += 2;
        }
        self.clocks += 3;
        match op {
            // ALU r/m, reg / reg, r/m / acc, imm
            0x00..=0x3F if op & 7 < 6 => {
                let alu = (op >> 3) as usize;
                let w = op & 1 != 0;
                match op & 7 {
                    0..=3 => {
                        let m = self.fetch8(b);
                        let (reg, rm) = self.modrm(b, &c, m);
                        let to_reg = op & 2 != 0;
                        if w {
                            let (x, y) = if to_reg { (self.r[reg], self.get16(b, rm)) } else { (self.get16(b, rm), self.r[reg]) };
                            let r = self.alu(alu, x as u32, y as u32, true) as u16;
                            if alu != 7 {
                                if to_reg {
                                    self.r[reg] = r
                                }
                                else {
                                    self.put16(b, rm, r)
                                }
                            }
                        }
                        else {
                            let (x, y) = if to_reg { (self.r8(reg), self.get8(b, rm)) } else { (self.get8(b, rm), self.r8(reg)) };
                            let r = self.alu(alu, x as u32, y as u32, false) as u8;
                            if alu != 7 {
                                if to_reg {
                                    self.set_r8(reg, r)
                                }
                                else {
                                    self.put8(b, rm, r)
                                }
                            }
                        }
                    }
                    4 => {
                        let i = self.fetch8(b);
                        let r = self.alu(alu, self.r8(0) as u32, i as u32, false) as u8;
                        if alu != 7 {
                            self.set_r8(0, r);
                        }
                    }
                    _ => {
                        let i = self.fetch16(b);
                        let r = self.alu(alu, self.r[AX] as u32, i as u32, true) as u16;
                        if alu != 7 {
                            self.r[AX] = r;
                        }
                    }
                }
            }
            0x06 | 0x0E | 0x16 | 0x1E => {
                let v = self.s[(op >> 3) as usize];
                self.push(b, v);
                self.clocks += 7;
            }
            0x07 | 0x0F | 0x17 | 0x1F => {
                let v = self.pop(b);
                self.s[(op >> 3) as usize] = v;
                self.inhibit = true;
                self.clocks += 5;
            }
            0x27 => {
                // DAA (8086 quirks as measured by MartyPC: 0x9F check with AF, OF pattern)
                let al = self.r8(0);
                let (cf, af) = (self.f(CF), self.f(AF));
                let of = if cf { (0x1A..=0x7F).contains(&al) } else { (0x7A..=0x7F).contains(&al) };
                let mut v = al;
                if al & 0x0F > 9 || af {
                    v = v.wrapping_add(6);
                }
                self.setf(AF, al & 0x0F > 9 || af);
                let hi = al > if af { 0x9F } else { 0x99 } || cf;
                if hi {
                    v = v.wrapping_add(0x60);
                }
                self.setf(CF, hi);
                self.setf(OF, of);
                self.set_r8(0, v);
                self.szp(v as u32, false);
            }
            0x2F => {
                // DAS
                let al = self.r8(0);
                let (cf, af) = (self.f(CF), self.f(AF));
                let of = match (af, cf) {
                    (false, false) => (0x9A..=0xDF).contains(&al),
                    (true, false) => (0x80..=0x85).contains(&al) || (0xA0..=0xE5).contains(&al),
                    (false, true) => (0x80..=0xDF).contains(&al),
                    (true, true) => (0x80..=0xE5).contains(&al),
                };
                let mut v = al;
                if al & 0x0F > 9 || af {
                    v = v.wrapping_sub(6);
                }
                self.setf(AF, al & 0x0F > 9 || af);
                let hi = al > if af { 0x9F } else { 0x99 } || cf;
                if hi {
                    v = v.wrapping_sub(0x60);
                }
                self.setf(CF, hi);
                self.setf(OF, of);
                self.set_r8(0, v);
                self.szp(v as u32, false);
            }
            0x37 | 0x3F => {
                // AAA / AAS
                let al = self.r8(0);
                let aaa = op == 0x37;
                if al & 0x0F > 9 || self.f(AF) {
                    let ax = self.r[AX];
                    let nal = if aaa { al.wrapping_add(6) } else { al.wrapping_sub(6) };
                    let nah = if aaa { (ax >> 8) as u8 + 1 } else { ((ax >> 8) as u8).wrapping_sub(1) };
                    self.szp(nal as u32, false);
                    self.setf(OF, if aaa { (al ^ nal) & nal & 0x80 != 0 } else { (al ^ nal) & al & 0x80 != 0 });
                    self.r[AX] = ((nah as u16) << 8) | (nal & 0x0F) as u16;
                    self.setf(AF, true);
                    self.setf(CF, true);
                }
                else {
                    self.szp(al as u32, false);
                    self.setf(OF, false);
                    self.set_r8(0, al & 0x0F);
                    self.setf(AF, false);
                    self.setf(CF, false);
                }
            }
            0x40..=0x47 => {
                let n = (op & 7) as usize;
                self.r[n] = self.inc_dec(self.r[n] as u32, false, true) as u16;
            }
            0x48..=0x4F => {
                let n = (op & 7) as usize;
                self.r[n] = self.inc_dec(self.r[n] as u32, true, true) as u16;
            }
            0x50..=0x57 => {
                let n = (op & 7) as usize;
                // PUSH SP pushes the new SP on the 8086
                let v = if n == SP { self.r[SP].wrapping_sub(2) } else { self.r[n] };
                self.push(b, v);
                self.clocks += 8;
            }
            0x58..=0x5F => {
                let v = self.pop(b);
                self.r[(op & 7) as usize] = v;
                self.clocks += 5;
            }
            0x60..=0x7F => {
                let d = self.fetch8(b) as i8 as i16 as u16;
                if self.cond(op & 0x0F) {
                    self.ip = self.ip.wrapping_add(d);
                    self.clocks += 12;
                }
            }
            0x80..=0x83 => {
                let w = op & 1 != 0;
                let m = self.fetch8(b);
                let (reg, rm) = self.modrm(b, &c, m);
                if w {
                    let x = self.get16(b, rm);
                    let i = if op == 0x83 { self.fetch8(b) as i8 as i16 as u16 } else { self.fetch16(b) };
                    let r = self.alu(reg, x as u32, i as u32, true) as u16;
                    if reg != 7 {
                        self.put16(b, rm, r);
                    }
                }
                else {
                    let x = self.get8(b, rm);
                    let i = self.fetch8(b);
                    let r = self.alu(reg, x as u32, i as u32, false) as u8;
                    if reg != 7 {
                        self.put8(b, rm, r);
                    }
                }
            }
            0x84 | 0x85 => {
                let m = self.fetch8(b);
                let (reg, rm) = self.modrm(b, &c, m);
                if op & 1 != 0 {
                    let x = self.get16(b, rm);
                    self.alu(4, x as u32, self.r[reg] as u32, true);
                }
                else {
                    let x = self.get8(b, rm);
                    self.alu(4, x as u32, self.r8(reg) as u32, false);
                }
            }
            0x86 | 0x87 => {
                let m = self.fetch8(b);
                let (reg, rm) = self.modrm(b, &c, m);
                if op & 1 != 0 {
                    let x = self.get16(b, rm);
                    let y = self.r[reg];
                    self.put16(b, rm, y);
                    self.r[reg] = x;
                }
                else {
                    let x = self.get8(b, rm);
                    let y = self.r8(reg);
                    self.put8(b, rm, y);
                    self.set_r8(reg, x);
                }
            }
            0x88..=0x8B => {
                let m = self.fetch8(b);
                let (reg, rm) = self.modrm(b, &c, m);
                match op {
                    0x88 => {
                        let v = self.r8(reg);
                        self.put8(b, rm, v)
                    }
                    0x89 => {
                        let v = self.r[reg];
                        self.put16(b, rm, v)
                    }
                    0x8A => {
                        let v = self.get8(b, rm);
                        self.set_r8(reg, v)
                    }
                    _ => {
                        let v = self.get16(b, rm);
                        self.r[reg] = v
                    }
                }
            }
            0x8C => {
                let m = self.fetch8(b);
                let (reg, rm) = self.modrm(b, &c, m);
                let v = self.s[reg & 3];
                self.put16(b, rm, v);
            }
            0x8D => {
                let m = self.fetch8(b);
                let (reg, rm) = self.modrm(b, &c, m);
                match rm {
                    Rm::Mem(_, a) => self.r[reg] = a,
                    // LEA with a register operand: the 8086 uses the last computed address
                    Rm::Reg(_) => {}
                }
            }
            0x8E => {
                let m = self.fetch8(b);
                let (reg, rm) = self.modrm(b, &c, m);
                let v = self.get16(b, rm);
                self.s[reg & 3] = v;
                self.inhibit = true;
            }
            0x8F => {
                let m = self.fetch8(b);
                // the operand address uses SP after the pop
                let v = self.pop(b);
                let (_, rm) = self.modrm(b, &c, m);
                self.put16(b, rm, v);
            }
            0x90..=0x97 => {
                let n = (op & 7) as usize;
                self.r.swap(AX, n);
            }
            0x98 => self.r[AX] = self.r8(0) as i8 as i16 as u16,
            0x99 => self.r[DX] = if self.r[AX] & 0x8000 != 0 { 0xFFFF } else { 0 },
            0x9A => {
                let ip = self.fetch16(b);
                let cs = self.fetch16(b);
                let (ocs, oip) = (self.s[CS], self.ip);
                self.push(b, ocs);
                self.push(b, oip);
                self.s[CS] = cs;
                self.ip = ip;
                self.clocks += 28;
            }
            0x9B => {
                // WAIT
                let (mut n, start, min) = (0, self.clocks, b.wait_min());
                while (b.fpu_busy(self.clocks) || self.clocks - start < min) && n < WAIT_POLLS {
                    self.clocks += 5;
                    n += 1;
                }
                if n == WAIT_POLLS && b.fpu_busy(self.clocks) {
                    // BUSY held for good (an unmasked 8087 exception nobody cleared): the
                    // 8086 stays in the WAIT. It takes interrupts there and returns to it.
                    self.ip = c.start_ip;
                }
            }
            0x9C => {
                let f = self.flags | 0xF002;
                self.push(b, f);
            }
            0x9D => {
                let f = self.pop(b);
                self.flags = (f & 0x0FD5) | 0xF002;
            }
            0x9E => self.flags = (self.flags & 0xFF00) | (self.r8(4) as u16 & 0xD5) | 0x02,
            0x9F => {
                let f = self.flags as u8;
                self.set_r8(4, f);
            }
            0xA0..=0xA3 => {
                let a = self.fetch16(b);
                let s = self.s[c.seg.unwrap_or(DS)];
                match op {
                    0xA0 => {
                        let v = b.read8(lin(s, a));
                        self.set_r8(0, v)
                    }
                    0xA1 => self.r[AX] = self.rd16(b, s, a),
                    0xA2 => b.write8(lin(s, a), self.r8(0)),
                    _ => {
                        let v = self.r[AX];
                        self.wr16(b, s, a, v)
                    }
                }
                self.clocks += 7;
            }
            0xA4..=0xA7 | 0xAA..=0xAF => self.string(b, &c, op),
            0xA8 => {
                let i = self.fetch8(b);
                self.alu(4, self.r8(0) as u32, i as u32, false);
            }
            0xA9 => {
                let i = self.fetch16(b);
                self.alu(4, self.r[AX] as u32, i as u32, true);
            }
            0xB0..=0xB7 => {
                let i = self.fetch8(b);
                self.set_r8((op & 7) as usize, i);
            }
            0xB8..=0xBF => {
                let i = self.fetch16(b);
                self.r[(op & 7) as usize] = i;
            }
            0xC0 | 0xC2 => {
                let n = self.fetch16(b);
                self.ip = self.pop(b);
                self.r[SP] = self.r[SP].wrapping_add(n);
                self.clocks += 16;
            }
            0xC1 | 0xC3 => {
                self.ip = self.pop(b);
                self.clocks += 16;
            }
            0xC4 | 0xC5 => {
                let m = self.fetch8(b);
                let (reg, rm) = self.modrm(b, &c, m);
                let (s, a) = match rm {
                    Rm::Mem(s, a) => (s, a),
                    // register form: the 8086 reads from the last address; rarely used
                    Rm::Reg(_) => (self.s[DS], 0),
                };
                self.r[reg] = self.rd16(b, s, a);
                let v = self.rd16(b, s, a.wrapping_add(2));
                self.s[if op == 0xC4 { ES } else { DS }] = v;
                self.clocks += 8;
            }
            0xC6 => {
                let m = self.fetch8(b);
                let (_, rm) = self.modrm(b, &c, m);
                let i = self.fetch8(b);
                self.put8(b, rm, i);
            }
            0xC7 => {
                let m = self.fetch8(b);
                let (_, rm) = self.modrm(b, &c, m);
                let i = self.fetch16(b);
                self.put16(b, rm, i);
            }
            0xC8 | 0xCA => {
                let n = self.fetch16(b);
                self.ip = self.pop(b);
                self.s[CS] = self.pop(b);
                self.r[SP] = self.r[SP].wrapping_add(n);
                self.clocks += 25;
            }
            0xC9 | 0xCB => {
                self.ip = self.pop(b);
                self.s[CS] = self.pop(b);
                self.clocks += 24;
            }
            0xCC => self.interrupt(b, 3),
            0xCD => {
                let n = self.fetch8(b);
                self.interrupt(b, n);
            }
            0xCE => {
                if self.f(OF) {
                    self.interrupt(b, 4);
                }
            }
            0xCF => {
                self.ip = self.pop(b);
                self.s[CS] = self.pop(b);
                let f = self.pop(b);
                self.flags = (f & 0x0FD5) | 0xF002;
                self.clocks += 24;
            }
            0xD0..=0xD3 => {
                let w = op & 1 != 0;
                let m = self.fetch8(b);
                let (reg, rm) = self.modrm(b, &c, m);
                let count = if op & 2 != 0 { self.r8(1) } else { 1 };
                self.clocks += 4 * count as u64;
                if w {
                    let v = self.get16(b, rm) as u32;
                    let r = if reg == 6 { self.setmo(v, count, true) } else { self.shift(reg, v, count, true) };
                    self.put16(b, rm, r as u16);
                }
                else {
                    let v = self.get8(b, rm) as u32;
                    let r = if reg == 6 { self.setmo(v, count, false) } else { self.shift(reg, v, count, false) };
                    self.put8(b, rm, r as u8);
                }
            }
            0xD4 => {
                // AAM
                let base = self.fetch8(b);
                self.clocks += 80;
                if base == 0 {
                    self.szp(0, false); // flags as the 8086 leaves them for AL = 0... approx
                    self.interrupt(b, 0);
                }
                else {
                    let al = self.r8(0);
                    self.r[AX] = (((al / base) as u16) << 8) | (al % base) as u16;
                    self.szp((al % base) as u32, false);
                    self.setf(OF, false);
                    self.setf(AF, false);
                    self.setf(CF, false);
                }
            }
            0xD5 => {
                // AAD
                let base = self.fetch8(b);
                let (al, ah) = (self.r8(0), self.r8(4));
                let prod = (ah as u32 * base as u32) & 0xFF;
                let r = self.alu(0, al as u32, prod, false) as u8;
                self.r[AX] = r as u16;
                self.clocks += 60;
            }
            0xD6 => {
                // SALC
                let v = if self.f(CF) { 0xFF } else { 0 };
                self.set_r8(0, v);
            }
            0xD7 => {
                let s = self.s[c.seg.unwrap_or(DS)];
                let a = self.r[BX].wrapping_add(self.r8(0) as u16);
                let v = b.read8(lin(s, a));
                self.set_r8(0, v);
                self.clocks += 8;
            }
            0xD8..=0xDF => {
                let m = self.fetch8(b);
                let (_, rm) = self.modrm(b, &c, m);
                let ea = match rm {
                    Rm::Mem(s, a) => {
                        // the 8086 does a dummy word read of the operand
                        let _ = self.rd16(b, s, a);
                        Some(lin(s, a))
                    }
                    Rm::Reg(_) => None,
                };
                // the 8087 latches the address of the ESC byte itself, not of a prefix before it
                let extra = b.esc(self.clocks, op, m, ea, (c.lin_start + c.seg.is_some() as u32) & 0xFFFFF);
                self.clocks += 2 + extra as u64;
            }
            0xE0..=0xE3 => {
                let d = self.fetch8(b) as i8 as i16 as u16;
                let jump = if op == 0xE3 {
                    self.r[CX] == 0
                }
                else {
                    self.r[CX] = self.r[CX].wrapping_sub(1);
                    let nz = self.r[CX] != 0;
                    match op {
                        0xE0 => nz && !self.f(ZF),
                        0xE1 => nz && self.f(ZF),
                        _ => nz,
                    }
                };
                if jump {
                    self.ip = self.ip.wrapping_add(d);
                    self.clocks += 14;
                }
            }
            0xE4 | 0xE5 | 0xEC | 0xED => {
                let port = if op < 0xEC { self.fetch8(b) as u16 } else { self.r[DX] };
                if op & 1 != 0 {
                    let lo = b.io_in8(port) as u16;
                    let hi = b.io_in8(port.wrapping_add(1)) as u16;
                    self.r[AX] = lo | hi << 8;
                }
                else {
                    let v = b.io_in8(port);
                    self.set_r8(0, v);
                }
                self.clocks += 8;
            }
            0xE6 | 0xE7 | 0xEE | 0xEF => {
                let port = if op < 0xEE { self.fetch8(b) as u16 } else { self.r[DX] };
                b.io_out8(port, self.r8(0));
                if op & 1 != 0 {
                    b.io_out8(port.wrapping_add(1), self.r8(4));
                }
                self.clocks += 8;
            }
            0xE8 => {
                let d = self.fetch16(b);
                let ip = self.ip;
                self.push(b, ip);
                self.ip = self.ip.wrapping_add(d);
                self.clocks += 16;
            }
            0xE9 => {
                let d = self.fetch16(b);
                self.ip = self.ip.wrapping_add(d);
                self.clocks += 12;
            }
            0xEA => {
                let ip = self.fetch16(b);
                let cs = self.fetch16(b);
                self.ip = ip;
                self.s[CS] = cs;
                self.clocks += 12;
            }
            0xEB => {
                let d = self.fetch8(b) as i8 as i16 as u16;
                self.ip = self.ip.wrapping_add(d);
                self.clocks += 12;
            }
            0xF4 => self.halted = true,
            0xF5 => self.flags ^= CF,
            0xF6 | 0xF7 => self.group3(b, &c, op),
            0xF8 => self.setf(CF, false),
            0xF9 => self.setf(CF, true),
            0xFA => self.setf(IF, false),
            0xFB => self.setf(IF, true),
            0xFC => self.setf(DF, false),
            0xFD => self.setf(DF, true),
            0xFE | 0xFF => self.group45(b, &c, op),
            _ => {
                // prefixes are consumed above; nothing else is left
                let _ = c.start_ip;
            }
        }
    }

    /// The undocumented SETMO/SETMOC (D0-D3 /6): sets the operand to all ones.
    fn setmo(&mut self, v: u32, count: u8, w: bool) -> u32 {
        if count == 0 {
            return v;
        }
        let r = if w { 0xFFFF } else { 0xFF };
        self.setf(CF, false);
        self.setf(OF, false);
        self.setf(AF, false);
        self.szp(r, w);
        r
    }

    fn cond(&self, cc: u8) -> bool {
        let r = match cc >> 1 {
            0 => self.f(OF),
            1 => self.f(CF),
            2 => self.f(ZF),
            3 => self.f(CF) || self.f(ZF),
            4 => self.f(SF),
            5 => self.f(PF),
            6 => self.f(SF) != self.f(OF),
            _ => self.f(ZF) || (self.f(SF) != self.f(OF)),
        };
        r != (cc & 1 != 0)
    }

    fn string<B: Bus86>(&mut self, b: &mut B, c: &Ctx, op: u8) {
        let w = op & 1 != 0;
        let step: u16 = if w { 2 } else { 1 };
        let delta = if self.f(DF) { step.wrapping_neg() } else { step };
        let src = self.s[c.seg.unwrap_or(DS)];
        let cmp = matches!(op, 0xA6 | 0xA7 | 0xAE | 0xAF);
        loop {
            if c.rep != 0 {
                if self.r[CX] == 0 {
                    break;
                }
            }
            match op {
                0xA4 | 0xA5 => {
                    if w {
                        let v = self.rd16(b, src, self.r[SI]);
                        let (es, di) = (self.s[ES], self.r[DI]);
                        self.wr16(b, es, di, v);
                    }
                    else {
                        let v = b.read8(lin(src, self.r[SI]));
                        if B::WAITS && b.stalled() {
                            self.pause(c);
                            return;
                        }
                        b.write8(lin(self.s[ES], self.r[DI]), v);
                        if B::WAITS && b.stalled() {
                            self.pause(c);
                            return;
                        }
                    }
                    self.r[SI] = self.r[SI].wrapping_add(delta);
                    self.r[DI] = self.r[DI].wrapping_add(delta);
                }
                0xA6 | 0xA7 => {
                    let (x, y) = if w {
                        (self.rd16(b, src, self.r[SI]) as u32, self.rd16(b, self.s[ES], self.r[DI]) as u32)
                    }
                    else {
                        (b.read8(lin(src, self.r[SI])) as u32, b.read8(lin(self.s[ES], self.r[DI])) as u32)
                    };
                    self.alu(7, x, y, w);
                    self.r[SI] = self.r[SI].wrapping_add(delta);
                    self.r[DI] = self.r[DI].wrapping_add(delta);
                }
                0xAA | 0xAB => {
                    if w {
                        let (es, di, v) = (self.s[ES], self.r[DI], self.r[AX]);
                        self.wr16(b, es, di, v);
                    }
                    else {
                        b.write8(lin(self.s[ES], self.r[DI]), self.r8(0));
                        if B::WAITS && b.stalled() {
                            self.pause(c);
                            return;
                        }
                    }
                    self.r[DI] = self.r[DI].wrapping_add(delta);
                }
                0xAC | 0xAD => {
                    if w {
                        self.r[AX] = self.rd16(b, src, self.r[SI]);
                    }
                    else {
                        let v = b.read8(lin(src, self.r[SI]));
                        if B::WAITS && b.stalled() {
                            self.pause(c);
                            return;
                        }
                        self.set_r8(0, v);
                    }
                    self.r[SI] = self.r[SI].wrapping_add(delta);
                }
                _ => {
                    // SCAS
                    let y = if w { self.rd16(b, self.s[ES], self.r[DI]) as u32 } else { b.read8(lin(self.s[ES], self.r[DI])) as u32 };
                    let x = if w { self.r[AX] as u32 } else { self.r8(0) as u32 };
                    self.alu(7, x, y, w);
                    self.r[DI] = self.r[DI].wrapping_add(delta);
                }
            }
            self.clocks += if w { 14 } else { 10 };
            if c.rep == 0 {
                break;
            }
            self.r[CX] = self.r[CX].wrapping_sub(1);
            if cmp {
                let z = self.f(ZF);
                if (c.rep == 0xF3 && !z) || (c.rep == 0xF2 && z) {
                    break;
                }
            }
            // A real 8086 takes a pending interrupt between iterations and then resumes the
            // instruction (only modelled where the bus can hold accesses: the 2101's 8088).
            if B::WAITS && self.r[CX] != 0 && self.f(IF) && b.intr() {
                self.ip = c.start_ip;
                return;
            }
        }
    }

    /// Stop a string instruction on a held access: it runs again from its first prefix.
    fn pause(&mut self, c: &Ctx) {
        self.ip = c.start_ip;
        self.paused = true;
    }

    fn group3<B: Bus86>(&mut self, b: &mut B, c: &Ctx, op: u8) {
        let w = op & 1 != 0;
        let m = self.fetch8(b);
        let (reg, rm) = self.modrm(b, c, m);
        // the 8086 negates MUL/IMUL/IDIV results with a REP prefix
        let negate = c.rep != 0;
        match reg {
            0 | 1 => {
                if w {
                    let x = self.get16(b, rm);
                    let i = self.fetch16(b);
                    self.alu(4, x as u32, i as u32, true);
                }
                else {
                    let x = self.get8(b, rm);
                    let i = self.fetch8(b);
                    self.alu(4, x as u32, i as u32, false);
                }
            }
            2 => {
                if w {
                    let v = !self.get16(b, rm);
                    self.put16(b, rm, v)
                }
                else {
                    let v = !self.get8(b, rm);
                    self.put8(b, rm, v)
                }
            }
            3 => {
                if w {
                    let v = self.get16(b, rm) as u32;
                    let r = self.alu(5, 0, v, true);
                    self.put16(b, rm, r as u16);
                }
                else {
                    let v = self.get8(b, rm) as u32;
                    let r = self.alu(5, 0, v, false);
                    self.put8(b, rm, r as u8);
                }
            }
            4 | 5 => {
                self.clocks += if w { 120 } else { 70 };
                let signed = reg == 5;
                if w {
                    let x = self.get16(b, rm);
                    let (mut r, hi_used) = if signed {
                        let p = (self.r[AX] as i16 as i32) * (x as i16 as i32);
                        (p as u32, p != (p as i16 as i32))
                    }
                    else {
                        let p = self.r[AX] as u32 * x as u32;
                        (p, p >> 16 != 0)
                    };
                    if negate && signed {
                        r = (r as i32).wrapping_neg() as u32;
                    }
                    self.r[AX] = r as u16;
                    self.r[DX] = (r >> 16) as u16;
                    self.setf(CF, hi_used);
                    self.setf(OF, hi_used);
                    self.szp(self.r[DX] as u32, true);
                    self.setf(ZF, false);
                }
                else {
                    let x = self.get8(b, rm);
                    let (mut r, hi_used) = if signed {
                        let p = (self.r8(0) as i8 as i16) * (x as i8 as i16);
                        (p as u16, p != (p as i8 as i16))
                    }
                    else {
                        let p = self.r8(0) as u16 * x as u16;
                        (p, p >> 8 != 0)
                    };
                    if negate && signed {
                        r = (r as i16).wrapping_neg() as u16;
                    }
                    self.r[AX] = r;
                    self.setf(CF, hi_used);
                    self.setf(OF, hi_used);
                    self.szp((r >> 8) as u32, false);
                    self.setf(ZF, false);
                }
            }
            _ => {
                self.clocks += if w { 150 } else { 85 };
                let signed = reg == 7;
                let ok = if w {
                    let d = self.get16(b, rm);
                    let n = ((self.r[DX] as u32) << 16) | self.r[AX] as u32;
                    if signed {
                        let (n, d) = (n as i32 as i64, d as i16 as i64);
                        if d == 0 {
                            false
                        }
                        else {
                            let mut q = n / d;
                            let r = n % d;
                            if negate {
                                q = -q;
                            }
                            if q > 0x7FFF || q < -0x7FFF {
                                false
                            }
                            else {
                                self.r[AX] = q as u16;
                                self.r[DX] = r as u16;
                                true
                            }
                        }
                    }
                    else if d == 0 || n / d as u32 > 0xFFFF {
                        false
                    }
                    else {
                        self.r[AX] = (n / d as u32) as u16;
                        self.r[DX] = (n % d as u32) as u16;
                        true
                    }
                }
                else {
                    let d = self.get8(b, rm);
                    let n = self.r[AX];
                    if signed {
                        let (n, d) = (n as i16 as i32, d as i8 as i32);
                        if d == 0 {
                            false
                        }
                        else {
                            let mut q = n / d;
                            let r = n % d;
                            if negate {
                                q = -q;
                            }
                            if q > 0x7F || q < -0x7F {
                                false
                            }
                            else {
                                self.r[AX] = ((r as u8 as u16) << 8) | q as u8 as u16;
                                true
                            }
                        }
                    }
                    else if d == 0 || n / d as u16 > 0xFF {
                        false
                    }
                    else {
                        self.r[AX] = (((n % d as u16) as u16) << 8) | (n / d as u16);
                        true
                    }
                };
                if !ok {
                    self.interrupt(b, 0);
                }
            }
        }
    }

    fn group45<B: Bus86>(&mut self, b: &mut B, c: &Ctx, op: u8) {
        let m = self.fetch8(b);
        let (reg, rm) = self.modrm(b, c, m);
        if op == 0xFE {
            // INC/DEC byte (reg 2-7 are undefined on the 8086)
            let v = self.get8(b, rm) as u32;
            let r = self.inc_dec(v, reg & 1 != 0, false);
            self.put8(b, rm, r as u8);
            return;
        }
        match reg {
            0 | 1 => {
                let v = self.get16(b, rm) as u32;
                let r = self.inc_dec(v, reg == 1, true);
                self.put16(b, rm, r as u16);
            }
            2 => {
                let t = self.get16(b, rm);
                let ip = self.ip;
                self.push(b, ip);
                self.ip = t;
                self.clocks += 16;
            }
            3 => {
                if let Rm::Mem(s, a) = rm {
                    let ip = self.rd16(b, s, a);
                    let cs = self.rd16(b, s, a.wrapping_add(2));
                    let (ocs, oip) = (self.s[CS], self.ip);
                    self.push(b, ocs);
                    self.push(b, oip);
                    self.ip = ip;
                    self.s[CS] = cs;
                }
                self.clocks += 30;
            }
            4 => {
                self.ip = self.get16(b, rm);
                self.clocks += 12;
            }
            5 => {
                if let Rm::Mem(s, a) = rm {
                    self.ip = self.rd16(b, s, a);
                    self.s[CS] = self.rd16(b, s, a.wrapping_add(2));
                }
                self.clocks += 20;
            }
            _ => {
                // PUSH r/m (6, and 7 as an alias); PUSH SP-form pushes the new SP
                let v = match rm {
                    Rm::Reg(SP) => self.r[SP].wrapping_sub(2),
                    _ => self.get16(b, rm),
                };
                self.push(b, v);
                self.clocks += 10;
            }
        }
    }
}
