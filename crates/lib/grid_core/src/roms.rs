//! Which ROM files a model boots from, looked up in a roms folder under MAME's names.
//!
//! Boot PROM: the model's own pair (`1129even.ccprom.bin` ...) when present. Otherwise the
//! nearest dump: the 1101's for the 1109, the 1129's for the 1121 (never dumped), the
//! 1139's for the 1131 (never dumped). The 1139 PROM can't stand in for the 1121/1129: it
//! keeps its data at 0440:0, where the 11xx build of CCOS loads. Without the 1129 PROM the
//! 1101 PROM stands in, with the split board also answering the 1101's GPIB DMA window at
//! E0000 (`GridBoard::e0000_dma`).
//!
//! The 1101 PROM is the later rev 4 (`1101even.rev4.bin`, Prom 0.25 3/16/83, the one on the
//! real 1101 that GRiDBench was measured on) when present, else rev 3. Rev 4 differs from
//! rev 3 in 31 bytes: its reset code first clears DFE00/DFE02/DFE04 (the serial select
//! latches on the 1101 board).

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
    /// 2102 floppy firmware (300237-02 or -01): with it the 2102 runs low-level. Set
    /// GRID_2102=protocol to use the protocol-level model anyway.
    pub fd2102: Option<PathBuf>,
}

/// The 2102 firmware in `dir`, newest first.
pub fn find_2102(dir: &Path) -> Option<PathBuf> {
    if std::env::var("GRID_2102").is_ok_and(|v| v.eq_ignore_ascii_case("protocol")) {
        return None;
    }
    ["300237-02.bin", "300237-01.bin", "2102.bin"].iter().map(|n| dir.join(n)).find(|p| p.exists())
}

pub fn find(model: GridModel, dir: &Path) -> RomSet {
    let p = |n: &str| dir.join(n);
    let pair = |m: &str| (p(&format!("{m}even.ccprom.bin")), p(&format!("{m}odd.ccprom.bin")));
    let have = |x: &(PathBuf, PathBuf)| x.0.exists() && x.1.exists();
    let rev4 = (p("1101even.rev4.bin"), p("1101odd.rev4.bin"));
    let r1101 = if have(&rev4) { rev4 } else { (p("1101even.rev3.bin"), p("1101odd.rev3.bin")) };
    let own = pair(model.name());
    let (prom, e0000_dma, note) = match model {
        _ if have(&own) => (own, false, None),
        GridModel::Compass1101 => (r1101, false, None),
        GridModel::Compass1109 => (r1101, false, Some("no 1109 boot PROM in roms, using the 1101's".to_string())),
        GridModel::Compass1121 if have(&pair("1129")) => (pair("1129"), false, Some("no 1121 boot PROM in roms, using the 1129's".to_string())),
        GridModel::Compass1121 | GridModel::Compass1129 => (
            r1101,
            true,
            Some(format!("no {} boot PROM in roms, using the 1101's (put 1129even/odd.ccprom.bin in roms)", model.name())),
        ),
        GridModel::Compass1131 => (pair("1139"), false, Some("no 1131 boot PROM in roms, using the 1139's".to_string())),
        GridModel::Compass1139 => (own, false, None),
    };
    let k2 = p("300001-02.bin");
    let keyboard = if model.split_board() && k2.exists() { k2 } else { p("300067-01.bin") };
    RomSet { prom_even: prom.0, prom_odd: prom.1, keyboard, e0000_dma, note, fd2102: find_2102(dir) }
}

/// A ready machine for `model` with the ROMs from `dir` (keyboard firmware loaded, the
/// E0000 window set for a stand-in PROM), or a message naming the missing file.
pub fn machine(model: GridModel, dir: &Path) -> Result<crate::GridMachine, String> {
    let r = find(model, dir);
    let rd = |p: &PathBuf| std::fs::read(p).map_err(|e| format!("cannot read {}: {e}", p.display()));
    let (even, odd, kbd) = (rd(&r.prom_even)?, rd(&r.prom_odd)?, rd(&r.keyboard)?);
    let mut m = crate::GridMachine::new(model, &even, &odd);
    m.board_mut().keyboard = Some(crate::keyboard::Keyboard::new(kbd));
    m.board_mut().e0000_dma = r.e0000_dma;
    if let Some(p) = &r.fd2102 {
        m.board_mut().gpib.rom_2102 = Some(rd(p)?);
    }
    Ok(m)
}
