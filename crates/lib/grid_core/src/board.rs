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
//! IO ports 00h/02h: 80130 interrupt controller (8259 compatible); 08h-0Eh: 80130 timers.
//! IRQ wiring (MAME): IR0 serial, IR1 bubble, IR2 modem, IR3 vsync, IR4 keyboard, IR5 GPIB,
//! IR6 8087, IR7 ring.

use std::any::Any;

use marty_core::bus::external::ExternalBoard;
use marty_core::devices::pic::Pic;

use crate::gpib::Gpib;
use crate::rtc::Rtc;
use crate::keyboard::Keyboard;
use crate::screen::{Beam, FRAME_CLOCKS, LINE_CLOCKS, VISIBLE_LINES};

pub const IR_SERIAL: u8 = 0;
pub const IR_BUBBLE: u8 = 1;
pub const IR_MODEM: u8 = 2;
pub const IR_VSYNC: u8 = 3;
pub const IR_KEYBOARD: u8 = 4;
pub const IR_GPIB: u8 = 5;
pub const IR_8087: u8 = 6;

const RAM_TOP: usize = 0x40000;

/// A logged access to a device, for debugging and for Claude's tools.
#[derive(Clone, Debug)]
pub struct IoLog {
    pub clock: u64,
    pub address: usize,
    pub write: bool,
    pub data: u8,
}

pub struct GridBoard {
    prom: Vec<u8>,
    /// CPU clocks since power-on (advanced by tick()).
    pub clocks: u64,
    pic: Pic,
    /// 80130 timer registers, stored only (no 80130 timer output reaches an IRQ on the Compass).
    timer_regs: [u8; 4],
    // 8741 keyboard (HLE for now)
    kbd_status: u8,
    kbd_queue: std::collections::VecDeque<u8>,
    pub kbd_commands: Vec<(u64, u8, bool)>,
    // sound
    pub dac_writes: Vec<(u64, u8)>,
    pub volume: u8,
    // Unknown/unhandled accesses
    pub log_unhandled: bool,
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
}

impl GridBoard {
    /// `even` and `odd` are the two 8K PROM chips.
    pub fn new(even: &[u8], odd: &[u8]) -> GridBoard {
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
            kbd_status: 0,
            kbd_queue: Default::default(),
            kbd_commands: Vec::new(),
            dac_writes: Vec::new(),
            volume: 0,
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
        }
    }

    pub fn beam(&self) -> Beam {
        Beam::at(self.clocks)
    }

    /// Advance board time by `cycles` CPU clocks.
    pub fn tick(&mut self, cycles: u32) {
        self.clocks += cycles as u64;
        // IR3: rising edge at the start of vertical blank (line 240), low again at frame start.
        let in_frame = self.clocks % FRAME_CLOCKS;
        let vblank = in_frame >= VISIBLE_LINES * LINE_CLOCKS;
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
        self.gpib.advance(now);
        self.sync_gpib_int();
        self.kbd_at(0);
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
        vec![(RAM_TOP, 0x100000 - RAM_TOP)]
    }

    fn io_ports(&self) -> Vec<u16> {
        (0u16..0x10).collect()
    }

    fn mem_read_u8(&mut self, address: usize, _cycles: u32) -> u8 {
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
            0xE0000..=0xEFFFF => {
                self.gpib_at(_cycles);
                let v = self.gpib.read(7);
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
        match address {
            0xDFE40..=0xDFE4F => {
                let off = address & 0xF;
                if off & 1 != 0 {
                    self.volume = data;
                }
                if off & 0x6 != 0 && off & 1 == 0 {
                    self.dac_writes.push((self.clocks, data));
                }
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
            0xE0000..=0xEFFFF => {
                self.gpib_at(_cycles);
                self.gpib.write(7, data);
                self.sync_gpib_int();
            }
            0xDFE00..=0xDFFFF => self.log(address, true, data),
            _ => {}
        }
    }

    fn mem_wait(&mut self, address: usize, cycles: u32, write: bool) -> u32 {
        if (0xE0000..=0xEFFFF).contains(&address) {
            self.gpib_at(cycles);
            let lim = self.dma_wait_limit;
            return self.gpib.dma_wait(write, lim) as u32;
        }
        0
    }

    fn io_read_u8(&mut self, port: u16, _cycles: u32) -> u8 {
        match port {
            0x00 => self.pic.handle_command_register_read(),
            0x02 => self.pic.handle_data_register_read(),
            0x08 | 0x0A | 0x0C | 0x0E => self.timer_regs[((port - 8) / 2) as usize],
            _ => 0xFF,
        }
    }

    fn io_write_u8(&mut self, port: u16, data: u8, _cycles: u32) {
        match port {
            0x00 => self.pic.handle_command_register_write(data),
            0x02 => self.pic.handle_data_register_write(data),
            0x08 | 0x0A | 0x0C | 0x0E => self.timer_regs[((port - 8) / 2) as usize] = data,
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
