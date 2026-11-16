//! Which ROM files a model boots from, looked up in a roms folder under MAME's names.
//!
//! Boot PROM: the model's own pair (`1129even.ccprom.bin` ...) when present. The models
//! share PROMs: the 1109 uses the 1101's (rev 4), the 1121 the 1129's and the 1131 and 1137
//! the 1139's (Kurt's real 1137 has the 1139 PROM, 300442-17/300441-17). The 1139 PROM can't stand in for the 1121/1129: it
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
    /// 2101 firmware: 300205-05 (20 MB drive) and 300205-06 (10 MB). The one that fits the
    /// hard disk image runs the 2101 low-level. Set GRID_2101=protocol to use the
    /// protocol-level model anyway.
    pub hd2101: [Option<PathBuf>; 2],
    /// Internal modem 8749 firmware (16020-007): with it the modem board runs low-level.
    /// Without it (or with GRID_MODEM=off) the 8255 only drives the DTMF chip.
    pub modem: Option<PathBuf>,
}

/// The modem firmware in `dir`.
pub fn find_modem(dir: &Path) -> Option<PathBuf> {
    if std::env::var("GRID_MODEM").is_ok_and(|v| v.eq_ignore_ascii_case("off")) {
        return None;
    }
    ["16020-007.bin", "modem.bin"].iter().map(|n| dir.join(n)).find(|p| p.exists())
}

/// The 2101 firmware files in `dir`: [-05, -06].
pub fn find_2101(dir: &Path) -> [Option<PathBuf>; 2] {
    if std::env::var("GRID_2101").is_ok_and(|v| v.eq_ignore_ascii_case("protocol")) {
        return [None, None];
    }
    let f = |names: &[&str]| names.iter().map(|n| dir.join(n)).find(|p| p.exists());
    [f(&["300205-05.bin", "2101.bin"]), f(&["300205-06.bin"])]
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
        GridModel::Compass1109 => (r1101, false, None),
        GridModel::Compass1121 if have(&pair("1129")) => (pair("1129"), false, None),
        GridModel::Compass1121 | GridModel::Compass1129 => (
            r1101,
            true,
            Some(format!("no {} boot PROM in roms, using the 1101's (put 1129even/odd.ccprom.bin in roms)", model.name())),
        ),
        GridModel::Compass1131 | GridModel::Compass1137 => (pair("1139"), false, None),
        GridModel::Compass1139 => (own, false, None),
    };
    // Keyboard 8741 firmware as the boards carry it (photos): 300001-02 on the 1137 ("TEMP
    // 300001-02", dumped from Kurt's 1137), 300067-01 on the 1121 and 1139 (the early 1101's
    // 300027-01 is not dumped, so 300067-01 stands in for the 110x). Either stands in for the
    // other when missing.
    let (k_own, k_other) = (p("300001-02.bin"), p("300067-01.bin"));
    let (k_own, k_other) = if model == GridModel::Compass1137 { (k_own, k_other) } else { (k_other, k_own) };
    let keyboard = if !k_own.exists() && k_other.exists() { k_other } else { k_own };
    RomSet { prom_even: prom.0, prom_odd: prom.1, keyboard, e0000_dma, note, fd2102: find_2102(dir), hd2101: find_2101(dir), modem: find_modem(dir) }
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
    if let (Some(p), true) = (&r.modem, model.has_modem()) {
        m.board_mut().modem = Some(crate::modem::Modem::new(rd(p)?));
    }
    for (i, p) in r.hd2101.iter().enumerate() {
        if let Some(p) = p {
            m.board_mut().gpib.rom_2101[i] = Some(rd(p)?);
        }
    }
    Ok(m)
}
