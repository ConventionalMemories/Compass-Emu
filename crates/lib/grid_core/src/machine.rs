//! A runnable Compass: MartyPC 8086 core + GridBoard, with a simple run loop.

use marty_core::bus::external::ExternalBoard;
use marty_core::cpu_common::{builder::CpuBuilder, Cpu, CpuDispatch, CpuType};

use crate::board::GridBoard;

#[derive(Copy, Clone, Debug)]
pub enum GridModel {
    Compass1101,
}

pub struct GridMachine {
    pub cpu: CpuDispatch,
    pub model: GridModel,
    pub instructions: u64,
}

impl GridMachine {
    pub fn new(model: GridModel, prom_even: &[u8], prom_odd: &[u8]) -> GridMachine {
        let mut cpu = CpuBuilder::new().with_cpu_type(CpuType::Intel8086).build().expect("cpu");
        // Plain RAM: clear the whole bus memory.
        for a in 0..0x40000usize {
            let _ = cpu.bus_mut().write_u8(a, 0, 0);
        }
        cpu.bus_mut().install_external(Box::new(GridBoard::new(prom_even, prom_odd)));
        cpu.reset();
        GridMachine { cpu, model, instructions: 0 }
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
        let board = self.board_mut();
        board.tick(c);
        let intr = board.intr();
        self.cpu.set_intr(intr);
        c
    }

    /// Run for `clocks` CPU clocks.
    pub fn run_clocks(&mut self, clocks: u64) {
        let end = self.board().clocks + clocks;
        while self.board().clocks < end {
            self.step();
        }
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
