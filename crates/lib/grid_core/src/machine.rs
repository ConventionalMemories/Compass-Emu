//! A runnable Compass: MartyPC 8086 core + GridBoard, with a simple run loop.

use marty_core::bus::external::ExternalBoard;
use marty_core::cpu_common::{builder::CpuBuilder, Cpu, CpuDispatch, CpuOption, CpuType};

use crate::board::GridBoard;
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
}

impl GridMachine {
    pub fn new(model: GridModel, prom_even: &[u8], prom_odd: &[u8]) -> GridMachine {
        let mut cpu = CpuBuilder::new().with_cpu_type(CpuType::Intel8086).build().expect("cpu");
        // Plain RAM: clear it.
        for a in 0..model.ram_size() {
            let _ = cpu.bus_mut().write_u8(a, 0, 0);
        }
        cpu.bus_mut().install_external(Box::new(GridBoard::new_model(model, prom_even, prom_odd)));
        // Calibrated against GRiDBench on a real 1101 (times scaled by its AAM, with display
        // contention on): every 8087 instruction takes ~35 clocks more than its data-sheet
        // time there, and a bare FWAIT costs ~90 clocks.
        let mut fpu = i8087::I8087::new();
        fpu.extra_busy = 35;
        fpu.wait_min = 89;
        cpu.bus_mut().install_coprocessor(Box::new(crate::fpu::Fpu(fpu)));
        cpu.reset();
        // Wait states on: display contention and the GPIB pseudo-DMA window need them.
        cpu.set_option(CpuOption::EnableWaitStates(true));
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

    pub fn board(&self) -> &GridBoard {
        self.cpu.bus().external().unwrap().as_any().downcast_ref::<GridBoard>().unwrap()
    }

    pub fn board_mut(&mut self) -> &mut GridBoard {
        self.cpu.bus_mut().external_mut().unwrap().as_any_mut().downcast_mut::<GridBoard>().unwrap()
    }

    /// Execute one instruction; returns CPU clocks used.
    pub fn step(&mut self) -> u32 {
        let c = match self.cpu.step(false) {
            Ok((_, c)) => c,
            Err(e) => panic!("CPU error: {:?}", e),
        };
        let _ = self.cpu.step_finish(None);
        self.instructions += 1;
        let fpu_int = self.cpu.bus().coprocessor().map(|c| c.irq()).unwrap_or(false);
        let now = self.cpu.bus().cpu_now();
        let board = self.board_mut();
        board.set_fpu_int(fpu_int);
        board.tick_to(now);
        let intr = board.intr();
        self.cpu.set_intr(intr);
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
        let regs = STATE_REGS.iter().map(|&r| self.cpu.get_register16(r)).collect();
        let ip = self.cpu.get_ip();
        let flags = self.cpu.get_flags();
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
        m.cpu.set_reset_vector(marty_core::cpu_common::CpuAddress::Segmented(st.regs[9], st.ip));
        m.cpu.reset();
        for (&r, &v) in STATE_REGS.iter().zip(st.regs.iter()) {
            m.cpu.set_register16(r, v);
        }
        m.cpu.set_flags(st.flags);
        m.cpu.set_cycle_num(st.clocks);
        m.instructions = st.instructions;
        m.beam_frame = st.beam_frame;
        m.shown_frame = st.shown_frame;
        m.frames = st.frames;
        m.next_fetch = st.next_fetch;
        m.fetch_line = st.fetch_line;
        m.fetch_word = st.fetch_word;
        let intr = m.board().intr();
        m.cpu.set_intr(intr);
        Ok(m)
    }
}
