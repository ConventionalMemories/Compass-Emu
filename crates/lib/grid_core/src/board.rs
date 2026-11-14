//! GRiD Compass 1101 motherboard and peripherals as one ExternalBoard.
//!
//! Address map (1101, from MAME gridcomp.cpp grid1101_map):
//!   00000-3FFFF  RAM (plain bus memory, not claimed here; contention via ram_wait)
//!   C0000-CFFFF  test ROM socket (empty -> FF)
//!   DFE40-DFE4F  sound DACs (DFE42/DFE44 sound, odd = volume)
//!   DFE80-DFE83  Intel 7220 bubble controller (even bytes)
//!   DFEC0-DFECF  8255 modem interface (even bytes; not on the 1137, which has no modem)
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
//! IO ports 00h/02h: interrupt controller (80130 on the 110x/112x, 8259A on the 113x);
//! 08h-0Eh: timers (80130, or an 8254 on the 113x; see timers.rs).
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
use crate::machine::{GridModel, TimerChip};
use crate::timers::Timers;
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
    /// Timers at ports 08h-0Eh: the 80130's (110x/112x) or a discrete 8254 (113x), see
    /// timers.rs. Timer 2 is the baud clock for the 8274; no timer output reaches an IRQ.
    pub timers: Timers,
    // 8741 keyboard (HLE for now)
    kbd_status: u8,
    kbd_queue: std::collections::VecDeque<u8>,
    #[serde(skip)]
    pub kbd_commands: Vec<(u64, u8, bool)>,
    /// Sound DAC, volume DAC and DTMF (modem board; 1137 motherboard).
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
    /// Longest pseudo-DMA wait per access, CPU clocks: a backstop behind the keyboard
    /// watchdog (about 4 s), which normally ends a hung transfer first.
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
    /// Serial port select latches ST0-ST3 (last written): DFE10/12/14/16 on the split board,
    /// DFE00/02/04/06 on the 1101 board. Patent FIG. 12: ST0-ST2 pick the clock sources and
    /// the RS-422 transmit driver; GRiDLink multidrop sets ST0 (DFE10) only while sending.
    #[serde(default)]
    pub serial_select: [u8; 4],
    /// 113x large screen switched on.
    wide: bool,
    /// Next clock at which tick_to updates bubble, GPIB and keyboard.
    #[serde(default)]
    next_slow: u64,
    /// Split board only: also answer the 1101's GPIB DMA window at E0000, so a 1101 boot
    /// PROM can stand in for a missing 1121/1129 PROM.
    pub e0000_dma: bool,
    /// Cycle counter marks (OUT 0F0h/0F2h/0F4h, see marks.rs).
    #[serde(skip)]
    pub marks: crate::marks::Marks,
    /// Breakpoints (see debug.rs); the board reports port and interrupt hits.
    #[serde(skip)]
    pub debug: crate::debug::Debugger,
    /// Split board application ROM sockets 0-3 (chip images; empty = no chip).
    #[serde(skip)]
    pub rom_slots: [Vec<u8>; 4],
    /// Socket registers: DFE08/0A/0C/0E quarter codes (slot | wait code << 2, bit 7 =
    /// 32K part), DFEA2 32K mode, DFEA8 enable.
    #[serde(default)]
    rom_quarter: [u8; 4],
    #[serde(default)]
    rom_32k: u8,
    #[serde(default)]
    rom_enable: u8,
}

impl GridBoard {
    /// Timers for the debugger: (live count, loaded value, mode) of each.
    pub fn timer_state(&self) -> [(u16, u16, u8); 3] {
        self.timers.state(self.clocks)
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
            timers: Timers::new(model.timer_chip()),
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
            dma_wait_limit: 100_000_000,
            display: if model.board_113x() { DisplayTiming::wide() } else { DisplayTiming::default() },
            model,
            ram_top: model.ram_size(),
            ram_extra_wait: 0,
            split_regs: [0; 4],
            serial_select: [0; 4],
            wide: false,
            next_slow: 0,
            e0000_dma: false,
            marks: Default::default(),
            debug: Default::default(),
            rom_slots: Default::default(),
            rom_quarter: [0; 4],
            rom_32k: 0,
            rom_enable: 0,
        }
    }

    pub fn beam(&self) -> Beam {
        self.display.beam(self.clocks)
    }

    /// The 113x large screen is on (else 320 x 240).
    pub fn wide_mode(&self) -> bool {
        self.wide
    }

    /// Split board with DFE00 = 1 (video chip selected): the real 1137 shows a dark screen
    /// until the OS writes DFE00 = 0 again (GRiDProbe).
    pub fn screen_dark(&self) -> bool {
        self.model.split_board() && self.split_regs[0] & 1 != 0
    }

    fn split_write(&mut self, address: usize, data: u8) {
        let r = (address >> 1) & 3;
        self.split_regs[r] = data;
        if r == 2 && self.split_regs[0] == 1 && self.model.wide_screen() {
            self.wide = data & 4 == 0;
        }
    }

    /// CPU clocks in one display frame.
    pub fn frame_clocks(&self) -> u64 {
        self.display.frame_clocks()
    }

    /// Put a chip image in application ROM socket `slot` (0-3).
    pub fn insert_rom(&mut self, slot: usize, chip: Vec<u8>) -> Result<(), String> {
        if !self.model.split_board() {
            return Err(format!("the {} has no application ROM sockets", self.model.name()));
        }
        if slot > 3 {
            return Err("sockets are numbered 0 to 3".into());
        }
        if !matches!(chip.len(), 0x8000 | 0x10000 | 0x20000) {
            return Err(format!("a ROM chip is 32, 64 or 128 KB, not {} bytes", chip.len()));
        }
        self.rom_slots[slot] = chip;
        Ok(())
    }

    /// A byte of the application ROM window 80000-9FFFF (split boards). Each 32K quarter
    /// shows the socket its register selects. A 128K chip shows its quarter q in window
    /// quarter q; a 64K chip fills the top half (Rom Toolbox on a real 1137: the lower half
    /// reads FFh); a 32K (28-pin) part has no A15/A16 and shows in every quarter.
    fn rom_read(&self, address: usize) -> u8 {
        if self.rom_enable & 1 == 0 {
            return 0xFF;
        }
        let w = address - 0x80000;
        let q = w >> 15;
        let chip = &self.rom_slots[(self.rom_quarter[q] & 3) as usize];
        let n = chip.len();
        if n == 0 {
            0xFF
        }
        else if n <= 0x8000 {
            chip[(w & 0x7FFF) % n]
        }
        else if w + n >= 0x20000 {
            chip[(w + n - 0x20000) % n]
        }
        else {
            0xFF
        }
    }

    /// Wait states of a ROM window access: the quarter's code holds (3 - waits) * 4.
    fn rom_waits(&self, address: usize) -> u32 {
        let q = (address - 0x80000) >> 15;
        3 - ((self.rom_quarter[q] >> 2) & 3) as u32
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
        let elapsed = now.saturating_sub(self.clocks);
        self.clocks = now.max(self.clocks);
        // The 8259 raises INT for a request that is pending when its IR is unmasked (or a
        // lower one after an EOI) from its own run step; without it such a request waited
        // for the next unrelated interrupt (each disk command waited for an IR3 tick).
        if elapsed > 0 {
            self.pic.run(elapsed.min(u32::MAX as u64) as u32);
        }
        // IR3: high from the start of line ir3_line for ir3_len lines.
        let d = &self.display;
        let line = (self.clocks % d.frame_clocks()) / d.line_clocks;
        let vblank = (line + d.frame_lines - d.ir3_line) % d.frame_lines < d.ir3_len;
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

    /// Clocks a GPIB DMA window access holds the CPU: until the 9914 is ready (or
    /// interrupts), or the 8741 watchdog drops kbdDma (P2.4, through PAL U90), checked every
    /// KBD_DMA_STEP clocks. With kbdDma low the access doesn't wait at all. dma_wait_limit
    /// stays as a backstop (no keyboard MCU, or a watchdog that was never armed).
    fn dma_hold(&mut self, cycles: u32, write: bool) -> u32 {
        const KBD_DMA_STEP: u64 = 5_000;
        if self.keyboard.as_ref().is_some_and(|k| !k.dma_enabled()) {
            return 0;
        }
        let lim = self.dma_wait_limit;
        let mut waited = 0u64;
        loop {
            let chunk = (lim - waited).min(KBD_DMA_STEP);
            let w = self.gpib.dma_wait(write, chunk);
            waited += w;
            if w < chunk || waited >= lim || self.keyboard.is_none() {
                break;
            }
            self.kbd_at(cycles + waited as u32);
            if !self.keyboard.as_ref().unwrap().dma_enabled() {
                break;
            }
        }
        if waited > 1_000_000 && std::env::var_os("GPIB_STALL").is_some() {
            let kd = self.keyboard.as_ref().map(|k| k.dma_enabled());
            eprintln!("DMA hold {:.3} s ended at {:.2} s, kbdDma {:?}", waited as f64 / 5e6, (self.clocks + cycles as u64 + waited) as f64 / 5e6, kd);
        }
        waited as u32
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
        // The 80130 decodes only A1-A3 below 80h (real 1101: mirrored every 10h); the rest
        // of the 8-bit port space reads the address left on the bus.
        (0u16..0x100).collect()
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
                idn | (self.rtc.read(off, now) & 0x0F)
            }
            0x80000..=0x9FFFF if self.model.split_board() => self.rom_read(address),
            // The 7220 answers at DFE80/82; on the split board (real 1137, GRiDBench 2.2 u40) it
            // repeats every 4 bytes up to DFE8F.
            0xDFE80..=0xDFE8F
                if address & 1 == 0 && self.bubble.is_some() && (address <= 0xDFE83 || self.model.split_board()) =>
            {
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
            // The modem board's 8255. The 1137 has no modem and no 8255: FFh as elsewhere (so
            // Modem~Device~ finds no modem, as on the real machine).
            0xDFEC0..=0xDFECF if address & 1 == 0 && self.model.has_modem() => self.sound.read_8255((address >> 1) & 3),
            0xDFE00..=0xDFFFF => {
                self.log(address, false, 0xFF);
                0xFF
            }
            // Real 1137 (GRiDBench 2.2 u01-u09): the PROM shows again at F8000-FBFFF only, and
            // everything else unclaimed reads the address left on the bus (no FF lane at D0000).
            0xF8000..=0xFBFFF if self.model.split_board() => self.prom[(address - 0xF8000) % self.prom.len()],
            _ if self.model.split_board() => bus_value(address),
            // Real 1101 (GRiDBench 2.0, U1): the PROM also answers at F0000-FBFFF; D0000-DEFFF
            // pulls the even byte lane to FF; elsewhere nothing drives the bus and a read gets
            // the address still on it (A0-A7 on even bytes, A8-A15 on odd ones).
            0xF0000..=0xFBFFF => self.prom[(address - 0xF0000) % self.prom.len()],
            0xD0000..=0xDEFFF if address & 1 == 0 => 0xFF,
            _ => bus_value(address),
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
            // serial port select latches (the link reads ST0 as the transmit driver)
            0xDFE10..=0xDFE17 if split && address & 1 == 0 => self.serial_select[(address >> 1) & 3] = data,
            0xDFE10..=0xDFE1F if split => {}
            0xDFE00..=0xDFE07 if !split && address & 1 == 0 => self.serial_select[(address >> 1) & 3] = data,
            0xDFE08..=0xDFE0F if split && address & 1 == 0 => self.rom_quarter[(address >> 1) & 3] = data,
            0xDFEA2 if split => self.rom_32k = data,
            0xDFEA8 if split => self.rom_enable = data,
            // DFE4:2 sound DAC, DFE4:4 volume DAC (SoundTest on a real 1101).
            0xDFE40..=0xDFE4F => {
                let now = self.clocks + _cycles as u64;
                match address & 0xF {
                    0x2 => self.sound.write_dac(data, now),
                    0x4 => self.sound.write_volume(data, now),
                    _ => self.log(address, true, data),
                }
            }
            0xDFEC0..=0xDFECF if address & 1 == 0 && self.model.has_modem() => {
                let now = self.clocks + _cycles as u64;
                self.sound.write_8255((address >> 1) & 3, data, now);
            }
            // 1137: the DACs and the MK5089 are on the motherboard, but how the MK5089 is
            // driven is not known (no 8255; SoundTest on a real 1137 would tell). ASSUMED: a
            // write-only latch at the 8255's port B address with the same bits, so programs
            // written for the modem board's DTMF still sound. Nothing else answers here.
            0xDFEC2 => {
                let now = self.clocks + _cycles as u64;
                self.sound.write_8255(1, data, now);
            }
            0xDFEC0..=0xDFECF => {}
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
            return self.dma_hold(cycles, write);
        }
        // RTC/ID chip: slow on a real 1101 (GRiDBench 2.0 r15: a read loop turn 87.7 clocks
        // against 77.1 without waits).
        if (0xDFF40..=0xDFF5F).contains(&address) {
            return RTC_WAIT;
        }
        if (0x80000..=0x9FFFF).contains(&address) && self.model.split_board() && address >= self.ram_top {
            return self.rom_waits(address);
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
        if self.debug.ports.contains(&port) {
            self.debug.hit = Some(format!("port {port:02X}h read"));
        }
        // 80130 (110x/112x): ports 4/6 are not decoded and 0E reads FF (real 1101). 8254 +
        // 8259A (113x, real 1137): the 8259A decodes A1 only, so 4/6 answer like 0/2, and the
        // 8254's control port 0E reads the bus.
        let i8254 = self.model.timer_chip() == TimerChip::I8254;
        // The 8254 doesn't look at A0: a byte read at 09/0B/0D strobes the counter too (its
        // byte goes on the low lane, the CPU takes the bus's high lane). Real 1137 GRiDBench
        // 2.2 u20-u27: reading ports 00-0F in a row gives timer 1 and 2's low byte every time.
        if i8254 && port < 0x80 && matches!(port & 0x0F, 9 | 0x0B | 0x0D) {
            let now = self.clocks + _cycles as u64;
            self.timers.read((((port & 0x0F) - 9) / 2) as usize, now);
        }
        if port >= 0x80 || port & 1 == 1 || (!i8254 && matches!(port & 0x0F, 4 | 6)) || (i8254 && port & 0x0F == 0x0E) {
            return bus_value(port as usize);
        }
        let port = if i8254 && matches!(port & 0x0F, 4 | 6) { port & 0x0B } else { port & 0x0F };
        match port {
            0x00 => self.pic.handle_command_register_read(),
            0x02 => self.pic.handle_data_register_read(),
            // the 80130's control register is write-only (real 1101 reads FF)
            0x0E => 0xFF,
            0x08 | 0x0A | 0x0C => {
                let now = self.clocks + _cycles as u64;
                self.timers.read(((port - 8) / 2) as usize, now)
            }
            _ => 0xFF,
        }
    }

    fn io_write_u8(&mut self, port: u16, data: u8, _cycles: u32) {
        if self.debug.ports.contains(&port) {
            self.debug.hit = Some(format!("port {port:02X}h written: {data:02X}"));
        }
        if port >= 0x80 {
            // Not decoded on a real Compass: the emulator's cycle counter marks.
            let now = self.clocks + _cycles as u64;
            match port {
                crate::marks::PORT_START => self.marks.start(data, now),
                crate::marks::PORT_END => self.marks.end(data, now),
                crate::marks::PORT_FRAME => {
                    let d = &self.display;
                    let fc = d.frame_clocks();
                    let t = now.saturating_sub(d.ir3_line * d.line_clocks);
                    let line = (now % fc) / d.line_clocks;
                    self.marks.frame_mark(t % fc, t / fc, line);
                }
                _ => {}
            }
            return;
        }
        let port = port & 0x0F;
        match port {
            // A non-specific EOI (OCW2 20h) leaves the in-service bit set on a real 1101's
            // 80130 (GRiDProbe B5); GRiD-OS sends specific EOIs (60h + level). The 113x's
            // discrete 8259A is assumed to take it like any 8259A (not tested on a real 1137).
            0x00 if data & 0xF8 == 0x20 && self.model.timer_chip() == TimerChip::I80130 => {}
            0x00 => self.pic.handle_command_register_write(data),
            0x02 => self.pic.handle_data_register_write(data),
            0x0E => {
                let now = self.clocks + _cycles as u64;
                self.timers.control(data, now);
            }
            0x08 | 0x0A | 0x0C => {
                let r = ((port - 8) / 2) as usize;
                let now = self.clocks + _cycles as u64;
                if self.timers.write(r, data, now) && r == 2 {
                    let now = self.clocks;
                    self.serial.advance(now);
                    let c = self.timers.count[2];
                    self.serial.baud_count = if c == 0 { 65536 } else { c as u32 };
                }
            }
            _ => {}
        }
    }

    fn intr(&self) -> bool {
        self.pic.query_interrupt_line()
    }

    fn intr_now(&self) -> bool {
        self.pic.query_interrupt_line() || self.pic.calc_intr().0
    }

    fn inta(&mut self) -> Option<u8> {
        if self.pic.query_interrupt_line() {
            let v = self.pic.get_interrupt_vector();
            if let Some(v) = v {
                if self.debug.irqs.contains(&(v & 7)) {
                    self.debug.hit = Some(format!("interrupt IR{} (vector {v:02X}h) taken", v & 7));
                }
            }
            v
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

/// Wait states of an access to the RTC/ID chip at DFF40-DFF5F.
const RTC_WAIT: u32 = 8;

/// What an undriven bus reads: the address byte still on that byte lane.
fn bus_value(address: usize) -> u8 {
    if address & 1 == 0 { address as u8 } else { (address >> 8) as u8 }
}
