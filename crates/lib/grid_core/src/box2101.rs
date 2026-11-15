//! GRiD 2101 hard disk and floppy box, low-level: its board CPU runs the real firmware (ROM
//! 300205-05 for the 20 MB drive, 300205-06 for the 10 MB one).
//!
//! The board ("Disk Controller II", assembly 100371-00, from a photo of Kurt's 2101 and the
//! ROM's own accesses; the board CPU's DS is 0040h, so DS-relative addresses are 400h lower):
//! - Intel 8088 with an 8284A clock (crystal marked DS-1200, taken as 12 MHz: 4 MHz CPU;
//!   GRID_2101_MHZ overrides it). 8 KB RAM (2186) at 0-1FFFh, 8 KB ROM at E000h of every
//!   64 KB (reset jumps to FE00:0004).
//! - 4000h-5FFFh WD2797 (A12-A11 = register), 1 MHz clock, Tandon TM55-2 48 tpi drive.
//! - 6000h-6003h four-character display (6003h is the leftmost character); reading 6000h
//!   gives the DIP switches (bits 4-0, the box's GPIB address, 4).
//! - 8000h-9FFFh Intel 8291A GPIB talker/listener, A12-A10 = register.
//! - A000h-A00Eh 74LS259 latch, bit = A3-A1, data = D0: Q0 floppy light, Q1 display reset
//!   (low), Q2 floppy drive select and motor, Q3 not used here (the firmware sets it from
//!   cylinder 50 and while it talks GPIB), Q4 hold accesses to the data ports until the device
//!   is ready (READY, the pseudo-DMA used with REP MOVSB), Q5 hard disk light, Q6 SASI SEL,
//!   Q7 SASI RST.
//! - Reading A000h: SASI status, low = asserted: bit 0 C/D, 1 REQ, 2 I/O, 3 MSG, 4 BSY; bit 5
//!   the WD2797's INTRQ (high); bit 7 high starts the self test at power on.
//! - C000h SASI data: with Q4 set, an access waits for REQ and does the REQ/ACK handshake;
//!   without it a write only puts the byte on the bus (the controller ID for selection).
//! - INTR = 8291A INT or WD2797 INTRQ, with no interrupt controller (the data bus floats:
//!   vector FFh; every vector points at the firmware's one handler). NMI = the test button on
//!   the back (self test).
//!
//! Behind the SASI bus sits a Xebec S1410 with the drive, modelled at command level.

use serde::{Deserialize, Serialize};

use crate::box2102::{HostLines, Lines};
use crate::fast86::{Bus86, Fast86};
use crate::i8291::I8291;
use crate::s1410::S1410;
use crate::wd279x::{Disk, Env, Mech, Wd2797};

/// BOXLOG set: print the box's display, SASI commands and floppy commands on stderr.
fn box_log() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("BOXLOG").is_some())
}

/// Extra board time per instruction, per mille (BOX_SLOW; default 47).
fn box_slow() -> u64 {
    static V: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("BOX_SLOW").ok().and_then(|v| v.parse().ok()).unwrap_or(47))
}

/// The board's clock rate in Hz.
fn clock_hz() -> u64 {
    static HZ: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *HZ.get_or_init(|| {
        std::env::var("GRID_2101_MHZ").ok().and_then(|v| v.parse::<f64>().ok()).map(|m| (m * 1e6) as u64).unwrap_or(4_000_000)
    })
}

/// Which access is held (READY low).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Held {
    GpibIn,
    GpibOut,
    Sasi,
    Fdc,
}

#[derive(Clone, Serialize, Deserialize)]
struct Hw {
    rom: Vec<u8>,
    ram: Vec<u8>,
    clocks: u64,
    #[serde(default)]
    slow_acc: u64,
    latch: u8,
    /// Byte on the SASI data bus from the board (selection).
    sasi_out: u8,
    display: [u8; 4],
    dip: u8,
    gpib: I8291,
    s1410: S1410,
    fdc: Wd2797,
    mech: Mech,
    disk: Disk,
    #[serde(skip)]
    img_key: (usize, usize),
    /// The last access was held.
    #[serde(skip)]
    held: Option<Held>,
    /// Bus cycles in the current instruction (an 8088 needs 4 clocks each).
    #[serde(skip)]
    cycles: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Box2101 {
    cpu: Fast86,
    hw: Hw,
    /// Box time minus machine time, in ns: the box was switched on this long before.
    #[serde(default)]
    pub ahead: u64,
    /// Display changes, for the log.
    #[serde(skip)]
    pub log: Vec<String>,
}

/// Disk images the box works on.
pub struct Media<'a> {
    pub hd: &'a mut Vec<u8>,
    pub hd_dirty: &'a mut bool,
    pub fd: &'a mut Vec<u8>,
    pub fd_dirty: &'a mut bool,
}

struct Ctx<'a, 'b> {
    hw: &'a mut Hw,
    m: &'a mut Media<'b>,
}

impl Hw {
    fn ns(&self) -> u64 {
        (self.clocks as u128 * 1_000_000_000 / clock_hz() as u128) as u64
    }
    fn us(&self) -> u64 {
        self.clocks * 1_000_000 / clock_hz()
    }
    fn q(&self, n: u8) -> bool {
        self.latch & (1 << n) != 0
    }
    fn display_text(&self) -> String {
        self.display.iter().rev().map(|&c| if (0x20..0x7F).contains(&c) { c as char } else { ' ' }).collect()
    }
}

impl Ctx<'_, '_> {
    fn fdc_env<'c>(hw: &'c mut Hw, fd: &'c mut Vec<u8>, dirty: &'c mut bool) -> (&'c mut Wd2797, Env<'c>) {
        let selected = hw.latch & 0x04 != 0;
        let spinning = selected && !fd.is_empty();
        let now = hw.us();
        let Hw { fdc, mech, disk, .. } = hw;
        (fdc, Env { now, spinning, selected, mech, disk, image: fd, dirty, write_protect: false, ready: spinning })
    }

    /// Is a held access to `h` ready now?
    fn ready(&mut self, h: Held) -> bool {
        match h {
            Held::GpibIn => self.hw.gpib.dreq_in(),
            Held::GpibOut => self.hw.gpib.dreq_out(),
            Held::Sasi => self.hw.s1410.req(self.hw.ns()),
            Held::Fdc => {
                let (fdc, mut e) = Self::fdc_env(self.hw, self.m.fd, self.m.fd_dirty);
                fdc.advance(&mut e);
                fdc.drq
            }
        }
    }

    /// Hold the access if READY is low (Q4 set and the device not ready).
    fn hold(&mut self, h: Held) -> bool {
        if self.hw.q(4) && !self.ready(h) {
            self.hw.held = Some(h);
            return true;
        }
        false
    }

    fn sasi_status(&self) -> u8 {
        let s = &self.hw.s1410;
        let now = self.hw.ns();
        let mut v = 0x5F;
        for (bit, on) in [(0, s.cd()), (1, s.req(now)), (2, s.io()), (3, s.msg()), (4, s.bsy_at(now))] {
            if on {
                v &= !(1 << bit);
            }
        }
        if self.hw.fdc.intrq {
            v |= 0x20;
        }
        v
    }

    fn latch_write(&mut self, bit: u8, on: bool) {
        let before = self.hw.latch;
        if on {
            self.hw.latch |= 1 << bit;
        }
        else {
            self.hw.latch &= !(1 << bit);
        }
        let now = self.hw.ns();
        match bit {
            1 if !on && before & 2 != 0 => self.hw.display = [b' '; 4],
            6 => self.hw.s1410.select(now, on, self.hw.sasi_out),
            7 if on => self.hw.s1410.reset(),
            _ => {}
        }
    }
}

impl Bus86 for Ctx<'_, '_> {
    const WAITS: bool = true;

    fn stalled(&mut self) -> bool {
        self.hw.held.is_some()
    }

    fn read8(&mut self, a: u32) -> u8 {
        self.hw.cycles += 1;
        let lo = a & 0xFFFF;
        if lo >= 0xE000 {
            return self.hw.rom[(a & 0x1FFF) as usize];
        }
        match lo >> 13 {
            0 => self.hw.ram[(a & 0x1FFF) as usize],
            2 => {
                let reg = ((lo >> 11) & 3) as u8;
                if reg == 3 && self.hold(Held::Fdc) {
                    return 0xFF;
                }
                let (fdc, mut e) = Self::fdc_env(self.hw, self.m.fd, self.m.fd_dirty);
                fdc.read(reg, &mut e)
            }
            3 => self.hw.dip & 0x1F,
            4 => {
                let reg = ((lo >> 10) & 7) as u8;
                if reg == 0 && self.hold(Held::GpibIn) {
                    return 0xFF;
                }
                self.hw.gpib.read(reg)
            }
            5 => self.sasi_status(),
            6 => {
                if self.hold(Held::Sasi) {
                    return 0xFF;
                }
                let now = self.hw.ns();
                if self.hw.q(4) {
                    self.hw.s1410.read_byte(now, self.m.hd)
                }
                else {
                    0xFF
                }
            }
            _ => 0xFF,
        }
    }

    fn write8(&mut self, a: u32, v: u8) {
        self.hw.cycles += 1;
        let lo = a & 0xFFFF;
        if lo >= 0xE000 {
            return;
        }
        match lo >> 13 {
            0 => self.hw.ram[(a & 0x1FFF) as usize] = v,
            2 => {
                let reg = ((lo >> 11) & 3) as u8;
                if reg == 3 && self.hold(Held::Fdc) {
                    return;
                }
                let (fdc, mut e) = Self::fdc_env(self.hw, self.m.fd, self.m.fd_dirty);
                fdc.write(reg, v, &mut e);
            }
            3 => {
                let i = (lo & 3) as usize;
                if self.hw.display[i] != v {
                    self.hw.display[i] = v;
                }
            }
            4 => {
                let reg = ((lo >> 10) & 7) as u8;
                if reg == 0 && self.hold(Held::GpibOut) {
                    return;
                }
                self.hw.gpib.write(reg, v);
            }
            5 => self.latch_write(((lo >> 1) & 7) as u8, v & 1 != 0),
            6 => {
                if self.hold(Held::Sasi) {
                    return;
                }
                let now = self.hw.ns();
                if self.hw.q(4) {
                    self.hw.s1410.write_byte(now, v, self.m.hd, self.m.hd_dirty);
                }
                else {
                    self.hw.sasi_out = v;
                    let sel = self.hw.q(6);
                    self.hw.s1410.select(now, sel, v);
                }
            }
            _ => {}
        }
    }

    fn io_in8(&mut self, _port: u16) -> u8 {
        0xFF
    }

    fn io_out8(&mut self, _port: u16, _v: u8) {}

    fn intr(&mut self) -> bool {
        self.hw.gpib.int() || self.hw.fdc.intrq
    }

    fn inta(&mut self) -> u8 {
        0xFF
    }
}

impl Box2101 {
    /// Set the DIP switches to GPIB address `addr` (hard disk; the floppy answers on addr + 1).
    pub fn set_address(&mut self, addr: u8) {
        self.hw.dip = addr & 0x1F;
    }

    /// A 2101 with firmware `rom` (8 KB).
    pub fn new(rom: Vec<u8>) -> Box2101 {
        let mut fdc = Wd2797::new();
        fdc.reset();
        let mut rom = rom;
        rom.resize(0x2000, 0xFF);
        let mut cpu = Fast86::new();
        // reset: FFFF:0000
        cpu.s = [0, 0xFFFF, 0, 0];
        cpu.ip = 0;
        Box2101 {
            cpu,
            hw: Hw {
                rom,
                ram: vec![0; 0x2000],
                clocks: 0,
                slow_acc: 0,
                latch: 0,
                sasi_out: 0,
                display: [b' '; 4],
                dip: 4,
                gpib: I8291::new(),
                s1410: S1410::new(),
                fdc,
                mech: Mech { cyl: 0, phase: 0 },
                disk: Disk::default(),
                img_key: (0, 0),
                held: None,
                cycles: 0,
            },
            ahead: 0,
            log: Vec::new(),
        }
    }

    pub fn ns(&self) -> u64 {
        self.hw.ns()
    }

    /// Start the box's clock at `ns` (when it joins a machine that has run for a while).
    pub fn set_ns(&mut self, ns: u64) {
        self.hw.clocks = (ns as u128 * clock_hz() as u128 / 1_000_000_000) as u64;
    }

    /// Run the box alone for `ns` (switched on before the Compass).
    pub fn power_on_early(&mut self, ns: u64, m: &mut Media) {
        let start = self.ns();
        while self.ns() < start + ns {
            self.run(start + ns, m);
        }
        self.ahead += ns;
    }

    /// The four-character display.
    pub fn display(&self) -> String {
        self.hw.display_text()
    }

    /// The hard disk or floppy light is on.
    pub fn busy(&self) -> bool {
        self.hw.q(5) || self.hw.q(2)
    }

    pub fn hd_busy(&self) -> bool {
        self.hw.q(5)
    }

    pub fn fd_busy(&self) -> bool {
        self.hw.q(2)
    }

    pub fn set_host(&mut self, h: HostLines) {
        self.hw.gpib.set_host(h);
    }

    pub fn lines(&self) -> Lines {
        self.hw.gpib.lines()
    }

    /// Press the test button on the back (NMI: the self test).
    pub fn test_button(&mut self, m: &mut Media) {
        let mut ctx = Ctx { hw: &mut self.hw, m };
        self.cpu.nmi(&mut ctx);
    }

    /// When the box next has something to do while the CPU is held.
    fn next_event(&self) -> u64 {
        let ns = self.hw.ns();
        let fdc_us = self.hw.fdc.next_event();
        let fdc_ns = if fdc_us == u64::MAX { u64::MAX } else { fdc_us.saturating_mul(1000) };
        self.hw.s1410.next_event(ns).min(fdc_ns)
    }

    fn add_ns(&mut self, ns: u64) {
        let c = (ns as u128 * clock_hz() as u128).div_ceil(1_000_000_000) as u64;
        self.hw.clocks += c.max(1);
    }

    /// Run until `target` ns, or until the lines the box drives change (returns true then).
    pub fn run(&mut self, target: u64, m: &mut Media) -> bool {
        let key = (m.fd.as_ptr() as usize, m.fd.len());
        if key != self.hw.img_key {
            self.hw.img_key = key;
            self.hw.disk = Disk::new(m.fd);
            // A disk the 2101 formats gets interleave 3 with no skew between the sides (IDs
            // 1 4 7 2 5 8 3 6 9): take that for tracks nobody formatted here.
            if std::env::var_os("FD_IL").is_none() {
                self.hw.disk.il = 3;
            }
            if std::env::var_os("FD_SKEW").is_none() {
                self.hw.disk.skew = 0;
            }
        }
        let before = self.lines();
        while self.hw.ns() < target {
            let now = self.hw.ns();
            if self.hw.us() >= self.hw.fdc.next_event() {
                let (fdc, mut e) = Ctx::fdc_env(&mut self.hw, m.fd, m.fd_dirty);
                fdc.advance(&mut e);
            }
            self.hw.s1410.advance(now, m.hd, m.hd_dirty);
            let disp = self.hw.display;
            let held = self.hw.held.take();
            let mut ctx = Ctx { hw: &mut self.hw, m };
            if let Some(h) = held {
                // Held access: retry once it is ready (or an interrupt comes, which the CPU
                // takes before running the instruction again).
                let intr = ctx.intr() && self.cpu.flags & 0x0200 != 0;
                if !ctx.ready(h) && !intr {
                    self.hw.held = Some(h);
                    let next = self.next_event().min(target);
                    let step = if h == Held::GpibIn || h == Held::GpibOut { next } else { next.min(now + 1_000) };
                    let wait = step.saturating_sub(now).max(250);
                    self.add_ns(wait);
                    continue;
                }
            }
            self.hw.cycles = 0;
            let c0 = self.cpu.clocks;
            let mut ctx = Ctx { hw: &mut self.hw, m };
            self.cpu.step(&mut ctx);
            let c = (self.cpu.clocks - c0).max(self.hw.cycles * 4).max(2);
            // The fast core leaves out the 8088's byte-wide prefetch: the real board's wait loop
            // runs 617 turns a disk turn (27.0 us) where the core gives 647 (box probe 0.4.1),
            // so stretch its time by 4.7 % (BOX_SLOW, per mille).
            self.hw.slow_acc += c * box_slow();
            self.hw.clocks += c + self.hw.slow_acc / 1000;
            self.hw.slow_acc %= 1000;
            if self.hw.display != disp {
                let t = self.hw.display_text();
                if box_log() {
                    eprintln!("2101 {} display '{}'", self.hw.ns() / 1000, t);
                }
                if self.log.len() < 1000 {
                    self.log.push(t);
                }
            }
            if box_log() {
                for s in self.hw.s1410.trace.drain(..) {
                    eprintln!("2101 {} {}", self.hw.clocks * 1_000_000 / clock_hz(), s);
                }
            }
            if self.lines() != before {
                return true;
            }
        }
        false
    }

    /// Sector IDs of a floppy track in rotation order, with their byte positions.
    pub fn floppy_layout(&self, fd: &[u8], cyl: u8, head: u8) -> Vec<(u8, u16)> {
        self.hw.disk.track(fd, cyl, head).iter().map(|s| (s.id[2], s.id_pos)).collect()
    }

    /// The controller's state, for traces.
    pub fn debug(&self) -> String {
        format!(
            "cs:ip {:04X}:{:04X} latch {:02X} display '{}' sasi {:?} held {:?}",
            self.cpu.s[1],
            self.cpu.ip,
            self.hw.latch,
            self.hw.display_text(),
            self.hw.s1410.phase,
            self.hw.held
        )
    }
}
