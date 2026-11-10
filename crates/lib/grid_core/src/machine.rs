//! A runnable Compass: MartyPC 8086 core + GridBoard, with a simple run loop.

use marty_core::bus::external::ExternalBoard;
use marty_core::cpu_common::{builder::CpuBuilder, Cpu, CpuDispatch, CpuOption, CpuType};

use crate::board::GridBoard;
use crate::screen::{FRAME_CLOCKS, LINE_CLOCKS, LINE_WORDS, SCREEN_BASE, SCREEN_H, SCREEN_W, VISIBLE_LINES};

#[derive(Copy, Clone, Debug)]
pub enum GridModel {
    Compass1101,
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
        // Plain RAM: clear the whole bus memory.
        for a in 0..0x40000usize {
            let _ = cpu.bus_mut().write_u8(a, 0, 0);
        }
        cpu.bus_mut().install_external(Box::new(GridBoard::new(prom_even, prom_odd)));
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
        let size = SCREEN_W * SCREEN_H / 8;
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
        frame * FRAME_CLOCKS + line * LINE_CLOCKS + self.board().display.fetch_clock(w)
    }

    /// Capture the display fetches up to the current time.
    fn run_display(&mut self, now: u64) {
        while self.next_fetch <= now {
            let a = (self.fetch_line as usize * LINE_WORDS + self.fetch_word) * 2;
            for i in 0..2 {
                self.beam_frame[a + i] = self.cpu.bus().peek_u8(SCREEN_BASE + a + i).unwrap_or(0);
            }
            let frame = self.next_fetch / FRAME_CLOCKS;
            self.fetch_word += 1;
            if self.fetch_word == LINE_WORDS {
                self.fetch_word = 0;
                self.fetch_line += 1;
                if self.fetch_line == VISIBLE_LINES {
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
        let end = crate::screen::SCREEN_BASE + crate::screen::SCREEN_W * crate::screen::SCREEN_H / 8;
        let mut v = vec![0u8; end];
        for a in crate::screen::SCREEN_BASE..end {
            v[a] = self.cpu.bus().peek_u8(a).unwrap_or(0);
        }
        v
    }

    /// Plain RAM (first 256K) as a byte slice copy.
    pub fn ram(&self) -> Vec<u8> {
        (0..0x40000usize).map(|a| self.cpu.bus().peek_u8(a).unwrap_or(0)).collect()
    }
}
