//! Intel 8741A (UPI-41) microcontroller core, as used for the Compass keyboard.
//! MCS-48 instruction set plus the UPI host interface (DBB in/out, STS, IBF/OBF, F1 = A0,
//! EN FLAGS, JOBF/JNIBF). One machine cycle = 15 oscillator clocks.
//!
//! `new_8048` makes the same core an 8048/8749 (the internal modem's MCU): 128 bytes of RAM,
//! a BUS port with MOVX / INS / OUTL instead of the DBB, JNI on the INT pin, SEL MB0/1 and
//! ENT0 CLK in place of EN DMA / EN FLAGS, and no IBF interrupt.

pub trait UpiPorts {
    fn p1_in(&mut self, p2_out: u8) -> u8;
    fn p2_in(&mut self) -> u8 {
        0xFF
    }
    fn p1_out(&mut self, _v: u8) {}
    fn p2_out(&mut self, _v: u8) {}
    fn t0(&mut self) -> bool;
    fn t1(&mut self) -> bool;
    /// 8048 mode: the INT pin (JNI jumps when it is low; EN I takes an interrupt on low).
    fn int_pin(&mut self) -> bool {
        true
    }
    /// 8048 mode: external data bus read (MOVX A,@Rr / INS A,BUS) with RD strobe.
    fn bus_read(&mut self, _addr: u8) -> u8 {
        0xFF
    }
    /// 8048 mode: external data bus write (MOVX @Rr,A) with WR strobe.
    fn bus_write(&mut self, _addr: u8, _v: u8) {}
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Upi41 {
    pub rom: Vec<u8>,
    #[serde(with = "crate::bytes_n")]
    pub ram: [u8; 64],
    pub pc: u16,
    pub a: u8,
    pub psw: u8,
    f1: bool,
    pub p1: u8,
    pub p2: u8,
    pub dbb_in: u8,
    pub dbb_out: u8,
    pub ibf: bool,
    pub obf: bool,
    sts: u8,
    flags_en: bool,
    dma_en: bool,
    ien: bool,
    tien: bool,
    in_irq: bool,
    timer: u8,
    prescale: u8,
    timer_run: bool,
    counter_run: bool,
    tf: bool,
    t_ovf_pending: bool,
    last_t1: bool,
    pub cycles: u64,
    /// 8048/8749 mode (see the module notes).
    #[serde(default)]
    pub mcs48: bool,
    /// 8749 RAM 40h-7Fh.
    #[serde(default)]
    pub ram_hi: Vec<u8>,
    /// 8048 memory bank flip-flop (SEL MB0/1).
    #[serde(default)]
    mbf: bool,
    /// 8048 external interrupt enable (EN I / DIS I).
    #[serde(default)]
    xien: bool,
    /// 8048 BUS port latch (OUTL BUS).
    #[serde(default)]
    pub bus: u8,
}

impl Upi41 {
    pub fn new(rom: Vec<u8>) -> Upi41 {
        let mut u = Upi41 {
            rom,
            ram: [0; 64],
            pc: 0,
            a: 0,
            psw: 0x08,
            f1: false,
            p1: 0xFF,
            p2: 0xFF,
            dbb_in: 0,
            dbb_out: 0,
            ibf: false,
            obf: false,
            sts: 0,
            flags_en: false,
            dma_en: false,
            ien: false,
            tien: false,
            in_irq: false,
            timer: 0,
            prescale: 0,
            timer_run: false,
            counter_run: false,
            tf: false,
            t_ovf_pending: false,
            last_t1: false,
            cycles: 0,
            mcs48: false,
            ram_hi: Vec::new(),
            mbf: false,
            xien: false,
            bus: 0xFF,
        };
        u.reset();
        u
    }

    /// An 8048/8749 (2 KB program memory, 128 bytes of RAM).
    pub fn new_8048(rom: Vec<u8>) -> Upi41 {
        let mut u = Upi41::new(rom);
        u.mcs48 = true;
        u.ram_hi = vec![0; 64];
        u
    }

    /// Read RAM byte `i` (indirect addresses are masked to the RAM size).
    pub fn ram_at(&self, i: usize) -> u8 {
        if i < 64 { self.ram[i] } else { self.ram_hi.get(i - 64).copied().unwrap_or(0) }
    }

    fn set_ram(&mut self, i: usize, v: u8) {
        if i < 64 {
            self.ram[i] = v;
        }
        else if let Some(r) = self.ram_hi.get_mut(i - 64) {
            *r = v;
        }
    }

    pub fn reset(&mut self) {
        self.mbf = false;
        self.xien = false;
        self.bus = 0xFF;
        self.pc = 0;
        self.psw = 0x08;
        self.f1 = false;
        self.p1 = 0xFF;
        self.p2 = 0xFF;
        self.ibf = false;
        self.obf = false;
        self.flags_en = false;
        self.dma_en = false;
        self.ien = false;
        self.tien = false;
        self.in_irq = false;
        self.timer_run = false;
        self.counter_run = false;
        self.tf = false;
        self.t_ovf_pending = false;
    }

    // ---- host side ----
    /// Host read: a0=false data (clears OBF), a0=true status.
    pub fn host_read(&mut self, a0: bool) -> u8 {
        if a0 {
            (self.sts & 0xF0)
                | if self.f1 { 0x08 } else { 0 }
                | if self.psw & 0x20 != 0 { 0x04 } else { 0 }
                | if self.ibf { 0x02 } else { 0 }
                | if self.obf { 0x01 } else { 0 }
        }
        else {
            self.obf = false;
            self.dbb_out
        }
    }

    pub fn host_peek(&self, a0: bool) -> u8 {
        if a0 {
            (self.sts & 0xF0)
                | if self.f1 { 0x08 } else { 0 }
                | if self.psw & 0x20 != 0 { 0x04 } else { 0 }
                | if self.ibf { 0x02 } else { 0 }
                | if self.obf { 0x01 } else { 0 }
        }
        else {
            self.dbb_out
        }
    }

    /// Host write: a0=false data, a0=true command (F1 = A0).
    pub fn host_write(&mut self, a0: bool, v: u8) {
        self.dbb_in = v;
        self.ibf = true;
        self.f1 = a0;
    }

    /// P2 as seen on the pins (EN FLAGS puts OBF on P2.4 and /IBF on P2.5).
    pub fn p2_pins(&self) -> u8 {
        let mut v = self.p2;
        if self.flags_en {
            v = (v & !0x30) | if self.obf { 0x10 } else { 0 } | if self.ibf { 0 } else { 0x20 };
        }
        v
    }

    // ---- internals ----
    fn fetch(&mut self) -> u8 {
        let v = self.rom[(self.pc as usize) & (self.rom.len() - 1)];
        self.pc = (self.pc & 0x800) | ((self.pc + 1) & 0x7FF);
        v
    }

    fn bank(&self) -> usize {
        if self.psw & 0x10 != 0 { 24 } else { 0 }
    }

    fn r(&self, n: u8) -> u8 {
        self.ram[self.bank() + n as usize]
    }

    fn set_r(&mut self, n: u8, v: u8) {
        let b = self.bank();
        self.ram[b + n as usize] = v;
    }

    fn ind(&self, n: u8) -> usize {
        (self.r(n) & if self.mcs48 { 0x7F } else { 0x3F }) as usize
    }

    fn cy(&self) -> bool {
        self.psw & 0x80 != 0
    }

    fn set_cy(&mut self, c: bool) {
        if c {
            self.psw |= 0x80
        }
        else {
            self.psw &= !0x80
        }
    }

    fn add(&mut self, v: u8, carry: bool) {
        let c = carry as u16;
        let r = self.a as u16 + v as u16 + c;
        let ac = (self.a & 0xF) + (v & 0xF) + c as u8 > 0xF;
        self.set_cy(r > 0xFF);
        if ac {
            self.psw |= 0x40
        }
        else {
            self.psw &= !0x40
        }
        self.a = r as u8;
    }

    fn push_pc(&mut self) {
        let sp = (self.psw & 7) as usize;
        self.ram[8 + sp * 2] = self.pc as u8;
        self.ram[9 + sp * 2] = ((self.pc >> 8) as u8 & 0x0F) | (self.psw & 0xF0);
        self.psw = (self.psw & 0xF8) | ((sp as u8 + 1) & 7);
    }

    fn pop_pc(&mut self, restore_psw: bool) {
        let sp = ((self.psw & 7) + 7) & 7;
        self.psw = (self.psw & 0xF8) | sp;
        let lo = self.ram[8 + sp as usize * 2];
        let hi = self.ram[9 + sp as usize * 2];
        self.pc = ((hi as u16 & 0x0F) << 8) | lo as u16;
        if restore_psw {
            self.psw = (self.psw & 0x0F) | (hi & 0xF0);
        }
    }

    fn jcc(&mut self, cond: bool) {
        let page = self.pc & 0xF00;
        let off = self.fetch();
        if cond {
            self.pc = page | off as u16;
        }
    }

    /// Program memory bit 11 for JMP/CALL: the bank flip-flop on an 8048 (forced to 0 inside
    /// an interrupt routine), the current bank on the 8741.
    fn a11(&self) -> u16 {
        if self.mcs48 {
            if self.mbf && !self.in_irq { 0x800 } else { 0 }
        }
        else {
            self.pc & 0x800
        }
    }

    /// The opcodes where the 8048 differs from the 8741. Returns false for the shared ones.
    fn step_8048(&mut self, op: u8, io: &mut dyn UpiPorts, cyc: &mut u32) -> bool {
        match op {
            0x02 => {
                // OUTL BUS,A
                self.bus = self.a;
                io.bus_write(0, self.a);
                *cyc = 2;
            }
            0x05 => self.xien = true,
            0x08 => {
                // INS A,BUS
                self.a = io.bus_read(0);
                *cyc = 2;
            }
            0x15 => self.xien = false,
            0x22 | 0xD6 => {} // IN A,DBB / JNIBF: not on the 8048
            0x75 => {}        // ENT0 CLK: T0 outputs the clock (not used by the emulation)
            0x80 | 0x81 => {
                let r = self.r(op & 1);
                self.a = io.bus_read(r);
                *cyc = 2;
            }
            0x86 => {
                let i = io.int_pin();
                self.jcc(!i);
                *cyc = 2;
            }
            0x88 => {
                let v = self.fetch();
                self.bus |= v;
                io.bus_write(0, self.bus);
                *cyc = 2;
            }
            0x90 | 0x91 => {
                let r = self.r(op & 1);
                io.bus_write(r, self.a);
                *cyc = 2;
            }
            0x98 => {
                let v = self.fetch();
                self.bus &= v;
                io.bus_write(0, self.bus);
                *cyc = 2;
            }
            0xE5 => self.mbf = false,
            0xF5 => self.mbf = true,
            _ => return false,
        }
        true
    }

    fn timer_cycles(&mut self, n: u32, t1: bool) {
        for _ in 0..n {
            if self.timer_run {
                self.prescale += 1;
                if self.prescale == 32 {
                    self.prescale = 0;
                    self.inc_timer();
                }
            }
        }
        if self.counter_run && self.last_t1 && !t1 {
            self.inc_timer();
        }
        self.last_t1 = t1;
    }

    fn inc_timer(&mut self) {
        self.timer = self.timer.wrapping_add(1);
        if self.timer == 0 {
            self.tf = true;
            if self.tien {
                self.t_ovf_pending = true;
            }
        }
    }

    /// Execute one instruction (or take an interrupt); returns machine cycles.
    pub fn step(&mut self, io: &mut dyn UpiPorts) -> u32 {
        if !self.in_irq {
            if self.mcs48 && self.xien && !io.int_pin() {
                self.push_pc();
                self.pc = 3;
                self.in_irq = true;
                self.cycles += 2;
                return 2;
            }
            if !self.mcs48 && self.ien && self.ibf {
                self.push_pc();
                self.pc = 3;
                self.in_irq = true;
                self.cycles += 2;
                return 2;
            }
            if self.t_ovf_pending {
                self.t_ovf_pending = false;
                self.push_pc();
                self.pc = 7;
                self.in_irq = true;
                self.cycles += 2;
                return 2;
            }
        }
        let op = self.fetch();
        let mut cyc = 1u32;
        let rn = op & 7;
        let ri = op & 1;
        if self.mcs48 && self.step_8048(op, io, &mut cyc) {
            let t1 = io.t1();
            self.timer_cycles(cyc, t1);
            self.cycles += cyc as u64;
            return cyc;
        }
        match op {
            0x00 => {}
            0x02 => {
                self.dbb_out = self.a;
                self.obf = true;
            }
            0x03 => {
                let v = self.fetch();
                self.add(v, false);
                cyc = 2;
            }
            0x04 | 0x24 | 0x44 | 0x64 | 0x84 | 0xA4 | 0xC4 | 0xE4 => {
                let v = self.fetch();
                self.pc = self.a11() | (((op as u16) & 0xE0) << 3) | v as u16;
                cyc = 2;
            }
            0x05 => self.ien = true,
            0x07 => self.a = self.a.wrapping_sub(1),
            0x09 => {
                self.a = io.p1_in(self.p2) & self.p1;
                cyc = 2;
            }
            0x0A => {
                self.a = io.p2_in() & self.p2;
                cyc = 2;
            }
            0x0C..=0x0F | 0x3C..=0x3F | 0x8C..=0x8F | 0x9C..=0x9F => {
                // MOVD/ORLD/ANLD (8243 expander): not fitted
                if op < 0x10 {
                    self.a = 0x0F;
                }
                cyc = 2;
            }
            0x10 | 0x11 => {
                let i = self.ind(ri);
                let v = self.ram_at(i).wrapping_add(1);
                self.set_ram(i, v);
            }
            0x12 | 0x32 | 0x52 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => {
                let b = op >> 5;
                let c = self.a & (1 << b) != 0;
                self.jcc(c);
                cyc = 2;
            }
            0x13 => {
                let v = self.fetch();
                let c = self.cy();
                self.add(v, c);
                cyc = 2;
            }
            0x14 | 0x34 | 0x54 | 0x74 | 0x94 | 0xB4 | 0xD4 | 0xF4 => {
                let v = self.fetch();
                self.push_pc();
                self.pc = self.a11() | (((op as u16) & 0xE0) << 3) | v as u16;
                cyc = 2;
            }
            0x15 => self.ien = false,
            0x16 => {
                let t = self.tf;
                self.tf = false;
                self.jcc(t);
                cyc = 2;
            }
            0x17 => self.a = self.a.wrapping_add(1),
            0x18..=0x1F => {
                let v = self.r(rn).wrapping_add(1);
                self.set_r(rn, v);
            }
            0x20 | 0x21 => {
                let i = self.ind(ri);
                let m = self.ram_at(i);
                let a = self.a;
                self.set_ram(i, a);
                self.a = m;
            }
            0x22 => {
                self.a = self.dbb_in;
                self.ibf = false;
            }
            0x23 => {
                self.a = self.fetch();
                cyc = 2;
            }
            0x25 => self.tien = true,
            0x26 => {
                let t = io.t0();
                self.jcc(!t);
                cyc = 2;
            }
            0x27 => self.a = 0,
            0x28..=0x2F => {
                let v = self.r(rn);
                let a = self.a;
                self.set_r(rn, a);
                self.a = v;
            }
            0x30 | 0x31 => {
                let i = self.ind(ri);
                let m = self.ram_at(i);
                self.set_ram(i, (m & 0xF0) | (self.a & 0x0F));
                self.a = (self.a & 0xF0) | (m & 0x0F);
            }
            0x35 => {
                self.tien = false;
                self.t_ovf_pending = false;
            }
            0x36 => {
                let t = io.t0();
                self.jcc(t);
                cyc = 2;
            }
            0x37 => self.a = !self.a,
            0x39 => {
                self.p1 = self.a;
                io.p1_out(self.p1);
                cyc = 2;
            }
            0x3A => {
                self.p2 = self.a;
                let p = self.p2_pins();
                io.p2_out(p);
                cyc = 2;
            }
            0x40 | 0x41 => self.a |= self.ram_at(self.ind(ri)),
            0x42 => self.a = self.timer,
            0x43 => {
                self.a |= self.fetch();
                cyc = 2;
            }
            0x45 => {
                self.counter_run = true;
                self.timer_run = false;
            }
            0x46 => {
                let t = io.t1();
                self.jcc(!t);
                cyc = 2;
            }
            0x47 => self.a = self.a.rotate_left(4),
            0x48..=0x4F => self.a |= self.r(rn),
            0x50 | 0x51 => self.a &= self.ram_at(self.ind(ri)),
            0x53 => {
                self.a &= self.fetch();
                cyc = 2;
            }
            0x55 => {
                self.timer_run = true;
                self.counter_run = false;
                self.prescale = 0;
            }
            0x56 => {
                let t = io.t1();
                self.jcc(t);
                cyc = 2;
            }
            0x57 => {
                if self.a & 0x0F > 9 || self.psw & 0x40 != 0 {
                    let r = self.a as u16 + 6;
                    if r > 0xFF {
                        self.set_cy(true);
                    }
                    self.a = r as u8;
                }
                if self.a & 0xF0 > 0x90 || self.cy() {
                    let r = self.a as u16 + 0x60;
                    if r > 0xFF {
                        self.set_cy(true);
                    }
                    self.a = r as u8;
                }
            }
            0x58..=0x5F => self.a &= self.r(rn),
            0x60 | 0x61 => {
                let v = self.ram_at(self.ind(ri));
                self.add(v, false);
            }
            0x62 => self.timer = self.a,
            0x65 => {
                self.timer_run = false;
                self.counter_run = false;
            }
            0x67 => {
                let c = self.a & 1 != 0;
                self.a = (self.a >> 1) | if self.cy() { 0x80 } else { 0 };
                self.set_cy(c);
            }
            0x68..=0x6F => {
                let v = self.r(rn);
                self.add(v, false);
            }
            0x70 | 0x71 => {
                let v = self.ram_at(self.ind(ri));
                let c = self.cy();
                self.add(v, c);
            }
            0x76 => {
                let f = self.f1;
                self.jcc(f);
                cyc = 2;
            }
            0x77 => self.a = self.a.rotate_right(1),
            0x78..=0x7F => {
                let v = self.r(rn);
                let c = self.cy();
                self.add(v, c);
            }
            0x83 => {
                self.pop_pc(false);
                cyc = 2;
            }
            0x85 => self.psw &= !0x20,
            0x86 => {
                let o = self.obf;
                self.jcc(o);
                cyc = 2;
            }
            0x89 => {
                self.p1 |= self.fetch();
                io.p1_out(self.p1);
                cyc = 2;
            }
            0x8A => {
                self.p2 |= self.fetch();
                let p = self.p2_pins();
                io.p2_out(p);
                cyc = 2;
            }
            0x90 => self.sts = self.a,
            0x93 => {
                self.pop_pc(true);
                self.in_irq = false;
                cyc = 2;
            }
            0x95 => self.psw ^= 0x20,
            0x96 => {
                let c = self.a != 0;
                self.jcc(c);
                cyc = 2;
            }
            0x97 => self.set_cy(false),
            0x99 => {
                self.p1 &= self.fetch();
                io.p1_out(self.p1);
                cyc = 2;
            }
            0x9A => {
                self.p2 &= self.fetch();
                let p = self.p2_pins();
                io.p2_out(p);
                cyc = 2;
            }
            0xA0 | 0xA1 => {
                let i = self.ind(ri);
                let a = self.a;
                self.set_ram(i, a);
            }
            0xA3 => {
                let addr = (self.pc & 0xF00) | self.a as u16;
                self.a = self.rom[addr as usize & (self.rom.len() - 1)];
                cyc = 2;
            }
            0xA5 => self.f1 = false,
            0xA7 => {
                let c = !self.cy();
                self.set_cy(c);
            }
            0xA8..=0xAF => {
                let a = self.a;
                self.set_r(rn, a);
            }
            0xB0 | 0xB1 => {
                let v = self.fetch();
                let i = self.ind(ri);
                self.set_ram(i, v);
                cyc = 2;
            }
            0xB3 => {
                let addr = (self.pc & 0xF00) | self.a as u16;
                let v = self.rom[addr as usize & (self.rom.len() - 1)];
                self.pc = (self.pc & 0xF00) | v as u16;
                cyc = 2;
            }
            0xB5 => self.f1 = !self.f1,
            0xB6 => {
                let f = self.psw & 0x20 != 0;
                self.jcc(f);
                cyc = 2;
            }
            0xB8..=0xBF => {
                let v = self.fetch();
                self.set_r(rn, v);
                cyc = 2;
            }
            0xC5 => self.psw &= !0x10,
            0xC6 => {
                let z = self.a == 0;
                self.jcc(z);
                cyc = 2;
            }
            0xC7 => self.a = self.psw | 0x08,
            0xC8..=0xCF => {
                let v = self.r(rn).wrapping_sub(1);
                self.set_r(rn, v);
            }
            0xD0 | 0xD1 => self.a ^= self.ram_at(self.ind(ri)),
            0xD3 => {
                self.a ^= self.fetch();
                cyc = 2;
            }
            0xD5 => self.psw |= 0x10,
            0xD6 => {
                let n = !self.ibf;
                self.jcc(n);
                cyc = 2;
            }
            0xD7 => self.psw = self.a | 0x08,
            0xD8..=0xDF => self.a ^= self.r(rn),
            0xE3 => {
                let addr = 0x300 | self.a as u16;
                self.a = self.rom[addr as usize & (self.rom.len() - 1)];
                cyc = 2;
            }
            0xE5 => self.dma_en = true,
            0xE6 => {
                let c = !self.cy();
                self.jcc(c);
                cyc = 2;
            }
            0xE7 => self.a = self.a.rotate_left(1),
            0xE8..=0xEF => {
                let v = self.r(rn).wrapping_sub(1);
                self.set_r(rn, v);
                self.jcc(v != 0);
                cyc = 2;
            }
            0xF0 | 0xF1 => self.a = self.ram_at(self.ind(ri)),
            0xF5 => {
                self.flags_en = true;
                let p = self.p2_pins();
                io.p2_out(p);
            }
            0xF6 => {
                let c = self.cy();
                self.jcc(c);
                cyc = 2;
            }
            0xF7 => {
                let c = self.a & 0x80 != 0;
                self.a = (self.a << 1) | self.cy() as u8;
                self.set_cy(c);
            }
            0xF8..=0xFF => self.a = self.r(rn),
            _ => {} // undefined opcodes act as NOP
        }
        let t1 = io.t1();
        self.timer_cycles(cyc, t1);
        self.cycles += cyc as u64;
        cyc
    }
}
