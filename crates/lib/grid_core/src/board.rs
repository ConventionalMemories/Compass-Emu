//! GRiD Compass 1101 motherboard and peripherals as one ExternalBoard.
//!
//! Address map (1101, from MAME gridcomp.cpp grid1101_map):
//!   00000-3FFFF  RAM (plain bus memory, not claimed here; contention via ram_wait)
//!   C0000-CFFFF  test ROM socket (empty -> FF)
//!   DFE40-DFE4F  sound DACs (DFE42/DFE44 sound, odd = volume)
//!   DFE80-DFE83  Intel 7220 bubble controller (even bytes)
//!   DFEC0-DFECF  8255 modem interface (even bytes)
//!   DFF00-DFF1F  8274 serial (even bytes)
//!   DFF40-DFF5F  MM58174 RTC (low nibble) + X2210 machine ID (high nibble), odd bytes
//!   DFF80-DFF8F  TMS9914 GPIB (even bytes)
//!   DFFC0-DFFCF  8741 keyboard: DFFC0 data, DFFC2 status/command
//!   E0000-EFFFF  GPIB pseudo-DMA window (TMS9914 data register)
//!   FC000-FFFFF  16K boot PROM (even/odd chips interleaved)
//!
//! Split board (Compass II 1121/1129/1131/1139, MAME grid1121_map + the 1139 PROM and 113x OS):
//!   00000-7FFFF  RAM (256K or 512K)
//!   80000-9FFFF  application ROM sockets (empty -> FF)
//!   DFA00-DFDFF  GPIB pseudo-DMA window (instead of E0000)
//!   DFE00-DFE07  video/memory control: DFE00 = 1 selects the video chip, then DFE04 sets the
//!                mode; the 113x OS writes 0Bh (512K) or 0Ah (256K) for the large screen, the
//!                PROM writes 0Eh (bit 2 set = 320 x 240 compatibility mode)
//!   DFE08-DFE0F  application ROM control
//!   DFE10-DFE1F  serial port select latches (ST0-3, at DFE00 on the 1101)
//! IO ports 00h/02h: 80130 interrupt controller (8259 compatible); 08h-0Eh: 80130 timers.
//! IRQ wiring (MAME): IR0 serial, IR1 bubble, IR2 modem, IR3 vsync, IR4 keyboard, IR5 GPIB,
//! IR6 8087, IR7 ring.

use std::any::Any;

use marty_core::bus::external::ExternalBoard;
use marty_core::devices::pic::Pic;

use crate::bubble::Bubble;
use crate::gpib::Gpib;
use crate::serial::Serial8274;
use crate::sound::Sound;
use crate::rtc::Rtc;
use crate::keyboard::Keyboard;
use crate::machine::GridModel;
use crate::screen::{Beam, DisplayTiming};

pub const IR_SERIAL: u8 = 0;
pub const IR_BUBBLE: u8 = 1;
pub const IR_MODEM: u8 = 2;
pub const IR_VSYNC: u8 = 3;
pub const IR_KEYBOARD: u8 = 4;
pub const IR_GPIB: u8 = 5;
pub const IR_8087: u8 = 6;

/// See tick_to.
const SLOW_STEP: u64 = 64;


/// A logged access to a device, for debugging and for Claude's tools.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct IoLog {
    pub clock: u64,
    pub address: usize,
    pub write: bool,
    pub data: u8,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct GridBoard {
    prom: Vec<u8>,
    /// CPU clocks since power-on (advanced by tick()).
    pub clocks: u64,
    pic: Pic,
    /// 80130 timers (8254-like at ports 08-0E), counting at the 5 MHz CPU clock (assumed; the
    /// input clock and readback are still to be checked on hardware, test P3). Timer 2 is
    /// the baud clock for the 8274; no timer output reaches an IRQ on the Compass.
    timer_regs: [u8; 4],
    pit_count: [u16; 3],
    pit_lsb_next: [bool; 3],
    pit_rl: [u8; 3],
    #[serde(default)]
    pit_mode: [u8; 3],
    /// CPU clock at which each counter was last loaded.
    #[serde(default)]
    pit_t0: [u64; 3],
    /// Counter latch (counter latch command), read out before the live count.
    #[serde(default)]
    pit_latch: [Option<u16>; 3],
    #[serde(default = "pit_read_lsb_default")]
    pit_read_lsb: [bool; 3],
    // 8741 keyboard (HLE for now)
    kbd_status: u8,
    kbd_queue: std::collections::VecDeque<u8>,
    #[serde(skip)]
    pub kbd_commands: Vec<(u64, u8, bool)>,
    /// Sound DAC, volume DAC and DTMF (modem board).
    pub sound: Sound,
    /// 7220 bubble memory (None = no bubble image given: reads return FFh).
    pub bubble: Option<Bubble>,
    bubble_irq: bool,
    /// 8274 serial controller.
    pub serial: Serial8274,
    serial_irq: bool,
    // Unknown/unhandled accesses
    pub log_unhandled: bool,
    #[serde(skip)]
    pub unhandled: Vec<IoLog>,
    last_vsync_frame: u64,
    vsync_line_high: bool,
    pub gpib: Gpib,
    pub rtc: Rtc,
    /// 8741 keyboard MCU (LLE). When None, a simple HLE queue answers DFFC0/DFFC2.
    pub keyboard: Option<Keyboard>,
    kbd_irq: bool,
    fpu_int: bool,
    /// X2210 machine ID / serial number nibbles (16 bytes).
    pub machine_id: [u8; 16],
    gpib_int: bool,
    /// Longest pseudo-DMA wait per access (stands in for the watchdog), CPU clocks.
    pub dma_wait_limit: u64,
    /// Display timing and RAM contention.
    pub display: DisplayTiming,
    /// The model this board belongs to.
    pub model: GridModel,
    /// End of RAM (256K or 512K).
    ram_top: usize,
    /// Extra wait states on every RAM access (not measured yet; see docs/real-hw-tests.md).
    pub ram_extra_wait: u32,
    /// Split board video/memory control registers DFE00-DFE06 (last written).
    pub split_regs: [u8; 4],
    /// 113x large screen switched on.
    wide: bool,
    /// Next clock at which tick_to updates bubble, GPIB and keyboard.
    #[serde(default)]
    next_slow: u64,
    /// Split board only: also answer the 1101's GPIB DMA window at E0000, so a 1101 boot
    /// PROM can stand in for a missing 1121/1129 PROM.
    pub e0000_dma: bool,
}

impl GridBoard {
    /// Live count of 80130 timer r: down from the loaded value at the CPU clock; modes 2/3
    /// reload (mode 3 counts by 2, twice a period), the others wrap through 0.
    fn pit_now(&self, r: usize) -> u16 {
        let n = if self.pit_count[r] == 0 { 65536u64 } else { self.pit_count[r] as u64 };
        let e = self.clocks.saturating_sub(self.pit_t0[r]);
        match self.pit_mode[r] {
            2 | 6 => (n - e % n) as u16,
            3 | 7 => {
                let half = (n + 1) / 2;
                (n - 2 * (e % half)) as u16 & !1
            }
            _ => (n as u64).wrapping_sub(e) as u16,
        }
    }

    /// A 1101 board. `even` and `odd` are the two 8K PROM chips.
    pub fn new(even: &[u8], odd: &[u8]) -> GridBoard {
        Self::new_model(GridModel::Compass1101, even, odd)
    }

    /// The board of `model`.
    pub fn new_model(model: GridModel, even: &[u8], odd: &[u8]) -> GridBoard {
        assert_eq!(even.len(), odd.len());
        let mut prom = vec![0xFFu8; even.len() * 2];
        for i in 0..even.len() {
            prom[i * 2] = even[i];
            prom[i * 2 + 1] = odd[i];
        }
        GridBoard {
            prom,
            clocks: 0,
            pic: Pic::new(),
            timer_regs: [0; 4],
            pit_count: [0; 3],
            pit_lsb_next: [true; 3],
            pit_rl: [3; 3],
            pit_mode: [0; 3],
            pit_t0: [0; 3],
            pit_latch: [None; 3],
            pit_read_lsb: [true; 3],
            kbd_status: 0,
            kbd_queue: Default::default(),
            kbd_commands: Vec::new(),
            sound: Sound::new(crate::screen::CPU_HZ, 44100),
            bubble: None,
            bubble_irq: false,
            serial: Serial8274::new(),
            serial_irq: false,
            log_unhandled: true,
            unhandled: Vec::new(),
            last_vsync_frame: u64::MAX,
            vsync_line_high: false,
            gpib: Gpib::new(),
            rtc: Rtc::new(),
            keyboard: None,
            kbd_irq: false,
            fpu_int: false,
            machine_id: [0; 16],
            gpib_int: false,
            dma_wait_limit: 20_000_000,
            display: if model.wide_screen() { DisplayTiming::wide() } else { DisplayTiming::default() },
            model,
            ram_top: model.ram_size(),
            ram_extra_wait: 0,
            split_regs: [0; 4],
            wide: false,
            next_slow: 0,
            e0000_dma: false,
        }
    }

    pub fn beam(&self) -> Beam {
        self.display.beam(self.clocks)
    }

    /// The 113x large screen is on (else 320 x 240).
    pub fn wide_mode(&self) -> bool {
        self.wide
    }

    fn split_write(&mut self, address: usize, data: u8) {
        let r = (address >> 1) & 3;
        self.split_regs[r] = data;
        if r == 2 && self.split_regs[0] == 1 && self.model.wide_screen() {
            self.wide = data & 4 == 0;
        }
    }

    fn is_dma_window(&self, address: usize) -> bool {
        if self.model.split_board() {
            (0xDFA00..=0xDFDFF).contains(&address) || (self.e0000_dma && (0xE0000..=0xEFFFF).contains(&address))
        }
        else {
            (0xE0000..=0xEFFFF).contains(&address)
        }
    }

    /// Advance board time by `cycles` CPU clocks.
    pub fn tick(&mut self, cycles: u32) {
        self.tick_to(self.clocks + cycles as u64);
    }

    /// Advance board time to CPU clock `now`.
    pub fn tick_to(&mut self, now: u64) {
        self.clocks = now.max(self.clocks);
        // IR3: rising edge at the start of line ir3_line (vertical blank), low again at frame start.
        let in_frame = self.clocks % self.display.frame_clocks();
        let vblank = in_frame >= self.display.ir3_line * self.display.line_clocks;
        if vblank && !self.vsync_line_high {
            self.vsync_line_high = true;
            self.pic.request_interrupt(IR_VSYNC);
        }
        else if !vblank && self.vsync_line_high {
            self.vsync_line_high = false;
            self.pic.clear_interrupt(IR_VSYNC);
        }
        let _ = self.last_vsync_frame;
        let now = self.clocks;
        if now >= self.sound.next_event() {
            self.sound.advance(now);
        }
        if now >= self.serial.next_event() {
            self.serial.advance(now);
        }
        self.sync_serial_int();
        // Bubble, GPIB and the keyboard MCU in steps of SLOW_STEP clocks (any access to them
        // brings them up to date first, so this only delays their IRQs by up to 13 us).
        if now >= self.next_slow {
            self.next_slow = now + SLOW_STEP;
            if let Some(b) = &mut self.bubble {
                b.advance(now);
            }
            self.sync_bubble_int();
            self.gpib.advance(now);
            self.sync_gpib_int();
            self.kbd_at(0);
        }
    }

    /// Run the keyboard MCU up to the current time and follow its IRQ line.
    fn kbd_at(&mut self, cycles: u32) {
        let t = self.clocks + cycles as u64;
        if let Some(k) = &mut self.keyboard {
            k.run_to(t);
            let l = k.irq();
            if l && !self.kbd_irq {
                self.pic.request_interrupt(IR_KEYBOARD);
            }
            else if !l && self.kbd_irq {
                self.pic.clear_interrupt(IR_KEYBOARD);
            }
            self.kbd_irq = l;
        }
    }

    /// 8087 INT line (IR6).
    pub fn set_fpu_int(&mut self, l: bool) {
        if l && !self.fpu_int {
            self.pic.request_interrupt(IR_8087);
        }
        else if !l && self.fpu_int {
            self.pic.clear_interrupt(IR_8087);
        }
        self.fpu_int = l;
    }

    fn sync_serial_int(&mut self) {
        let l = self.serial.int_line();
        if l && !self.serial_irq {
            self.pic.request_interrupt(IR_SERIAL);
        }
        else if !l && self.serial_irq {
            self.pic.clear_interrupt(IR_SERIAL);
        }
        self.serial_irq = l;
    }

    fn sync_bubble_int(&mut self) {
        let l = self.bubble.as_ref().map(|b| b.drq()).unwrap_or(false);
        if l && !self.bubble_irq {
            self.pic.request_interrupt(IR_BUBBLE);
        }
        else if !l && self.bubble_irq {
            self.pic.clear_interrupt(IR_BUBBLE);
        }
        self.bubble_irq = l;
    }

    fn sync_gpib_int(&mut self) {
        let l = self.gpib.int_line();
        if l && !self.gpib_int {
            self.pic.request_interrupt(IR_GPIB);
        }
        else if !l && self.gpib_int {
            self.pic.clear_interrupt(IR_GPIB);
        }
        self.gpib_int = l;
    }

    fn gpib_at(&mut self, cycles: u32) {
        let t = self.clocks + cycles as u64;
        self.gpib.advance(t);
    }

    /// Queue a byte from the keyboard (HLE): sets OBF and raises IR4.
    pub fn key_byte(&mut self, b: u8) {
        self.kbd_queue.push_back(b);
        self.pic.pulse_interrupt(IR_KEYBOARD);
    }

    fn log(&mut self, address: usize, write: bool, data: u8) {
        if self.log_unhandled && self.unhandled.len() < 100000 && !(address == 0xDFE82 && !write) {
            self.unhandled.push(IoLog { clock: self.clocks, address, write, data });
        }
    }

    pub fn pic(&self) -> &Pic {
        &self.pic
    }
}

impl ExternalBoard for GridBoard {
    fn mem_ranges(&self) -> Vec<(usize, usize)> {
        // Everything above RAM belongs to the board (unmapped space reads FFh).
        vec![(self.ram_top, 0x100000 - self.ram_top)]
    }

    fn io_ports(&self) -> Vec<u16> {
        (0u16..0x10).collect()
    }

    fn mem_read_u8(&mut self, address: usize, _cycles: u32) -> u8 {
        if self.is_dma_window(address) {
            self.gpib_at(_cycles);
            let v = self.gpib.read(7);
            self.sync_gpib_int();
            return v;
        }
        match address {
            0xFC000..=0xFFFFF => self.prom[(address - 0xFC000) % self.prom.len()],
            0xDFFC0..=0xDFFCF if self.keyboard.is_some() => {
                self.kbd_at(_cycles);
                let a0 = (address >> 1) & 1 == 1;
                let v = self.keyboard.as_mut().unwrap().mcu.host_read(a0);
                v
            }
            0xDFFC0..=0xDFFCF => {
                match address & 0xF {
                    0x0 => {
                        let b = self.kbd_queue.pop_front().unwrap_or(0);
                        b
                    }
                    0x2 => {
                        // bit0: data available (8741 OBF), bit1: 8741 busy reading input (IBF)
                        (if self.kbd_queue.is_empty() { 0 } else { 1 }) | self.kbd_status
                    }
                    _ => 0xFF,
                }
            }
            0xDFF80..=0xDFF8F if address & 1 == 0 => {
                self.gpib_at(_cycles);
                let v = self.gpib.read((address & 0xF) >> 1);
                self.sync_gpib_int();
                v
            }
            0xDFF40..=0xDFF5F if address & 1 == 1 => {
                // MAME: RTC in the low nibble, X2210 machine ID nibble in the high nibble.
                let off = (address - 0xDFF40) >> 1;
                let now = self.clocks + _cycles as u64;
                self.rtc.tick(now);
                let idb = self.machine_id[off >> 1];
                let idn = if off & 1 == 1 { idb & 0xF0 } else { (idb & 0x0F) << 4 };
                idn | (self.rtc.read(off) & 0x0F)
            }
            0xC0000..=0xCFFFF => 0xFF,
            0xDFE80..=0xDFE83 if address & 1 == 0 && self.bubble.is_some() => {
                let now = self.clocks + _cycles as u64;
                let v = self.bubble.as_mut().unwrap().read((address >> 1) & 1, now);
                self.sync_bubble_int();
                v
            }
            0xDFF00..=0xDFF1F if address & 1 == 0 => {
                let now = self.clocks + _cycles as u64;
                let v = self.serial.read((address >> 1) & 3, now);
                self.sync_serial_int();
                v
            }
            0xDFEC0..=0xDFECF if address & 1 == 0 => self.sound.read_8255((address >> 1) & 3),
            0xDFE00..=0xDFFFF => {
                self.log(address, false, 0xFF);
                0xFF
            }
            _ => 0xFF,
        }
    }

    fn mem_peek_u8(&self, address: usize) -> u8 {
        match address {
            0xFC000..=0xFFFFF => self.prom[(address - 0xFC000) % self.prom.len()],
            _ => 0xFF,
        }
    }

    fn mem_write_u8(&mut self, address: usize, data: u8, _cycles: u32) {
        if self.is_dma_window(address) {
            self.gpib_at(_cycles);
            self.gpib.write(7, data);
            self.sync_gpib_int();
            return;
        }
        let split = self.model.split_board();
        match address {
            0xDFE00..=0xDFE07 if split && address & 1 == 0 => self.split_write(address, data),
            // serial port select latches: nothing to model (channel A is the only port)
            0xDFE10..=0xDFE1F if split => {}
            // DFE4:2 sound DAC, DFE4:4 volume DAC (SoundTest on a real 1101).
            0xDFE40..=0xDFE4F => {
                let now = self.clocks + _cycles as u64;
                match address & 0xF {
                    0x2 => self.sound.write_dac(data, now),
                    0x4 => self.sound.write_volume(data, now),
                    _ => self.log(address, true, data),
                }
            }
            0xDFEC0..=0xDFECF if address & 1 == 0 => {
                let now = self.clocks + _cycles as u64;
                self.sound.write_8255((address >> 1) & 3, data, now);
            }
            0xDFE80..=0xDFE83 if address & 1 == 0 && self.bubble.is_some() => {
                let now = self.clocks + _cycles as u64;
                self.bubble.as_mut().unwrap().write((address >> 1) & 1, data, now);
                self.sync_bubble_int();
            }
            0xDFF00..=0xDFF1F if address & 1 == 0 => {
                let now = self.clocks + _cycles as u64;
                self.serial.write((address >> 1) & 3, data, now);
                self.sync_serial_int();
            }
            0xDFFC0..=0xDFFCF if self.keyboard.is_some() => {
                self.kbd_at(_cycles);
                let a0 = (address >> 1) & 1 == 1;
                self.kbd_commands.push((self.clocks, data, a0));
                self.keyboard.as_mut().unwrap().mcu.host_write(a0, data);
            }
            0xDFFC0..=0xDFFCF => {
                let cmd = address & 0xF == 0x2;
                self.kbd_commands.push((self.clocks, data, cmd));
            }
            0xDFF40..=0xDFF5F if address & 1 == 1 => {
                let now = self.clocks + _cycles as u64;
                self.rtc.tick(now);
                self.rtc.write((address - 0xDFF40) >> 1, data);
            }
            0xDFF80..=0xDFF8F if address & 1 == 0 => {
                self.gpib_at(_cycles);
                self.gpib.write((address & 0xF) >> 1, data);
                self.sync_gpib_int();
            }
            0xDFE00..=0xDFFFF => self.log(address, true, data),
            _ => {}
        }
    }

    fn mem_wait(&mut self, address: usize, cycles: u32, write: bool) -> u32 {
        if self.is_dma_window(address) {
            self.gpib_at(cycles);
            let lim = self.dma_wait_limit;
            return self.gpib.dma_wait(write, lim) as u32;
        }
        0
    }

    fn ram_wait(&mut self, address: usize, now: u64, write: bool) -> u32 {
        if address < self.ram_top {
            self.display.ram_wait(now, write) + self.ram_extra_wait
        }
        else {
            0
        }
    }

    fn io_read_u8(&mut self, port: u16, _cycles: u32) -> u8 {
        match port {
            0x00 => self.pic.handle_command_register_read(),
            0x02 => self.pic.handle_data_register_read(),
            0x0E => self.timer_regs[3],
            0x08 | 0x0A | 0x0C => {
                let r = ((port - 8) / 2) as usize;
                let v = match self.pit_latch[r] {
                    Some(v) => v,
                    None => self.pit_now(r),
                };
                let b = match self.pit_rl[r] {
                    1 => v as u8,
                    2 => (v >> 8) as u8,
                    _ => {
                        let lsb = self.pit_read_lsb[r];
                        self.pit_read_lsb[r] = !lsb;
                        if lsb { v as u8 } else { (v >> 8) as u8 }
                    }
                };
                // the latch is released once read out completely
                if self.pit_rl[r] != 3 || self.pit_read_lsb[r] {
                    self.pit_latch[r] = None;
                }
                b
            }
            _ => 0xFF,
        }
    }

    fn io_write_u8(&mut self, port: u16, data: u8, _cycles: u32) {
        match port {
            0x00 => self.pic.handle_command_register_write(data),
            0x02 => self.pic.handle_data_register_write(data),
            0x08 | 0x0A | 0x0C | 0x0E => {
                let r = ((port - 8) / 2) as usize;
                self.timer_regs[r] = data;
                if r == 3 {
                    // control word: counter in bits 6-7, read/load mode in bits 4-5
                    let c = (data >> 6) as usize;
                    if c < 3 && (data >> 4) & 3 != 0 {
                        self.pit_rl[c] = (data >> 4) & 3;
                        self.pit_mode[c] = (data >> 1) & 7;
                        self.pit_lsb_next[c] = true;
                        self.pit_read_lsb[c] = true;
                        self.pit_latch[c] = None;
                    }
                    else if c < 3 && self.pit_latch[c].is_none() {
                        // counter latch command
                        self.pit_latch[c] = Some(self.pit_now(c));
                        self.pit_read_lsb[c] = true;
                    }
                }
                else {
                    match self.pit_rl[r] {
                        1 => self.pit_count[r] = data as u16,
                        2 => self.pit_count[r] = (data as u16) << 8,
                        _ => {
                            if self.pit_lsb_next[r] {
                                self.pit_count[r] = (self.pit_count[r] & 0xFF00) | data as u16;
                            }
                            else {
                                self.pit_count[r] = (self.pit_count[r] & 0x00FF) | (data as u16) << 8;
                            }
                            self.pit_lsb_next[r] = !self.pit_lsb_next[r];
                        }
                    }
                    if self.pit_rl[r] != 3 || self.pit_lsb_next[r] {
                        self.pit_t0[r] = self.clocks;
                    }
                    if r == 2 {
                        let now = self.clocks;
                        self.serial.advance(now);
                        let c = self.pit_count[2];
                        self.serial.baud_count = if c == 0 { 65536 } else { c as u32 };
                    }
                }
            }
            _ => {}
        }
    }

    fn intr(&self) -> bool {
        self.pic.query_interrupt_line()
    }

    fn inta(&mut self) -> Option<u8> {
        if self.pic.query_interrupt_line() {
            self.pic.get_interrupt_vector()
        }
        else {
            None
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn pit_read_lsb_default() -> [bool; 3] {
    [true; 3]
}
