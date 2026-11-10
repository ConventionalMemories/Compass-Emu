//! Compass keyboard: the 8741A running GRiD's firmware (300067-01 on the 1101, 300001-02 on the
//! 1137), scanning an 8x8 key matrix. Wiring per MAME gridkeyb.cpp: P2.0-2 select the column,
//! P1 reads it (active low), Ctrl is on P1.0 of column 2, T0 = Shift, T1 = Code (active low),
//! P2.3 = IRQ to the 80130 (IR4), P2.4 = DMA (unused), P2.6 = NMI (not connected in MAME).

use crate::upi41::{Upi41, UpiPorts};

/// CPU clocks per 8741 machine cycle (5 MHz crystal / 15, CPU at 5 MHz).
pub const CLOCKS_PER_CYCLE: u64 = 15;

pub const MOD_CTRL: u8 = 0x01;
pub const MOD_SHIFT: u8 = 0x02;
pub const MOD_CODE: u8 = 0x08;

#[derive(serde::Serialize, serde::Deserialize)]
struct Matrix {
    cols: [u8; 8],
    mods: u8,
    p2: u8,
}

impl UpiPorts for Matrix {
    fn p1_in(&mut self, p2_out: u8) -> u8 {
        let col = (p2_out & 7) as usize;
        let mut d = !self.cols[col];
        if col == 2 && self.mods & MOD_CTRL != 0 {
            d &= !0x01;
        }
        d
    }
    fn p2_out(&mut self, v: u8) {
        self.p2 = v;
    }
    fn t0(&mut self) -> bool {
        self.mods & MOD_SHIFT == 0
    }
    fn t1(&mut self) -> bool {
        self.mods & MOD_CODE == 0
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Keyboard {
    pub mcu: Upi41,
    m: Matrix,
    /// CPU clock the MCU has run up to.
    pub time: u64,
}

/// Matrix position (column, bit) for a character, plus whether Shift is needed.
pub fn key_for_char(c: char) -> Option<(usize, u8, bool)> {
    const MAP: [(usize, u8, char, char); 58] = [
        (0, 0x01, '\t', '\t'),
        (0, 0x02, '\x1b', '\x1b'),
        (1, 0x01, '1', '!'),
        (1, 0x02, '/', '?'),
        (1, 0x04, '\r', '\r'),
        (1, 0x08, '\'', '"'),
        (1, 0x20, '-', '_'),
        (1, 0x40, 'i', 'I'),
        (1, 0x80, '9', '('),
        (2, 0x02, 'm', 'M'),
        (2, 0x04, '.', '>'),
        (2, 0x08, ',', '<'),
        (2, 0x10, ';', ':'),
        (2, 0x20, 'p', 'P'),
        (2, 0x40, 'u', 'U'),
        (2, 0x80, '8', '*'),
        (3, 0x02, 'v', 'V'),
        (3, 0x04, 'n', 'N'),
        (3, 0x08, 'b', 'B'),
        (3, 0x10, 'l', 'L'),
        (3, 0x20, 'k', 'K'),
        (3, 0x40, 'y', 'Y'),
        (3, 0x80, '7', '&'),
        (4, 0x02, 'q', 'Q'),
        (4, 0x04, 'c', 'C'),
        (4, 0x08, 'x', 'X'),
        (4, 0x10, 'j', 'J'),
        (4, 0x20, 'h', 'H'),
        (4, 0x40, 't', 'T'),
        (4, 0x80, '6', '^'),
        (5, 0x02, 'z', 'Z'),
        (5, 0x04, 'a', 'A'),
        (5, 0x08, 's', 'S'),
        (5, 0x10, 'g', 'G'),
        (5, 0x20, 'f', 'F'),
        (5, 0x40, 'r', 'R'),
        (5, 0x80, '5', '%'),
        (6, 0x04, '2', '@'),
        (6, 0x08, 'w', 'W'),
        (6, 0x10, 'd', 'D'),
        (6, 0x20, 'e', 'E'),
        (6, 0x40, '3', '#'),
        (6, 0x80, '4', '$'),
        (7, 0x01, ' ', ' '),
        (7, 0x02, '\x08', '\x08'),
        (7, 0x20, '=', '+'),
        (7, 0x40, 'o', 'O'),
        (7, 0x80, '0', ')'),
        // named keys via private-use chars: arrows
        (1, 0x10, '\u{e000}', '\u{e000}'), // left
        (7, 0x04, '\u{e001}', '\u{e001}'), // down
        (7, 0x08, '\u{e002}', '\u{e002}'), // right
        (7, 0x10, '\u{e003}', '\u{e003}'), // up
        (0, 0x00, '\0', '\0'),
        (0, 0x00, '\0', '\0'),
        (0, 0x00, '\0', '\0'),
        (0, 0x00, '\0', '\0'),
        (0, 0x00, '\0', '\0'),
        (0, 0x00, '\0', '\0'),
    ];
    if c == '\0' {
        return None;
    }
    for &(col, bit, lo, hi) in MAP.iter() {
        if bit == 0 {
            continue;
        }
        if c == lo {
            return Some((col, bit, false));
        }
        if c == hi {
            return Some((col, bit, true));
        }
    }
    None
}

impl Keyboard {
    pub fn new(rom: Vec<u8>) -> Keyboard {
        Keyboard { mcu: Upi41::new(rom), m: Matrix { cols: [0; 8], mods: 0, p2: 0xFF }, time: 0 }
    }

    /// Run the MCU up to CPU clock `t`.
    pub fn run_to(&mut self, t: u64) {
        while self.time + CLOCKS_PER_CYCLE <= t {
            let c = self.mcu.step(&mut self.m);
            self.time += c as u64 * CLOCKS_PER_CYCLE;
        }
    }

    pub fn irq(&self) -> bool {
        self.mcu.p2_pins() & 0x08 != 0
    }

    pub fn nmi(&self) -> bool {
        self.mcu.p2_pins() & 0x40 != 0
    }

    pub fn set_key(&mut self, col: usize, bit: u8, down: bool) {
        if down {
            self.m.cols[col] |= bit
        }
        else {
            self.m.cols[col] &= !bit
        }
    }

    pub fn set_mods(&mut self, mods: u8) {
        self.m.mods = mods;
    }

    pub fn mods(&self) -> u8 {
        self.m.mods
    }

    /// Set the whole matrix and modifiers at once (window frontends).
    pub fn set_matrix(&mut self, cols: [u8; 8], mods: u8) {
        self.m.cols = cols;
        self.m.mods = mods;
    }

    pub fn release_all(&mut self) {
        self.m.cols = [0; 8];
        self.m.mods = 0;
    }
}
