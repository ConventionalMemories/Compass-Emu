//! Intel 8051 (MCS-51) core for the GRiD 2102 floppy box.
//!
//! The whole instruction set with machine-cycle counts, 256 bytes of internal RAM, timers 0
//! and 1 (modes 0-3, timer mode only), the two external interrupts (edge or level) and the
//! two-level interrupt priority. No serial port (the 2102 doesn't use it).
//!
//! Ports follow the quasi-bidirectional pin model: an instruction that reads a port reads
//! latch AND the level the outside drives (`Bus51::pins`), except read-modify-write
//! instructions, which read the latch.

use serde::{Deserialize, Serialize};

/// What the core sees around it.
pub trait Bus51 {
    /// Program memory (PSEN).
    fn code(&mut self, a: u16) -> u8;
    /// External data memory read (MOVX, RD strobe).
    fn xread(&mut self, a: u16) -> u8;
    /// External data memory write (MOVX, WR strobe).
    fn xwrite(&mut self, a: u16, v: u8);
    /// Level the outside drives on port `port`'s pins (1 = high or not driven).
    fn pins(&mut self, port: usize) -> u8;
}

const ACC: u8 = 0xE0;
const B: u8 = 0xF0;
const PSW: u8 = 0xD0;
const SP: u8 = 0x81;
const DPL: u8 = 0x82;
const DPH: u8 = 0x83;
const P0: u8 = 0x80;
const P1: u8 = 0x90;
const P2: u8 = 0xA0;
const P3: u8 = 0xB0;
const TCON: u8 = 0x88;
const TMOD: u8 = 0x89;
const TL0: u8 = 0x8A;
const TL1: u8 = 0x8B;
const TH0: u8 = 0x8C;
const TH1: u8 = 0x8D;
const IE: u8 = 0xA8;
const IP: u8 = 0xB8;
const SCON: u8 = 0x98;

const CY: u8 = 0x80;
const AC: u8 = 0x40;
const OV: u8 = 0x04;

#[derive(Clone, Serialize, Deserialize)]
pub struct I8051 {
    pub pc: u16,
    pub iram: Vec<u8>,
    sfr: Vec<u8>,
    /// Machine cycles run so far.
    pub cycles: u64,
    /// An interrupt of low (0) / high (1) priority is being serviced.
    in_isr: [bool; 2],
    int_pin_prev: [bool; 2],
    /// One more instruction runs before an interrupt (after RETI or an IE/IP write).
    hold_int: bool,
}

impl Default for I8051 {
    fn default() -> Self {
        Self::new()
    }
}

impl I8051 {
    pub fn new() -> I8051 {
        let mut c = I8051 {
            pc: 0,
            iram: vec![0; 256],
            sfr: vec![0; 128],
            cycles: 0,
            in_isr: [false; 2],
            int_pin_prev: [true; 2],
            hold_int: false,
        };
        c.reset();
        c
    }

    pub fn reset(&mut self) {
        self.pc = 0;
        self.sfr.iter_mut().for_each(|v| *v = 0);
        self.sfr[(SP - 0x80) as usize] = 7;
        for p in [P0, P1, P2, P3] {
            self.sfr[(p - 0x80) as usize] = 0xFF;
        }
        self.in_isr = [false; 2];
        self.hold_int = false;
    }

    #[inline]
    fn s(&self, a: u8) -> u8 {
        self.sfr[(a & 0x7F) as usize]
    }
    #[inline]
    fn s_mut(&mut self, a: u8) -> &mut u8 {
        &mut self.sfr[(a & 0x7F) as usize]
    }

    /// Port latch (what the core drives; 1 = released / pulled up).
    pub fn latch(&self, port: usize) -> u8 {
        self.s(P0 + 0x10 * port as u8)
    }

    pub fn acc(&self) -> u8 {
        self.s(ACC)
    }

    fn a(&self) -> u8 {
        self.s(ACC)
    }
    fn set_a(&mut self, v: u8) {
        *self.s_mut(ACC) = v;
    }
    fn psw(&self) -> u8 {
        let p = (self.a().count_ones() & 1) as u8;
        (self.s(PSW) & !1) | p
    }
    fn cy(&self) -> bool {
        self.s(PSW) & CY != 0
    }
    fn set_cy(&mut self, c: bool) {
        let p = self.s_mut(PSW);
        *p = if c { *p | CY } else { *p & !CY };
    }
    fn dptr(&self) -> u16 {
        u16::from_le_bytes([self.s(DPL), self.s(DPH)])
    }
    fn set_dptr(&mut self, v: u16) {
        *self.s_mut(DPL) = v as u8;
        *self.s_mut(DPH) = (v >> 8) as u8;
    }
    fn bank(&self) -> usize {
        (self.s(PSW) as usize & 0x18) as usize
    }
    fn r(&self, n: u8) -> u8 {
        self.iram[self.bank() + n as usize]
    }
    fn set_r(&mut self, n: u8, v: u8) {
        let b = self.bank();
        self.iram[b + n as usize] = v;
    }

    /// Direct address read; `rmw` = read-modify-write (ports give the latch).
    fn rd(&mut self, bus: &mut dyn Bus51, a: u8, rmw: bool) -> u8 {
        if a < 0x80 {
            return self.iram[a as usize];
        }
        match a {
            PSW => self.psw(),
            P0 | P1 | P2 | P3 if !rmw => {
                let port = ((a - P0) >> 4) as usize;
                self.s(a) & bus.pins(port)
            }
            _ => self.s(a),
        }
    }

    fn wr(&mut self, a: u8, v: u8) {
        if a < 0x80 {
            self.iram[a as usize] = v;
            return;
        }
        if a == IE || a == IP {
            self.hold_int = true;
        }
        *self.s_mut(a) = v;
    }

    fn bit_addr(b: u8) -> (u8, u8) {
        if b < 0x80 {
            (0x20 + (b >> 3), 1 << (b & 7))
        }
        else {
            (b & 0xF8, 1 << (b & 7))
        }
    }

    fn rd_bit(&mut self, bus: &mut dyn Bus51, b: u8, rmw: bool) -> bool {
        let (a, m) = Self::bit_addr(b);
        self.rd(bus, a, rmw) & m != 0
    }

    fn wr_bit(&mut self, bus: &mut dyn Bus51, b: u8, v: bool) {
        let (a, m) = Self::bit_addr(b);
        let x = self.rd(bus, a, true);
        self.wr(a, if v { x | m } else { x & !m });
    }

    fn ind(&self, i: u8) -> u8 {
        self.iram[self.r(i) as usize]
    }
    fn set_ind(&mut self, i: u8, v: u8) {
        let a = self.r(i) as usize;
        self.iram[a] = v;
    }

    fn fetch(&mut self, bus: &mut dyn Bus51) -> u8 {
        let v = bus.code(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    fn push(&mut self, v: u8) {
        let sp = self.s(SP).wrapping_add(1);
        *self.s_mut(SP) = sp;
        self.iram[sp as usize] = v;
    }
    fn pop(&mut self) -> u8 {
        let sp = self.s(SP);
        *self.s_mut(SP) = sp.wrapping_sub(1);
        self.iram[sp as usize]
    }

    fn rel(&mut self, off: u8) {
        self.pc = self.pc.wrapping_add(off as i8 as u16);
    }

    fn add(&mut self, b: u8, carry: bool) {
        let a = self.a();
        let c = (carry && self.cy()) as u16;
        let r = a as u16 + b as u16 + c;
        let ac = (a & 0xF) as u16 + (b & 0xF) as u16 + c > 0xF;
        let ov = ((a ^ r as u8) & (b ^ r as u8) & 0x80) != 0;
        self.set_a(r as u8);
        self.flags(r > 0xFF, ac, ov);
    }

    fn subb(&mut self, b: u8) {
        let a = self.a();
        let c = self.cy() as i16;
        let r = a as i16 - b as i16 - c;
        let ac = ((a & 0xF) as i16) - ((b & 0xF) as i16) - c < 0;
        let ov = ((a ^ b) & (a ^ r as u8) & 0x80) != 0;
        self.set_a(r as u8);
        self.flags(r < 0, ac, ov);
    }

    fn flags(&mut self, cy: bool, ac: bool, ov: bool) {
        let p = self.s_mut(PSW);
        *p &= !(CY | AC | OV);
        if cy {
            *p |= CY;
        }
        if ac {
            *p |= AC;
        }
        if ov {
            *p |= OV;
        }
    }

    /// Operand of the regular rows (low nibble 6..F): @R0, @R1, R0..R7.
    fn op(&self, lo: u8) -> u8 {
        if lo < 8 {
            self.ind(lo & 1)
        }
        else {
            self.r(lo & 7)
        }
    }
    fn set_op(&mut self, lo: u8, v: u8) {
        if lo < 8 {
            self.set_ind(lo & 1, v)
        }
        else {
            self.set_r(lo & 7, v)
        }
    }

    /// Run one instruction (or take an interrupt). Returns machine cycles used.
    pub fn step(&mut self, bus: &mut dyn Bus51) -> u32 {
        if !self.hold_int {
            if let Some(c) = self.interrupt() {
                self.cycles += c as u64;
                self.timers(bus, c);
                self.sample_ints(bus);
                return c;
            }
        }
        self.hold_int = false;
        let c = self.exec(bus);
        self.cycles += c as u64;
        self.timers(bus, c);
        self.sample_ints(bus);
        c
    }

    fn interrupt(&mut self) -> Option<u32> {
        let ie = self.s(IE);
        if ie & 0x80 == 0 {
            return None;
        }
        let tcon = self.s(TCON);
        let scon = self.s(SCON);
        // (pending, IE/IP bit, vector, flag to clear in TCON)
        let srcs = [
            (tcon & 0x02 != 0, 0x01u8, 0x03u16, if tcon & 0x01 != 0 { 0x02u8 } else { 0 }),
            (tcon & 0x20 != 0, 0x02, 0x0B, 0x20),
            (tcon & 0x08 != 0, 0x04, 0x13, if tcon & 0x04 != 0 { 0x08 } else { 0 }),
            (tcon & 0x80 != 0, 0x08, 0x1B, 0x80),
            (scon & 0x03 != 0, 0x10, 0x23, 0),
        ];
        let ip = self.s(IP);
        for level in [1usize, 0] {
            if self.in_isr[1] || (level == 0 && self.in_isr[0]) {
                continue;
            }
            for &(pend, bit, vec, clr) in &srcs {
                let lv = if ip & bit != 0 { 1 } else { 0 };
                if pend && ie & bit != 0 && lv == level {
                    *self.s_mut(TCON) &= !clr;
                    self.in_isr[level] = true;
                    let pc = self.pc;
                    self.push(pc as u8);
                    self.push((pc >> 8) as u8);
                    self.pc = vec;
                    return Some(2);
                }
            }
        }
        None
    }

    fn sample_ints(&mut self, bus: &mut dyn Bus51) {
        let pins = self.s(P3) & bus.pins(3);
        for (i, (bit, it, ief)) in [(0x04u8, 0x01u8, 0x02u8), (0x08, 0x04, 0x08)].into_iter().enumerate() {
            let level = pins & bit != 0;
            let tcon = self.s(TCON);
            if tcon & it != 0 {
                if self.int_pin_prev[i] && !level {
                    *self.s_mut(TCON) |= ief;
                }
            }
            else if !level {
                *self.s_mut(TCON) |= ief;
            }
            else {
                *self.s_mut(TCON) &= !ief;
            }
            self.int_pin_prev[i] = level;
        }
    }

    fn timers(&mut self, bus: &mut dyn Bus51, c: u32) {
        let tcon = self.s(TCON);
        let tmod = self.s(TMOD);
        if tcon & 0x50 == 0 {
            return;
        }
        let p3 = self.s(P3) & bus.pins(3);
        for t in 0..2usize {
            let tr = tcon & (if t == 0 { 0x10 } else { 0x40 }) != 0;
            let m = (tmod >> (4 * t)) & 0x0F;
            let gate = m & 0x08 != 0;
            let ct = m & 0x04 != 0;
            if !tr || ct || (gate && p3 & (if t == 0 { 0x04 } else { 0x08 }) == 0) {
                continue;
            }
            let (tl, th, tf) = if t == 0 { (TL0, TH0, 0x20u8) } else { (TL1, TH1, 0x80u8) };
            let mode = m & 3;
            let mut over = false;
            match mode {
                0 => {
                    let v = ((self.s(th) as u32) << 5 | (self.s(tl) as u32 & 0x1F)) + c;
                    over = v > 0x1FFF;
                    let v = v & 0x1FFF;
                    *self.s_mut(th) = (v >> 5) as u8;
                    *self.s_mut(tl) = (self.s(tl) & 0xE0) | (v as u8 & 0x1F);
                }
                1 => {
                    let v = ((self.s(th) as u32) << 8 | self.s(tl) as u32) + c;
                    over = v > 0xFFFF;
                    *self.s_mut(th) = (v >> 8) as u8;
                    *self.s_mut(tl) = v as u8;
                }
                2 => {
                    let mut v = self.s(tl) as u32 + c;
                    while v > 0xFF {
                        over = true;
                        v = v - 0x100 + self.s(th) as u32;
                    }
                    *self.s_mut(tl) = v as u8;
                }
                _ => {
                    if t == 0 {
                        let v = self.s(TL0) as u32 + c;
                        over = v > 0xFF;
                        *self.s_mut(TL0) = v as u8;
                    }
                }
            }
            if over {
                *self.s_mut(TCON) |= tf;
            }
        }
    }

    fn exec(&mut self, bus: &mut dyn Bus51) -> u32 {
        let op = self.fetch(bus);
        let hi = op >> 4;
        let lo = op & 0x0F;
        // Regular rows: operand @Ri / Rn.
        if lo >= 6 {
            let v = self.op(lo);
            match hi {
                0x0 => self.set_op(lo, v.wrapping_add(1)),
                0x1 => self.set_op(lo, v.wrapping_sub(1)),
                0x2 => self.add(v, false),
                0x3 => self.add(v, true),
                0x4 => self.set_a(self.a() | v),
                0x5 => self.set_a(self.a() & v),
                0x6 => self.set_a(self.a() ^ v),
                0x7 => {
                    let d = self.fetch(bus);
                    self.set_op(lo, d);
                }
                0x8 => {
                    let d = self.fetch(bus);
                    self.wr(d, v);
                    return 2;
                }
                0x9 => self.subb(v),
                0xA => {
                    let d = self.fetch(bus);
                    let x = self.rd(bus, d, false);
                    self.set_op(lo, x);
                    return 2;
                }
                0xB => {
                    let d = self.fetch(bus);
                    let r = self.fetch(bus);
                    self.set_cy(v < d);
                    if v != d {
                        self.rel(r);
                    }
                    return 2;
                }
                0xC => {
                    let a = self.a();
                    self.set_a(v);
                    self.set_op(lo, a);
                }
                0xD => {
                    if lo < 8 {
                        // XCHD A,@Ri
                        let a = self.a();
                        self.set_a((a & 0xF0) | (v & 0x0F));
                        self.set_op(lo, (v & 0xF0) | (a & 0x0F));
                    }
                    else {
                        let r = self.fetch(bus);
                        let n = v.wrapping_sub(1);
                        self.set_op(lo, n);
                        if n != 0 {
                            self.rel(r);
                        }
                        return 2;
                    }
                }
                0xE => self.set_a(v),
                _ => {
                    let a = self.a();
                    self.set_op(lo, a);
                }
            }
            return 1;
        }
        match op {
            0x00 => 1,
            0x01 | 0x21 | 0x41 | 0x61 | 0x81 | 0xA1 | 0xC1 | 0xE1 => {
                let l = self.fetch(bus);
                self.pc = (self.pc & 0xF800) | ((op as u16 >> 5) << 8) | l as u16;
                2
            }
            0x11 | 0x31 | 0x51 | 0x71 | 0x91 | 0xB1 | 0xD1 | 0xF1 => {
                let l = self.fetch(bus);
                let pc = self.pc;
                self.push(pc as u8);
                self.push((pc >> 8) as u8);
                self.pc = (pc & 0xF800) | ((op as u16 >> 5) << 8) | l as u16;
                2
            }
            0x02 => {
                let h = self.fetch(bus);
                let l = self.fetch(bus);
                self.pc = u16::from_be_bytes([h, l]);
                2
            }
            0x12 => {
                let h = self.fetch(bus);
                let l = self.fetch(bus);
                let pc = self.pc;
                self.push(pc as u8);
                self.push((pc >> 8) as u8);
                self.pc = u16::from_be_bytes([h, l]);
                2
            }
            0x22 | 0x32 => {
                let h = self.pop();
                let l = self.pop();
                self.pc = u16::from_be_bytes([h, l]);
                if op == 0x32 {
                    if self.in_isr[1] {
                        self.in_isr[1] = false;
                    }
                    else {
                        self.in_isr[0] = false;
                    }
                    self.hold_int = true;
                }
                2
            }
            0x10 | 0x20 | 0x30 => {
                let b = self.fetch(bus);
                let r = self.fetch(bus);
                let v = self.rd_bit(bus, b, op == 0x10);
                let jump = if op == 0x30 { !v } else { v };
                if op == 0x10 && v {
                    self.wr_bit(bus, b, false);
                }
                if jump {
                    self.rel(r);
                }
                2
            }
            0x40 | 0x50 | 0x60 | 0x70 | 0x80 => {
                let r = self.fetch(bus);
                let jump = match op {
                    0x40 => self.cy(),
                    0x50 => !self.cy(),
                    0x60 => self.a() == 0,
                    0x70 => self.a() != 0,
                    _ => true,
                };
                if jump {
                    self.rel(r);
                }
                2
            }
            0x90 => {
                let h = self.fetch(bus);
                let l = self.fetch(bus);
                self.set_dptr(u16::from_be_bytes([h, l]));
                2
            }
            0xA0 | 0xB0 | 0x72 | 0x82 => {
                let b = self.fetch(bus);
                let v = self.rd_bit(bus, b, false);
                let c = self.cy();
                self.set_cy(match op {
                    0xA0 => c || !v,
                    0xB0 => c && !v,
                    0x72 => c || v,
                    _ => c && v,
                });
                2
            }
            0xC0 => {
                let d = self.fetch(bus);
                let v = self.rd(bus, d, false);
                self.push(v);
                2
            }
            0xD0 => {
                let d = self.fetch(bus);
                let v = self.pop();
                self.wr(d, v);
                2
            }
            0xE0 => {
                let v = bus.xread(self.dptr());
                self.set_a(v);
                2
            }
            0xF0 => {
                bus.xwrite(self.dptr(), self.a());
                2
            }
            0xE2 | 0xE3 => {
                let a = u16::from_be_bytes([self.s(P2), self.r(op & 1)]);
                let v = bus.xread(a);
                self.set_a(v);
                2
            }
            0xF2 | 0xF3 => {
                let a = u16::from_be_bytes([self.s(P2), self.r(op & 1)]);
                bus.xwrite(a, self.a());
                2
            }
            0x42 | 0x52 | 0x62 => {
                let d = self.fetch(bus);
                let x = self.rd(bus, d, true);
                let a = self.a();
                self.wr(d, alu(op >> 4, x, a));
                1
            }
            0x43 | 0x53 | 0x63 => {
                let d = self.fetch(bus);
                let i = self.fetch(bus);
                let x = self.rd(bus, d, true);
                self.wr(d, alu(op >> 4, x, i));
                2
            }
            0x92 => {
                let b = self.fetch(bus);
                let c = self.cy();
                self.wr_bit(bus, b, c);
                2
            }
            0xA2 => {
                let b = self.fetch(bus);
                let v = self.rd_bit(bus, b, false);
                self.set_cy(v);
                1
            }
            0xB2 => {
                let b = self.fetch(bus);
                let v = self.rd_bit(bus, b, true);
                self.wr_bit(bus, b, !v);
                1
            }
            0xC2 | 0xD2 => {
                let b = self.fetch(bus);
                self.wr_bit(bus, b, op == 0xD2);
                1
            }
            0x03 => {
                let a = self.a();
                self.set_a(a.rotate_right(1));
                1
            }
            0x13 => {
                let a = self.a();
                let c = self.cy();
                self.set_cy(a & 1 != 0);
                self.set_a((a >> 1) | if c { 0x80 } else { 0 });
                1
            }
            0x23 => {
                let a = self.a();
                self.set_a(a.rotate_left(1));
                1
            }
            0x33 => {
                let a = self.a();
                let c = self.cy();
                self.set_cy(a & 0x80 != 0);
                self.set_a((a << 1) | c as u8);
                1
            }
            0x73 => {
                self.pc = self.dptr().wrapping_add(self.a() as u16);
                2
            }
            0x83 => {
                let a = self.pc.wrapping_add(self.a() as u16);
                let v = bus.code(a);
                self.set_a(v);
                2
            }
            0x93 => {
                let a = self.dptr().wrapping_add(self.a() as u16);
                let v = bus.code(a);
                self.set_a(v);
                2
            }
            0xA3 => {
                let d = self.dptr().wrapping_add(1);
                self.set_dptr(d);
                2
            }
            0xB3 => {
                let c = self.cy();
                self.set_cy(!c);
                1
            }
            0xC3 | 0xD3 => {
                self.set_cy(op == 0xD3);
                1
            }
            0x04 => {
                self.set_a(self.a().wrapping_add(1));
                1
            }
            0x14 => {
                self.set_a(self.a().wrapping_sub(1));
                1
            }
            0x24 | 0x34 => {
                let i = self.fetch(bus);
                self.add(i, op == 0x34);
                1
            }
            0x44 | 0x54 | 0x64 => {
                let i = self.fetch(bus);
                self.set_a(alu(op >> 4, self.a(), i));
                1
            }
            0x74 => {
                let i = self.fetch(bus);
                self.set_a(i);
                1
            }
            0x84 => {
                let a = self.a();
                let b = self.s(B);
                let mut f = self.s(PSW) & !(CY | OV);
                if b == 0 {
                    f |= OV;
                }
                else {
                    self.set_a(a / b);
                    *self.s_mut(B) = a % b;
                }
                *self.s_mut(PSW) = f;
                4
            }
            0x94 => {
                let i = self.fetch(bus);
                self.subb(i);
                1
            }
            0xA4 => {
                let r = self.a() as u16 * self.s(B) as u16;
                self.set_a(r as u8);
                *self.s_mut(B) = (r >> 8) as u8;
                let mut f = self.s(PSW) & !(CY | OV);
                if r > 0xFF {
                    f |= OV;
                }
                *self.s_mut(PSW) = f;
                4
            }
            0xB4 | 0xB5 => {
                let x = self.fetch(bus);
                let r = self.fetch(bus);
                let v = if op == 0xB5 { self.rd(bus, x, false) } else { x };
                let a = self.a();
                self.set_cy(a < v);
                if a != v {
                    self.rel(r);
                }
                2
            }
            0xC4 => {
                self.set_a(self.a().rotate_left(4));
                1
            }
            0xD4 => {
                let mut a = self.a() as u16;
                let mut cy = self.cy();
                if a & 0x0F > 9 || self.s(PSW) & AC != 0 {
                    a += 6;
                    if a > 0xFF {
                        cy = true;
                    }
                }
                if (a >> 4) & 0x0F > 9 || cy || a > 0xFF {
                    a += 0x60;
                    if a > 0xFF {
                        cy = true;
                    }
                }
                self.set_a(a as u8);
                self.set_cy(cy);
                1
            }
            0xE4 => {
                self.set_a(0);
                1
            }
            0xF4 => {
                self.set_a(!self.a());
                1
            }
            0x05 | 0x15 => {
                let d = self.fetch(bus);
                let x = self.rd(bus, d, true);
                self.wr(d, if op == 0x05 { x.wrapping_add(1) } else { x.wrapping_sub(1) });
                1
            }
            0x25 | 0x35 => {
                let d = self.fetch(bus);
                let x = self.rd(bus, d, false);
                self.add(x, op == 0x35);
                1
            }
            0x45 | 0x55 | 0x65 => {
                let d = self.fetch(bus);
                let x = self.rd(bus, d, false);
                self.set_a(alu(op >> 4, self.a(), x));
                1
            }
            0x75 => {
                let d = self.fetch(bus);
                let i = self.fetch(bus);
                self.wr(d, i);
                2
            }
            0x85 => {
                let s = self.fetch(bus);
                let d = self.fetch(bus);
                let x = self.rd(bus, s, false);
                self.wr(d, x);
                2
            }
            0x95 => {
                let d = self.fetch(bus);
                let x = self.rd(bus, d, false);
                self.subb(x);
                1
            }
            0xC5 => {
                let d = self.fetch(bus);
                let x = self.rd(bus, d, false);
                let a = self.a();
                self.wr(d, a);
                self.set_a(x);
                1
            }
            0xD5 => {
                let d = self.fetch(bus);
                let r = self.fetch(bus);
                let x = self.rd(bus, d, true).wrapping_sub(1);
                self.wr(d, x);
                if x != 0 {
                    self.rel(r);
                }
                2
            }
            0xE5 => {
                let d = self.fetch(bus);
                let x = self.rd(bus, d, false);
                self.set_a(x);
                1
            }
            0xF5 => {
                let d = self.fetch(bus);
                let a = self.a();
                self.wr(d, a);
                1
            }
            // A5: reserved, runs as a 1-cycle NOP.
            _ => 1,
        }
    }
}

fn alu(row: u8, x: u8, y: u8) -> u8 {
    match row {
        4 => x | y,
        5 => x & y,
        _ => x ^ y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct T {
        rom: Vec<u8>,
        x: Vec<u8>,
        p1: u8,
    }
    impl Bus51 for T {
        fn code(&mut self, a: u16) -> u8 {
            *self.rom.get(a as usize).unwrap_or(&0)
        }
        fn xread(&mut self, a: u16) -> u8 {
            self.x[a as usize]
        }
        fn xwrite(&mut self, a: u16, v: u8) {
            self.x[a as usize] = v;
        }
        fn pins(&mut self, port: usize) -> u8 {
            if port == 1 {
                self.p1
            }
            else {
                0xFF
            }
        }
    }

    fn run(code: &[u8], steps: usize) -> (I8051, T) {
        let mut t = T { rom: code.to_vec(), x: vec![0; 65536], p1: 0xFF };
        let mut c = I8051::new();
        for _ in 0..steps {
            c.step(&mut t);
        }
        (c, t)
    }

    #[test]
    fn arithmetic_and_flags() {
        // MOV A,#99h; ADD A,#99h -> 32h CY OV AC; DA A -> 98h CY
        let (c, _) = run(&[0x74, 0x99, 0x24, 0x99, 0xD4], 3);
        assert_eq!(c.acc(), 0x98);
        assert!(c.cy());
        // MOV A,#10h; CLR C; SUBB A,#20h -> F0h CY
        let (c, _) = run(&[0x74, 0x10, 0xC3, 0x94, 0x20], 3);
        assert_eq!(c.acc(), 0xF0);
        assert!(c.cy());
        // MOV A,#12; MOV B,#5 (75 F0 05); DIV AB -> A=2 B=2
        let (c, _) = run(&[0x74, 12, 0x75, 0xF0, 5, 0x84], 3);
        assert_eq!((c.acc(), c.s(B)), (2, 2));
    }

    #[test]
    fn calls_movx_bits_ports() {
        // MOV DPTR,#1234h; MOV A,#5Ah; MOVX @DPTR,A; LCALL 0010h; ... 0010: SETB 20h.1 (D2 01); RET
        let mut code = vec![0x90, 0x12, 0x34, 0x74, 0x5A, 0xF0, 0x12, 0x00, 0x10, 0x00];
        code.resize(0x10, 0);
        code.extend([0xD2, 0x01, 0x22]);
        let (c, t) = run(&code, 6);
        assert_eq!(t.x[0x1234], 0x5A);
        assert_eq!(c.iram[0x20], 0x02);
        assert_eq!(c.pc, 0x0009);
        // P1 pins: latch FF, outside pulls bit 2 low: MOV A,P1 -> FB; ORL P1,#0 keeps latch FF
        let mut t = T { rom: vec![0xE5, 0x90, 0x43, 0x90, 0x00], x: vec![0; 65536], p1: 0xFB };
        let mut c = I8051::new();
        c.step(&mut t);
        assert_eq!(c.acc(), 0xFB);
        c.step(&mut t);
        assert_eq!(c.latch(1), 0xFF);
    }

    #[test]
    fn timer0_interrupt() {
        // 0000: MOV TMOD,#01; MOV TH0,#FFh; MOV TL0,#F0h; MOV IE,#82h; SETB TR0; SJMP $
        // 000B: INC 30h; RETI
        let mut code = vec![0x75, 0x89, 0x01, 0x75, 0x8C, 0xFF, 0x75, 0x8A, 0xF0, 0x80, 0x03];
        code.resize(0x0B, 0);
        code.extend([0x05, 0x30, 0x32]);
        code.resize(0x0E, 0);
        code[0x09] = 0x80;
        code[0x0A] = 0x03; // SJMP to 0x0E
        code.extend([0x75, 0xA8, 0x82, 0xD2, 0x8C, 0x80, 0xFE]);
        let (c, _) = run(&code, 40);
        assert_eq!(c.iram[0x30], 1);
    }
}
