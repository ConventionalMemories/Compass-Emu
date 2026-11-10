//! Which ROM files a model boots from, looked up in a roms folder under MAME's names.
//!
//! Boot PROM: the model's own pair (`1129even.ccprom.bin` ...) when present. Otherwise the
//! nearest dump we have: the 1101 rev 3 for the 1109, 1121 and 1129, and the 1139's for the
//! 1131. The 1139 PROM can't stand in for the 1121/1129: it keeps its data at 0440:0, where
//! the 11xx build of CCOS loads. The 1101 PROM can, if the split board also answers the
//! 1101's GPIB DMA window at E0000 (`GridBoard::e0000_dma`).

use std::path::{Path, PathBuf};

use crate::machine::GridModel;

pub struct RomSet {
    pub prom_even: PathBuf,
    pub prom_odd: PathBuf,
    pub keyboard: PathBuf,
    /// Set `GridBoard::e0000_dma` (a 1101 PROM on a split board).
    pub e0000_dma: bool,
    /// Explains a stand-in PROM, if one is used.
    pub note: Option<String>,
}

pub fn find(model: GridModel, dir: &Path) -> RomSet {
    let p = |n: &str| dir.join(n);
    let own = (p(&format!("{}even.ccprom.bin", model.name())), p(&format!("{}odd.ccprom.bin", model.name())));
    let r1101 = (p("1101even.rev3.bin"), p("1101odd.rev3.bin"));
    let r1139 = (p("1139even.ccprom.bin"), p("1139odd.ccprom.bin"));
    let (prom, e0000_dma, note) = if own.0.exists() && own.1.exists() || matches!(model, GridModel::Compass1101 | GridModel::Compass1139) {
        match model {
            GridModel::Compass1101 if !(own.0.exists() && own.1.exists()) => (r1101, false, None),
            GridModel::Compass1139 => (r1139, false, None),
            _ => (own, false, None),
        }
    }
    else if model.wide_screen() {
        (r1139, false, Some(format!("no {} boot PROM in roms, using the 1139's", model.name())))
    }
    else {
        let split = model.split_board();
        (r1101, split, Some(format!("no {} boot PROM in roms, using the 1101 rev 3", model.name())))
    };
    let k2 = p("300001-02.bin");
    let keyboard = if model.split_board() && k2.exists() { k2 } else { p("300067-01.bin") };
    RomSet { prom_even: prom.0, prom_odd: prom.1, keyboard, e0000_dma, note }
}

/// A ready machine for `model` with the ROMs from `dir` (keyboard firmware loaded, the
/// E0000 window set for a stand-in PROM), or a message naming the missing file.
pub fn machine(model: GridModel, dir: &Path) -> Result<crate::GridMachine, String> {
    let r = find(model, dir);
    let rd = |p: &PathBuf| std::fs::read(p).map_err(|e| format!("cannot read {}: {e}", p.display()));
    let (even, odd, kbd) = (rd(&r.prom_even)?, rd(&r.prom_odd)?, rd(&r.keyboard)?);
    let mut m = crate::GridMachine::new(model, &even, &odd);
    m.board_mut().keyboard = Some(crate::keyboard::Keyboard::new(kbd));
    m.board_mut().e0000_dma = model.split_board() && !model.wide_screen();
    Ok(m)
}
