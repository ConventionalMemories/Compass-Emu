//! A runnable Compass: MartyPC 8086 core + GridBoard, with a simple run loop.

use marty_core::bus::external::ExternalBoard;
use marty_core::cpu_common::{builder::CpuBuilder, Cpu, CpuDispatch, CpuOption, CpuType};

use crate::board::GridBoard;
use crate::fast86::{Bus86, Fast86};
use crate::screen::{self, SCREEN_BASE, SCREEN_H, SCREEN_W, WIDE_H, WIDE_W};

/// The Compass models. The 110x have the original board; the 112x and 113x (Compass II) the
/// split board (video/memory board + CPU board). The 113x have the 512 x 256 screen; the
/// xxx9 models 512K of RAM, the others 256K.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GridModel {
    Compass1101,
    Compass1109,
    Compass1121,
    Compass1129,
    Compass1131,
    Compass1139,
}

impl GridModel {
    pub const ALL: [GridModel; 6] = [
        GridModel::Compass1101,
        GridModel::Compass1109,
        GridModel::Compass1121,
        GridModel::Compass1129,
        GridModel::Compass1131,
        GridModel::Compass1139,
    ];

    /// "1101", "1139" ...
    pub fn name(self) -> &'static str {
        match self {
            GridModel::Compass1101 => "1101",
            GridModel::Compass1109 => "1109",
            GridModel::Compass1121 => "1121",
            GridModel::Compass1129 => "1129",
            GridModel::Compass1131 => "1131",
            GridModel::Compass1139 => "1139",
        }
    }

    /// Accepts "1139", "grid1139", "compass1139".
    pub fn parse(s: &str) -> Option<GridModel> {
        let s = s.trim().to_ascii_lowercase();
        let s = s.trim_start_matches("grid").trim_start_matches("compass");
        GridModel::ALL.iter().copied().find(|m| m.name() == s)
    }

    /// RAM in bytes.
    pub fn ram_size(self) -> usize {
        match self {
            GridModel::Compass1101 | GridModel::Compass1121 | GridModel::Compass1131 => 0x40000,
            _ => 0x80000,
        }
    }

    /// Compass II split board (memory map differs: GPIB DMA window at DFA00, video/memory
    /// control at DFE00, application ROM sockets at 80000).
    pub fn split_board(self) -> bool {
        !matches!(self, GridModel::Compass1101 | GridModel::Compass1109)
    }

    /// 512 x 256 screen (113x).
    pub fn wide_screen(self) -> bool {
        matches!(self, GridModel::Compass1131 | GridModel::Compass1139)
    }

    /// Size of the panel in pixels.
    pub fn screen_size(self) -> (usize, usize) {
        if self.wide_screen() {
            (WIDE_W, WIDE_H)
        }
        else {
            (SCREEN_W, SCREEN_H)
        }
    }
}

pub struct GridMachine {
    pub cpu: CpuDispatch,
    pub model: GridModel,
    pub instructions: u64,
    /// Screen RAM as the display fetched it, word by word as the beam passed (the frame being
    /// drawn now), and the last complete frame.
    beam_frame: Vec<u8>,
    shown_frame: Vec<u8>,
    /// Frames completed so far.
    pub frames: u64,
    /// Next display fetch: absolute CPU clock, line and word.
    next_fetch: u64,
    fetch_line: u64,
    fetch_word: usize,
    /// Fast mode: the fast interpreter runs instead of MartyPC's cycle-exact core (builds).
    fast: Option<Fast86>,
}

/// The fast core's view of the bus: plain RAM direct, the board's memory and ports through
/// MartyPC's bus (with the GPIB DMA window's wait states, so block transfers pace right).
struct FastBus<'a> {
    bus: &'a mut marty_core::bus::BusInterface,
    /// Plain RAM (0 .. ram_top), read and written directly.
    ram: *mut u8,
    ram_top: usize,
    /// Wait clocks met during this instruction.
    waits: u32,
}

impl FastBus<'_> {
    /// Only the board's I/O area (DFxxx, the E0000 DMA window) and the application ROM
    /// window can hold the CPU: skip the wait lookup for the PROM and empty space.
    #[inline]
    fn may_wait(a: usize) -> bool {
        (0xDF000..0xF0000).contains(&a) || (0x80000..0xA0000).contains(&a)
    }
}

impl Bus86 for FastBus<'_> {
    #[inline]
    fn read8(&mut self, a: u32) -> u8 {
        let a = a as usize;
        if a < self.ram_top {
            // SAFETY: `ram` points at the bus's memory (at least ram_top bytes), which is not
            // resized or otherwise touched while this FastBus lives (plain RAM has no mmio).
            return unsafe { *self.ram.add(a) };
        }
        if Self::may_wait(a) {
            self.waits += self.bus.get_read_wait(a, self.waits).unwrap_or(0);
        }
        self.bus.read_u8(a, self.waits).map(|v| v.0).unwrap_or(0xFF)
    }
    #[inline]
    fn write8(&mut self, a: u32, v: u8) {
        let a = a as usize;
        if a < self.ram_top {
            // SAFETY: as in read8
            unsafe { *self.ram.add(a) = v };
            return;
        }
        if Self::may_wait(a) {
            self.waits += self.bus.get_write_wait(a, self.waits).unwrap_or(0);
        }
        let _ = self.bus.write_u8(a, v, self.waits);
    }
    fn io_in8(&mut self, port: u16) -> u8 {
        self.bus.io_read_u8(port, self.waits)
    }
    fn io_out8(&mut self, port: u16, v: u8) {
        self.bus.io_write_u8(port, v, self.waits, None)
    }
    fn intr(&mut self) -> bool {
        self.bus.external().map(|e| e.intr()).unwrap_or(false)
    }
    fn inta(&mut self) -> u8 {
        self.bus.external_mut().and_then(|e| e.inta()).unwrap_or(0xFF)
    }
    fn esc(&mut self, now: u64, opcode: u8, modrm: u8, ea: Option<u32>, ip: u32) -> u32 {
        match self.bus.take_coprocessor() {
            Some(mut cp) => {
                let n = cp.esc(self.bus, now, opcode, modrm, ea, ip);
                self.bus.put_coprocessor(cp);
                n
            }
            None => 0,
        }
    }
    fn fpu_busy(&mut self, now: u64) -> bool {
        self.bus.coprocessor_busy(now)
    }
    fn wait_min(&mut self) -> u64 {
        self.bus.coprocessor_wait_min() as u64
    }
}

impl GridMachine {
    pub fn new(model: GridModel, prom_even: &[u8], prom_odd: &[u8]) -> GridMachine {
        let mut cpu = CpuBuilder::new().with_cpu_type(CpuType::Intel8086).build().expect("cpu");
        // Plain RAM: clear it.
        for a in 0..model.ram_size() {
            let _ = cpu.bus_mut().write_u8(a, 0, 0);
        }
        cpu.bus_mut().install_external(Box::new(GridBoard::new_model(model, prom_even, prom_odd)));
        // Plain data-sheet timing. What makes 8087 code slow on a real Compass is GRiD-OS, not
        // the chip: its loader turns the WAIT before every 8087 instruction into INT 3, whose
        // handler is WAIT + IRET (~95 clocks a time). With that, GRiDBench's 8087 lines match
        // a real 1101 within a few percent. FPU_EXTRA / FPU_WAITMIN override for experiments.
        let mut fpu = i8087::I8087::new();
        fpu.extra_busy = 0;
        fpu.wait_min = 0;
        if let Some(v) = std::env::var("FPU_EXTRA").ok().and_then(|v| v.parse().ok()) {
            fpu.extra_busy = v;
        }
        if let Some(v) = std::env::var("FPU_WAITMIN").ok().and_then(|v| v.parse().ok()) {
            fpu.wait_min = v;
        }
        cpu.bus_mut().install_coprocessor(Box::new(crate::fpu::Fpu(fpu)));
        cpu.reset();
        // Wait states on: display contention and the GPIB pseudo-DMA window need them.
        cpu.set_option(CpuOption::EnableWaitStates(true));
        // Bus steal: the real 1101 loses the bus for ~4 clocks every 48 (DRAM refresh), which
        // only shows when the bus is busy (prefetch-bound code, REP strings). Fitted together
        // with the display contention (screen.rs) to a real 1101's GRiDBench 2.1.
        // GRID_REFRESH=period (0 = off) and GRID_REFRESH_WAIT=clocks override it.
        let env_u32 = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<u32>().ok());
        let period = env_u32("GRID_REFRESH").unwrap_or(48);
        if period > 0 {
            cpu.set_option(CpuOption::ScheduleDramRefresh(true, period, period, true));
            cpu.set_option(CpuOption::DramRefreshAdjust(env_u32("GRID_REFRESH_WAIT").unwrap_or(4)));
        }
        let size = if model.wide_screen() { screen::DisplayTiming::wide() } else { screen::DisplayTiming::default() }.frame_bytes();
        let mut m = GridMachine {
            cpu,
            model,
            instructions: 0,
            beam_frame: vec![0; size],
            shown_frame: vec![0; size],
            frames: 0,
            next_fetch: 0,
            fetch_line: 0,
            fetch_word: 0,
            fast: None,
        };
        m.next_fetch = m.fetch_time(0, 0, 0);
        m
    }

    /// Absolute clock of the fetch of word `w` on line `line` of frame `frame`.
    fn fetch_time(&self, frame: u64, line: u64, w: usize) -> u64 {
        let d = &self.board().display;
        frame * d.frame_clocks() + line * d.line_clocks + d.fetch_clock(w)
    }

    /// Capture the display fetches up to the current time.
    fn run_display(&mut self, now: u64) {
        let d = self.board().display;
        while self.next_fetch <= now {
            let a = (self.fetch_line as usize * d.line_words + self.fetch_word) * 2;
            for i in 0..2 {
                self.beam_frame[a + i] = self.cpu.bus().peek_u8(SCREEN_BASE + a + i).unwrap_or(0);
            }
            let frame = self.next_fetch / d.frame_clocks();
            self.fetch_word += 1;
            if self.fetch_word == d.line_words {
                self.fetch_word = 0;
                self.fetch_line += 1;
                if self.fetch_line == d.visible_lines {
                    self.fetch_line = 0;
                    self.shown_frame.copy_from_slice(&self.beam_frame);
                    self.frames += 1;
                    self.next_fetch = self.fetch_time(frame + 1, 0, 0);
                    continue;
                }
            }
            self.next_fetch = self.fetch_time(frame, self.fetch_line, self.fetch_word);
        }
    }

    /// Fast mode's display capture: the whole screen RAM once per frame (no racing the beam).
    fn run_display_frames(&mut self, now: u64) {
        let d = self.board().display;
        let frame = now / d.frame_clocks();
        let n = self.beam_frame.len();
        let ram = self.cpu.bus().get_slice_at(SCREEN_BASE, n);
        self.beam_frame[..ram.len()].copy_from_slice(ram);
        self.shown_frame.copy_from_slice(&self.beam_frame);
        self.frames += 1;
        self.fetch_line = 0;
        self.fetch_word = 0;
        self.next_fetch = self.fetch_time(frame + 1, 0, 0);
    }

    pub fn board(&self) -> &GridBoard {
        self.cpu.bus().external().unwrap().as_any().downcast_ref::<GridBoard>().unwrap()
    }

    pub fn board_mut(&mut self) -> &mut GridBoard {
        self.cpu.bus_mut().external_mut().unwrap().as_any_mut().downcast_mut::<GridBoard>().unwrap()
    }

    /// Is fast mode on?
    pub fn is_fast(&self) -> bool {
        self.fast.is_some()
    }

    /// Fast mode on or off. Fast mode runs a whole-instruction interpreter with approximate
    /// clock counts (no prefetch queue, wait states or display contention): several times
    /// quicker, for builds, never for timing. Switch between instructions only.
    pub fn set_fast(&mut self, on: bool) {
        // Disk mechanics (seek, rotation) only in exact mode: fast mode is for builds.
        for d in self.board_mut().gpib.drives.iter_mut() {
            d.mechanics = !on;
        }
        if on == self.fast.is_some() {
            return;
        }
        if on {
            let (regs, ip, flags, clocks) = self.exact_regs();
            let mut f = Fast86::new();
            // regs: AX BX CX DX SP BP SI DI ES CS SS DS; Fast86 keeps ModRM order
            f.r = [regs[0], regs[2], regs[3], regs[1], regs[4], regs[5], regs[6], regs[7]];
            f.s = [regs[8], regs[9], regs[10], regs[11]];
            f.ip = ip;
            f.flags = flags;
            f.clocks = clocks;
            self.fast = Some(f);
        }
        else {
            let (regs, ip, flags, clocks) = self.regs();
            self.fast = None;
            self.set_exact_regs(&regs, ip, flags, clocks);
        }
    }

    fn exact_regs(&mut self) -> ([u16; 12], u16, u16, u64) {
        let mut regs = [0; 12];
        for (i, &r) in STATE_REGS.iter().enumerate() {
            regs[i] = self.cpu.get_register16(r);
        }
        (regs, self.cpu.get_ip(), self.cpu.get_flags(), self.cpu.bus().cpu_now())
    }

    /// AX BX CX DX SP BP SI DI ES CS SS DS, IP, flags, clock: from whichever core runs.
    pub fn regs(&mut self) -> ([u16; 12], u16, u16, u64) {
        match &self.fast {
            Some(f) => {
                let r = &f.r;
                ([r[0], r[3], r[1], r[2], r[4], r[5], r[6], r[7], f.s[0], f.s[1], f.s[2], f.s[3]], f.ip, f.flags, f.clocks)
            }
            None => self.exact_regs(),
        }
    }

    fn set_exact_regs(&mut self, regs: &[u16], ip: u16, flags: u16, clocks: u64) {
        self.cpu.set_reset_vector(marty_core::cpu_common::CpuAddress::Segmented(regs[9], ip));
        self.cpu.reset();
        for (&r, &v) in STATE_REGS.iter().zip(regs.iter()) {
            self.cpu.set_register16(r, v);
        }
        self.cpu.set_flags(flags);
        self.cpu.set_cycle_num(clocks);
        let intr = self.board().intr();
        self.cpu.set_intr(intr);
    }

    /// One instruction on the fast core.
    fn step_fast(&mut self) -> u32 {
        let mut f = self.fast.take().unwrap();
        let start = f.clocks;
        let ram_top = self.model.ram_size();
        let bus = self.cpu.bus_mut();
        bus.set_cpu_now(f.clocks);
        let ram = bus.memory_mut();
        assert!(ram.len() >= ram_top);
        let ram = ram.as_mut_ptr();
        let mut fb = FastBus { bus, ram, ram_top, waits: 0 };
        if f.halted {
            // idle until an interrupt: skip ahead in larger steps
            f.step(&mut fb);
            if f.halted {
                f.clocks += 60;
            }
        }
        else {
            f.step(&mut fb);
        }
        f.clocks += fb.waits as u64;
        let now = f.clocks;
        self.instructions += 1;
        self.fast = Some(f);
        let bus = self.cpu.bus_mut();
        bus.set_cpu_now(now);
        let fpu_int = bus.coprocessor().map(|c| c.irq()).unwrap_or(false);
        let board = self.board_mut();
        board.set_fpu_int(fpu_int);
        board.tick_to(now);
        if now >= self.next_fetch {
            self.run_display_frames(now);
        }
        (now - start) as u32
    }

    /// Execute one instruction; returns CPU clocks used.
    pub fn step(&mut self) -> u32 {
        if self.fast.is_some() {
            return self.step_fast();
        }
        let c = match self.cpu.step(false) {
            Ok((_, c)) => c,
            Err(e) => panic!("CPU error: {:?}", e),
        };
        self.instructions += 1;
        let fpu_int = self.cpu.bus().coprocessor().map(|c| c.irq()).unwrap_or(false);
        let now = self.cpu.bus().cpu_now();
        let board = self.board_mut();
        board.set_fpu_int(fpu_int);
        board.tick_to(now);
        let intr = board.intr();
        // Update INTR before step_finish, so an interrupt raised during this instruction is
        // taken at its end (and a REP string op that stopped for it via RPTI is not resumed).
        self.cpu.set_intr(intr);
        let _ = self.cpu.step_finish(None);
        if now >= self.next_fetch {
            self.run_display(now);
        }
        c
    }

    /// The 8087.
    pub fn fpu(&self) -> Option<&i8087::I8087> {
        self.cpu.bus().coprocessor().and_then(|c| c.as_any().downcast_ref::<crate::fpu::Fpu>()).map(|f| &f.0)
    }

    pub fn fpu_mut(&mut self) -> Option<&mut i8087::I8087> {
        self.cpu.bus_mut().coprocessor_mut().and_then(|c| c.as_any_mut().downcast_mut::<crate::fpu::Fpu>()).map(|f| &mut f.0)
    }

    /// Run for `clocks` CPU clocks.
    pub fn run_clocks(&mut self, clocks: u64) {
        let end = self.board().clocks + clocks;
        while self.board().clocks < end {
            self.step();
        }
    }

    /// The last complete frame as the display showed it (screen RAM fetched as the beam
    /// passed, so racing the beam shows as it would on the real screen), in the layout of
    /// screen_ram() (for screen::render).
    pub fn display_frame(&self) -> Vec<u8> {
        let mut v = vec![0u8; SCREEN_BASE];
        v.extend_from_slice(&self.shown_frame);
        v
    }

    /// Screen RAM and everything below it (enough for screen::render).
    pub fn screen_ram(&self) -> Vec<u8> {
        let end = SCREEN_BASE + self.board().display.frame_bytes();
        let mut v = vec![0u8; end];
        for a in SCREEN_BASE..end {
            v[a] = self.cpu.bus().peek_u8(a).unwrap_or(0);
        }
        v
    }

    /// Plain RAM (all of it) as a byte slice copy.
    pub fn ram(&self) -> Vec<u8> {
        (0..self.model.ram_size()).map(|a| self.cpu.bus().peek_u8(a).unwrap_or(0)).collect()
    }

    /// Panel size in pixels (512 x 256 on a 113x, else 320 x 240).
    pub fn screen_size(&self) -> (usize, usize) {
        self.model.screen_size()
    }

    /// The bitmap mode the screen is in now: (width, height) of the picture in screen RAM.
    /// A 113x shows 320 x 240 centred until the OS switches the large screen on.
    pub fn screen_mode(&self) -> (usize, usize) {
        if self.board().wide_mode() {
            (WIDE_W, WIDE_H)
        }
        else {
            (SCREEN_W, SCREEN_H)
        }
    }

    fn render_ram(&self, ram: &[u8]) -> Vec<u8> {
        let (w, h) = self.screen_mode();
        let (pw, ph) = self.screen_size();
        screen::render_into(ram, w, h, pw, ph)
    }

    /// The last complete frame, rendered: one byte per pixel (0/1), screen_size() pixels.
    pub fn render_display(&self) -> Vec<u8> {
        self.render_ram(&self.display_frame())
    }

    /// Screen RAM as it is now, rendered like render_display().
    pub fn render_now(&self) -> Vec<u8> {
        self.render_ram(&self.screen_ram())
    }
}

/// A saved machine state: everything but the drive images (attach them again after
/// loading, so one saved state can be restarted with a changed disk).
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedState {
    format: u32,
    model: GridModel,
    ram: Vec<u8>,
    /// AX BX CX DX SP BP SI DI ES CS SS DS
    regs: Vec<u16>,
    ip: u16,
    flags: u16,
    clocks: u64,
    instructions: u64,
    board: GridBoard,
    fpu: Option<i8087::I8087>,
    beam_frame: Vec<u8>,
    shown_frame: Vec<u8>,
    frames: u64,
    next_fetch: u64,
    fetch_line: u64,
    fetch_word: usize,
}

const STATE_FORMAT: u32 = 1;
const STATE_REGS: [marty_core::cpu_common::Register16; 12] = {
    use marty_core::cpu_common::Register16::*;
    [AX, BX, CX, DX, SP, BP, SI, DI, ES, CS, SS, DS]
};

impl GridMachine {
    /// Save the whole machine (between instructions) except the drive images.
    pub fn save_state(&mut self) -> Vec<u8> {
        let ram = self.ram();
        let (regs, ip, flags, _) = self.regs();
        let regs = regs.to_vec();
        let board = self.board();
        let st = SavedState {
            format: STATE_FORMAT,
            model: self.model,
            ram,
            regs,
            ip,
            flags,
            clocks: board.clocks,
            instructions: self.instructions,
            board: serde_json::from_value(serde_json::to_value(board).unwrap()).unwrap(),
            fpu: self.fpu().map(|f| serde_json::from_value(serde_json::to_value(f).unwrap()).unwrap()),
            beam_frame: self.beam_frame.clone(),
            shown_frame: self.shown_frame.clone(),
            frames: self.frames,
            next_fetch: self.next_fetch,
            fetch_line: self.fetch_line,
            fetch_word: self.fetch_word,
        };
        serde_json::to_vec(&st).unwrap()
    }

    /// Load a state saved by save_state. Drive images are empty: set them with
    /// `board_mut().gpib.drives[i].image` before running.
    pub fn load_state(data: &[u8]) -> Result<GridMachine, String> {
        let st: SavedState = serde_json::from_slice(data).map_err(|e| format!("not a saved Compass state: {e}"))?;
        if st.format != STATE_FORMAT {
            return Err(format!("saved state format {} (this version reads {STATE_FORMAT})", st.format));
        }
        let prom = vec![0xF4u8; 8192];
        let mut m = GridMachine::new(st.model, &prom, &prom);
        for (a, &b) in st.ram.iter().enumerate() {
            let _ = m.cpu.bus_mut().write_u8(a, b, 0);
        }
        m.cpu.bus_mut().install_external(Box::new(st.board));
        if let (Some(f), Some(saved)) = (m.fpu_mut(), st.fpu) {
            *f = saved;
        }
        m.set_exact_regs(&st.regs, st.ip, st.flags, st.clocks);
        m.instructions = st.instructions;
        m.beam_frame = st.beam_frame;
        m.shown_frame = st.shown_frame;
        m.frames = st.frames;
        m.next_fetch = st.next_fetch;
        m.fetch_line = st.fetch_line;
        m.fetch_word = st.fetch_word;
        Ok(m)
    }
}
