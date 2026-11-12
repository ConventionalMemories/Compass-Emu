//! GRiD 2102 portable floppy, low-level: its 8051 runs the real firmware (ROM 300237).
//!
//! Wiring from the 2102 schematic (CPU, GPIB and floppy controller boards):
//! - 8051 at 12 MHz (8284A oscillator): one machine cycle = 1 us. External 2732 ROM.
//! - PAL U5 decodes A14-A12 of MOVX: 1000h 2 KB RAM, 2000h WD2797 (A11/A10 = register),
//!   3000h DIP switches (SW1-5 = D0-4, through an inverting buffer, so closed = 1),
//!   4000h GPIB data (write latch / read buffer, both inverting, so the CPU sees true data).
//! - P1.0 IFC, P1.1 DRQ, P1.2 ATN, P1.3 SRQ, P1.4 DAV, P1.5 NRFD, P1.6 NDAC, P1.7 EOI, through a
//!   DS75161 (pin level = bus level, low = asserted).
//! - P3.0 motor on, P3.1 write precompensation, P3.2 / P3.4 drive select (second / first
//!   drive), P3.3 = INT1 = WD2797 INTRQ inverted, P3.5 = talk enable (low: the data latch
//!   and DAV/EOI drive the bus, NRFD/NDAC are read from it).
//! - WD2797 clock 1 MHz (2 MHz PCLK / 2), READY and HLT tied high.

use serde::{Deserialize, Serialize};

use crate::i8051::{Bus51, I8051};
use crate::wd279x::{Disk, Env, Mech, Wd2797};

/// Bus lines a side drives (true = asserted, i.e. low on the wire).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Lines {
    pub dav: bool,
    pub eoi: bool,
    pub nrfd: bool,
    pub ndac: bool,
    pub srq: bool,
    pub dio: u8,
}

/// What the Compass side drives.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct HostLines {
    pub atn: bool,
    pub ifc: bool,
    pub dav: bool,
    pub eoi: bool,
    pub nrfd: bool,
    pub ndac: bool,
    pub dio: u8,
}

#[derive(Clone, Serialize, Deserialize)]
struct Hw {
    rom: Vec<u8>,
    ram: Vec<u8>,
    fdc: Wd2797,
    /// Time in microseconds (8051 machine cycles).
    us: u64,
    host: HostLines,
    gpib_out: u8,
    dip: u8,
    mech: Mech,
    disk: Disk,
    /// Which image the disk layout belongs to (pointer, length): a new image is a new disk.
    #[serde(skip)]
    img_key: (usize, usize),
    /// Port latches at the start of the instruction.
    p1: u8,
    p3: u8,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Box2102 {
    cpu: I8051,
    hw: Hw,
    /// Box time minus machine time, in us: the box was switched on this long before.
    #[serde(default)]
    pub ahead: u64,
}

/// BOXLOG set: print the bytes the 2102 reads and writes on GPIB.
fn box_log() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("BOXLOG").is_some())
}

struct Ctx<'a> {
    hw: &'a mut Hw,
    image: &'a mut Vec<u8>,
    dirty: &'a mut bool,
}

impl Ctx<'_> {
    fn talk(&self) -> bool {
        self.hw.p3 & 0x20 == 0
    }

    fn fdc_env<'b>(hw: &'b mut Hw, image: &'b mut Vec<u8>, dirty: &'b mut bool) -> (&'b mut Wd2797, Env<'b>) {
        let selected = hw.p3 & 0x10 != 0;
        let spinning = selected && hw.p3 & 0x01 != 0 && !image.is_empty();
        let Hw { fdc, mech, disk, us, .. } = hw;
        (fdc, Env { now: *us, spinning, selected, mech, disk, image, dirty, write_protect: false })
    }
}

impl Bus51 for Ctx<'_> {
    fn code(&mut self, a: u16) -> u8 {
        *self.hw.rom.get(a as usize & 0xFFF).unwrap_or(&0xFF)
    }

    fn xread(&mut self, a: u16) -> u8 {
        match (a >> 12) & 7 {
            1 => self.hw.ram[a as usize & 0x7FF],
            2 => {
                let (fdc, mut e) = Ctx::fdc_env(self.hw, self.image, self.dirty);
                fdc.read(((a >> 10) & 3) as u8, &mut e)
            }
            3 => self.hw.dip & 0x1F,
            4 => {
                let v = if self.talk() { self.hw.gpib_out } else { self.hw.host.dio };
                if box_log() {
                    eprintln!("2102 {} gpib in {:02X} atn {} eoi {}", self.hw.us, v, self.hw.host.atn, self.hw.host.eoi);
                }
                v
            }
            _ => 0xFF,
        }
    }

    fn xwrite(&mut self, a: u16, v: u8) {
        match (a >> 12) & 7 {
            1 => self.hw.ram[a as usize & 0x7FF] = v,
            2 => {
                let (fdc, mut e) = Ctx::fdc_env(self.hw, self.image, self.dirty);
                fdc.write(((a >> 10) & 3) as u8, v, &mut e);
            }
            4 => {
                if box_log() {
                    eprintln!("2102 {} gpib out {:02X}", self.hw.us, v);
                }
                self.hw.gpib_out = v
            }
            _ => {}
        }
    }

    fn pins(&mut self, port: usize) -> u8 {
        let h = self.hw.host;
        match port {
            1 => {
                let talk = self.talk();
                let mut v = 0xFFu8;
                for (bit, asserted) in [(0, h.ifc), (1, !self.hw.fdc.drq), (2, h.atn)] {
                    if asserted {
                        v &= !(1 << bit);
                    }
                }
                let lines = if talk { [(5, h.nrfd), (6, h.ndac)] } else { [(4, h.dav), (7, h.eoi)] };
                for (bit, asserted) in lines {
                    if asserted {
                        v &= !(1 << bit);
                    }
                }
                v
            }
            3 => {
                if self.hw.fdc.intrq {
                    0xF7
                }
                else {
                    0xFF
                }
            }
            _ => 0xFF,
        }
    }
}

impl Box2102 {
    /// A 2102 with firmware `rom` at GPIB address `addr`.
    pub fn new(rom: Vec<u8>, addr: u8) -> Box2102 {
        let mut fdc = Wd2797::new();
        fdc.reset();
        Box2102 {
            cpu: I8051::new(),
            hw: Hw {
                rom,
                ram: vec![0; 2048],
                fdc,
                us: 0,
                host: HostLines::default(),
                gpib_out: 0,
                dip: addr & 0x1F,
                mech: Mech { cyl: 0, phase: 0 },
                disk: Disk::default(),
                img_key: (0, 0),
                p1: 0xFF,
                p3: 0xFF,
            },
            ahead: 0,
        }
    }

    /// Run the box alone for `us` (switched on before the Compass, so its power-on self test
    /// and drive probe, about 3 s, are over when the Compass first calls it).
    pub fn power_on_early(&mut self, us: u64, image: &mut Vec<u8>, dirty: &mut bool) {
        let start = self.hw.us;
        while self.hw.us < start + us {
            self.run(start + us, image, dirty);
        }
        self.ahead += us;
    }

    pub fn us(&self) -> u64 {
        self.hw.us
    }

    /// Start the box's clock at `us` (when it joins a machine that has run for a while).
    pub fn set_us(&mut self, us: u64) {
        self.hw.us = us;
    }

    /// The drive motor runs (for a drive activity light).
    pub fn motor_on(&self) -> bool {
        self.cpu.latch(3) & 0x01 != 0
    }

    pub fn address(&self) -> u8 {
        self.hw.dip
    }

    pub fn set_host(&mut self, h: HostLines) {
        self.hw.host = h;
    }

    /// The lines the box drives now.
    pub fn lines(&self) -> Lines {
        let p1 = self.cpu.latch(1);
        let talk = self.cpu.latch(3) & 0x20 == 0;
        Lines {
            dav: talk && p1 & 0x10 == 0,
            eoi: talk && !self.hw.host.atn && p1 & 0x80 == 0,
            nrfd: !talk && p1 & 0x20 == 0,
            ndac: !talk && p1 & 0x40 == 0,
            srq: p1 & 0x08 == 0,
            dio: if talk { self.hw.gpib_out } else { 0 },
        }
    }

    /// Run until `target` us, or until the lines the box drives change (returns true then).
    pub fn run(&mut self, target: u64, image: &mut Vec<u8>, dirty: &mut bool) -> bool {
        let key = (image.as_ptr() as usize, image.len());
        if key != self.hw.img_key {
            self.hw.img_key = key;
            self.hw.disk = Disk::new(image);
        }
        let before = self.lines();
        while self.hw.us < target {
            self.hw.p1 = self.cpu.latch(1);
            self.hw.p3 = self.cpu.latch(3);
            if self.hw.us >= self.hw.fdc.next_event() {
                let (fdc, mut e) = Ctx::fdc_env(&mut self.hw, image, dirty);
                fdc.advance(&mut e);
            }
            let mut ctx = Ctx { hw: &mut self.hw, image, dirty };
            let c = self.cpu.step(&mut ctx);
            self.hw.us += c as u64;
            if self.lines() != before {
                return true;
            }
        }
        false
    }

    /// The controller's state, for traces.
    pub fn debug(&self) -> String {
        format!("pc {:04X} p1 {:02X} p3 {:02X} cyl {} trk {}", self.cpu.pc, self.cpu.latch(1), self.cpu.latch(3), self.hw.mech.cyl, self.hw.fdc.track)
    }
}
