//! A machine as the user set it up: the settings (one machine profile, also what the command
//! line gives) and the running machine with its drives, sound and serial link.

use std::{
    fs,
    path::{Path, PathBuf},
};

use grid_core::{
    gpib::{Drive, DriveKind},
    keyboard::Keyboard,
    screen,
    GridMachine,
    GridModel,
};
use serde::{Deserialize, Serialize};

use crate::host;

/// One machine profile. Empty strings mean "none" or "the default".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MachineConfig {
    pub name: String,
    /// 1101, 1109, 1121, 1129, 1131, 1137 or 1139.
    pub model: String,
    /// Folder with the ROMs ("" = the roms folder next to the program).
    pub rom_dir: String,
    pub prom: String,
    pub prom_even: String,
    pub prom_odd: String,
    pub kbd: String,
    /// 2101 hard disk image (GPIB 4).
    pub hd: String,
    /// 2102 floppy image (GPIB 6).
    pub flop: String,
    /// 2101 floppy image (GPIB 5).
    pub flop2101: String,
    /// PC folder as the 2101 floppy.
    pub folder: String,
    /// Application ROM sockets 0-3 (Compass II).
    pub roms: [String; 4],
    pub bubble: String,
    /// tcp:PORT, tcp:HOST:PORT, connect:[HOST:]PORT or a port name.
    pub serial: String,
    /// GRiDLink (8274 SDLC frames over TCP): tcp:PORT, tcp:HOST:PORT or connect:[HOST:]PORT.
    pub gridlink: String,
    /// GRiDLink point-to-point cable (RTS looped to CTS) instead of multidrop.
    pub gridlink_p2p: bool,
    pub mute: bool,
    /// --readonly: nothing is written back (the setup page turns it into each unit's write
    /// protect and the bubble's).
    pub readonly: bool,
    /// Never write the bubble memory back.
    pub bubble_protect: bool,
    /// Fast CPU core (whole instructions, approximate timing).
    pub fast: bool,
    /// Program symbols: the project folder of a debug build (NAME.pas.obj files).
    pub symbols: String,
    /// Crash catcher off (it is on unless this is set).
    pub no_catch: bool,
    /// Run the 8087 on its own microcode ROM (supplied by the user) instead of the
    /// instruction-level model.
    pub fpu_micro: bool,
    /// The 8087 microcode ROM file ("" = I8087_MICROCODE, else 8087mc.txt or 8087mc.bin in
    /// the ROM folder or in roms/).
    pub fpu_rom: String,
    /// 2102 floppy firmware ("" = 300237-02/-01.bin in the ROM folder).
    pub rom_2102: String,
    /// 2101 firmware for the 20 MB box ("" = 300205-05.bin in the ROM folder).
    pub rom_2101_20: String,
    /// 2101 firmware for the 10 MB box ("" = 300205-06.bin in the ROM folder).
    pub rom_2101_10: String,
    /// "protocol": the 2102 answers from the protocol model even with its firmware there.
    pub fd_level: String,
    /// "protocol": the 2101 box answers from the protocol model even with its firmware there.
    pub hd_level: String,
    /// GPIB address for `folder` (0 = 13 with a 2101 box, else 5).
    pub folder_addr: u8,
    /// The units on the GPIB bus. Empty: made from the older fields above (hd, flop,
    /// flop2101, folder, hd_level, fd_level) by `devices`.
    pub gpib: Vec<GpibDevice>,
}

/// One unit on the GPIB bus.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpibDevice {
    /// "2101" (hard disk and its floppy drive), "2102" (floppy) or "folder" (PC folder drive).
    pub kind: String,
    /// Off: left out of the machine, but kept with its settings.
    pub enabled: bool,
    /// GPIB address: a 2101's hard disk (its floppy is at the next one), the 2102's or the
    /// folder's. GRiD-OS: 4 Hard Disk, 5 Floppy Disk, 6 Portable Floppy, 12 Extra Hard Disk,
    /// 13 Extra Floppy Disk.
    pub addr: u8,
    /// "protocol": the protocol model even with the firmware there ("" = low level when it is).
    pub level: String,
    /// 2101: hard disk image.
    pub hd: String,
    /// 2101 floppy or 2102: the floppy image in the drive ("" = no disk).
    pub disk: String,
    /// PC folder drive: the folder.
    pub folder: String,
    /// Firmware for this unit ("" = the profile's, from the ROMs card).
    pub rom: String,
    /// Write protect of the 2101's hard disk (GRiD may write, nothing is saved).
    pub hd_protect: bool,
    /// Write protect of the floppy (2101 floppy, 2102: the tab, GRiD gets error 106) or the
    /// PC folder (nothing written back to it).
    pub disk_protect: bool,
}

impl Default for GpibDevice {
    fn default() -> Self {
        GpibDevice {
            kind: "2101".into(),
            enabled: true,
            addr: 4,
            level: String::new(),
            hd: String::new(),
            disk: String::new(),
            folder: String::new(),
            rom: String::new(),
            hd_protect: false,
            disk_protect: false,
        }
    }
}

/// GRiD-OS's name for the device at a GPIB address.
pub fn grid_name(addr: u8) -> String {
    match addr {
        4 => "Hard Disk".into(),
        5 => "Floppy Disk".into(),
        6 => "Portable Floppy".into(),
        12 => "Extra Hard Disk".into(),
        13 => "Extra Floppy Disk".into(),
        a => format!("GPIB {a}"),
    }
}

impl GpibDevice {
    pub fn new(kind: &str, addr: u8) -> GpibDevice {
        GpibDevice { kind: kind.into(), addr, ..Default::default() }
    }

    /// The addresses this unit answers on.
    pub fn addrs(&self) -> Vec<u8> {
        if self.kind == "2101" {
            vec![self.addr, self.addr + 1]
        }
        else {
            vec![self.addr]
        }
    }

    pub fn title(&self) -> &'static str {
        match self.kind.as_str() {
            "2101" => "2101 Disc System",
            "2102" => "2102 Floppy",
            _ => "PC folder drive",
        }
    }

    /// The --gpib option text: kind|addr=N|key=value...
    pub fn spec(&self) -> String {
        let mut v = vec![self.kind.clone(), format!("addr={}", self.addr)];
        if !self.enabled {
            v.push("off".into());
        }
        if self.level == "protocol" {
            v.push("protocol".into());
        }
        if self.hd_protect {
            v.push("protect-hd".into());
        }
        if self.disk_protect {
            v.push("protect-disk".into());
        }
        for (k, val) in [("hd", &self.hd), ("disk", &self.disk), ("folder", &self.folder), ("rom", &self.rom)] {
            if !val.trim().is_empty() {
                v.push(format!("{k}={}", val.trim()));
            }
        }
        v.join("|")
    }

    /// Read a --gpib option: 2101|addr=4|hd=disk.img|disk=floppy.img|protocol|off|rom=F
    pub fn parse(spec: &str) -> Result<GpibDevice, String> {
        let mut parts = spec.split('|');
        let kind = parts.next().unwrap_or("").trim().to_ascii_lowercase();
        let kind = match kind.as_str() {
            "2101" | "2102" | "folder" => kind,
            _ => return Err(format!("--gpib {spec}: the unit is 2101, 2102 or folder")),
        };
        let mut d = GpibDevice::new(&kind, match kind.as_str() { "2101" => 4, "2102" => 6, _ => 13 });
        for p in parts {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            match k.trim() {
                "addr" => d.addr = v.trim().parse().map_err(|_| format!("--gpib {spec}: addr is a number"))?,
                "hd" => d.hd = v.into(),
                "disk" => d.disk = v.into(),
                "folder" => d.folder = v.into(),
                "rom" => d.rom = v.into(),
                "protocol" => d.level = "protocol".into(),
                "off" => d.enabled = false,
                "protect-hd" => d.hd_protect = true,
                "protect-disk" | "protect" => d.disk_protect = true,
                "" => {}
                other => return Err(format!("--gpib {spec}: unknown part {other}")),
            }
        }
        Ok(d)
    }
}

/// The drive firmware a machine will use: (2102, [2101 20 MB, 2101 10 MB]).
pub struct DriveRoms {
    pub fd2102: Option<PathBuf>,
    pub hd2101: [Option<PathBuf>; 2],
}

impl MachineConfig {
    pub fn model(&self) -> GridModel {
        GridModel::parse(&self.model).unwrap_or(GridModel::Compass1101)
    }

    pub fn rom_dir(&self, default: &Path) -> PathBuf {
        if self.rom_dir.trim().is_empty() {
            default.to_path_buf()
        }
        else {
            PathBuf::from(self.rom_dir.trim())
        }
    }

    /// The drive firmware files this profile uses (its own paths, else the ROM folder's),
    /// none for a drive set to the protocol model.
    pub fn drive_roms(&self, default_roms: &Path) -> DriveRoms {
        let dir = self.rom_dir(default_roms);
        let found = grid_core::roms::find(self.model(), &dir);
        let pick = |own: &str, found: &Option<PathBuf>| some(own).or_else(|| found.clone());
        DriveRoms {
            fd2102: pick(&self.rom_2102, &found.fd2102),
            hd2101: [pick(&self.rom_2101_20, &found.hd2101[0]), pick(&self.rom_2101_10, &found.hd2101[1])],
        }
    }

    /// The GPIB units: the list, or one made from the older single-drive fields.
    pub fn devices(&self) -> Vec<GpibDevice> {
        if !self.gpib.is_empty() {
            return self.gpib.clone();
        }
        let mut v = vec![];
        let has_hd = !self.hd.trim().is_empty();
        if has_hd {
            let mut d = GpibDevice::new("2101", 4);
            d.hd = self.hd.clone();
            d.disk = self.flop2101.clone();
            d.level = self.hd_level.clone();
            v.push(d);
        }
        else if !self.flop2101.trim().is_empty() {
            // a 2101 floppy with no hard disk image: the box with an empty hard disk
            let mut d = GpibDevice::new("2101", 4);
            d.disk = self.flop2101.clone();
            d.level = self.hd_level.clone();
            v.push(d);
        }
        if !self.flop.trim().is_empty() {
            let mut d = GpibDevice::new("2102", 6);
            d.disk = self.flop.clone();
            d.level = self.fd_level.clone();
            v.push(d);
        }
        if !self.folder.trim().is_empty() {
            // with a 2101 box the folder is a unit of its own at 13 (Extra Floppy Disk), so the
            // box keeps its floppy drive and can run its firmware; else it is the Floppy Disk
            let addr = match self.folder_addr {
                0 if v.iter().any(|d| d.kind == "2101") => 13,
                0 => 5,
                a => a,
            };
            let mut d = GpibDevice::new("folder", addr);
            d.folder = self.folder.clone();
            v.push(d);
        }
        v
    }

    /// Turn the older single-drive fields into the GPIB list (the setup page edits the list).
    pub fn normalize(&mut self) {
        if self.gpib.is_empty() {
            self.gpib = self.devices();
        }
        if self.readonly {
            for d in self.gpib.iter_mut() {
                d.hd_protect = true;
                d.disk_protect = true;
            }
            self.bubble_protect = true;
            self.readonly = false;
        }
        self.hd.clear();
        self.flop.clear();
        self.flop2101.clear();
        self.folder.clear();
        self.hd_level.clear();
        self.fd_level.clear();
        self.folder_addr = 0;
    }

    /// The first enabled unit of a kind, made if there is none (for dropped files).
    pub fn device_mut(&mut self, kind: &str) -> &mut GpibDevice {
        self.normalize();
        let i = match self.gpib.iter().position(|d| d.kind == kind && d.enabled) {
            Some(i) => i,
            None => {
                let used: Vec<u8> = self.gpib.iter().filter(|d| d.enabled).flat_map(|d| d.addrs()).collect();
                let addr = free_addr(kind, &used);
                self.gpib.push(GpibDevice::new(kind, addr));
                self.gpib.len() - 1
            }
        };
        &mut self.gpib[i]
    }

    /// Problems with the GPIB list: two units on one address.
    pub fn gpib_problems(&self) -> Vec<String> {
        let mut seen: Vec<(u8, String)> = vec![];
        let mut out = vec![];
        for d in self.devices().iter().filter(|d| d.enabled) {
            for a in d.addrs() {
                if let Some((_, other)) = seen.iter().find(|(x, _)| *x == a) {
                    out.push(format!("{} and {} both use GPIB address {a} ({})", other, d.title(), grid_name(a)));
                }
                seen.push((a, d.title().to_string()));
            }
        }
        if self.devices().iter().filter(|d| d.enabled && d.kind == "folder").count() > 1 {
            out.push("only one PC folder drive can be on".into());
        }
        out
    }

    /// A one-line description for lists.
    pub fn summary(&self) -> String {
        let mut v = vec![format!("{}", self.model().name())];
        for d in self.devices().iter().filter(|d| d.enabled) {
            v.push(match d.kind.as_str() {
                "folder" => "PC folder".to_string(),
                k => k.to_string(),
            });
        }
        if self.roms.iter().any(|r| !r.is_empty()) {
            v.push("ROMs".into());
        }
        v.join(", ")
    }

    /// The same machine as command line arguments (for a .bat shortcut).
    pub fn args(&self) -> Vec<String> {
        let mut a = vec!["--model".to_string(), self.model().name().to_string()];
        let mut opt = |flag: &str, v: &str| {
            if !v.trim().is_empty() {
                a.push(flag.to_string());
                a.push(v.trim().to_string());
            }
        };
        opt("--prom", &self.prom);
        opt("--prom-even", &self.prom_even);
        opt("--prom-odd", &self.prom_odd);
        opt("--kbd", &self.kbd);
        for d in self.devices() {
            opt("--gpib", &d.spec());
        }
        for (i, r) in self.roms.iter().enumerate() {
            opt(&format!("--rom{i}"), r);
        }
        opt("--bubble", &self.bubble);
        opt("--serial", &self.serial);
        opt("--gridlink", &self.gridlink);
        if self.gridlink_p2p {
            a.push("--gridlink-p2p".into());
        }
        if self.mute {
            a.push("--mute".into());
        }
        if self.bubble_protect {
            a.push("--bubble-protect".into());
        }
        if self.readonly {
            a.push("--readonly".into());
        }
        if self.fast {
            a.push("--fast".into());
        }
        if self.fpu_micro && self.fpu_rom.trim().is_empty() {
            a.push("--fpu-micro".into());
        }
        let mut opt = |flag: &str, v: &str| {
            if !v.trim().is_empty() {
                a.push(flag.to_string());
                a.push(v.trim().to_string());
            }
        };
        opt("--symbols", &self.symbols);
        if self.fpu_micro {
            opt("--fpu-rom", &self.fpu_rom);
        }
        opt("--rom-2102", &self.rom_2102);
        opt("--rom-2101", &self.rom_2101_20);
        opt("--rom-2101-10", &self.rom_2101_10);
        if self.no_catch {
            a.push("--no-catch".into());
        }
        a
    }
}

/// Addresses a unit of a kind can be set to, as GRiD-OS knows them.
pub fn addr_choices(kind: &str) -> &'static [u8] {
    match kind {
        "2101" => &[4, 12],
        _ => &[5, 6, 13],
    }
}

/// The first address of a kind's choices that nothing uses.
pub fn free_addr(kind: &str, used: &[u8]) -> u8 {
    let pref: &[u8] = match kind {
        "2101" => &[4, 12],
        "2102" => &[6, 13, 5],
        _ => &[13, 5, 6],
    };
    for &a in pref {
        let need: Vec<u8> = if kind == "2101" { vec![a, a + 1] } else { vec![a] };
        if need.iter().all(|x| !used.contains(x)) {
            return a;
        }
    }
    pref[0]
}

/// What a drive holds, for the media manager.
#[derive(Clone, Debug, PartialEq)]
pub enum Media {
    Image(PathBuf),
    Folder(PathBuf),
    Empty,
}

pub struct Session {
    pub m: GridMachine,
    pub cfg: MachineConfig,
    /// What each GPIB drive (same order as `m.board().gpib.drives`) holds.
    pub media: Vec<Media>,
    /// For each GPIB drive: its unit in `cfg.devices()` and which drive of it (0, or 1 for a
    /// 2101's floppy).
    pub units: Vec<(usize, u8)>,
    pub hostdir: Option<grid_core::hostdir::HostDir>,
    pub serial: Option<host::SerialLink>,
    pub gridlink: Option<host::GridLink>,
    pub audio: Option<host::Audio>,
    /// Remarks made while starting (a stand-in PROM, a folder's size).
    pub notes: Vec<String>,
    serial_out: Vec<u8>,
}

fn read(path: &Path, what: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("cannot read {what} {}: {e}", path.display()))
}

/// With the microcode 8087 picked: Some(the ROM file to name, or None to leave
/// I8087_MICROCODE and roms/ to the 8087 itself). None: the instruction-level 8087.
pub fn fpu_microcode(cfg: &MachineConfig, rom_dir: &Path) -> Option<Option<PathBuf>> {
    if !cfg.fpu_micro {
        return None;
    }
    if let Some(p) = some(&cfg.fpu_rom) {
        return Some(Some(p));
    }
    if std::env::var_os("I8087_MICROCODE").is_some() {
        return Some(None);
    }
    Some(["8087mc.txt", "8087mc.bin"].iter().map(|n| rom_dir.join(n)).find(|p| p.exists()))
}

fn some(s: &str) -> Option<PathBuf> {
    let s = s.trim();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

impl Session {
    /// Build and power on the machine.
    pub fn build(cfg: &MachineConfig, default_roms: &Path) -> Result<Session, String> {
        let model = cfg.model();
        let mut notes = vec![];
        let roms = grid_core::roms::find(model, &cfg.rom_dir(default_roms));
        let prom = some(&cfg.prom);
        let prom_even = some(&cfg.prom_even);
        let (even, odd) = if let Some(p) = &prom {
            let d = read(p, "boot PROM")?;
            (d.iter().step_by(2).copied().collect(), d.iter().skip(1).step_by(2).copied().collect())
        }
        else {
            if prom_even.is_none() {
                if let Some(n) = &roms.note {
                    notes.push(n.clone());
                }
            }
            (
                read(prom_even.as_ref().unwrap_or(&roms.prom_even), "boot PROM (even)")?,
                read(&some(&cfg.prom_odd).unwrap_or(roms.prom_odd.clone()), "boot PROM (odd)")?,
            )
        };
        let kbd = some(&cfg.kbd).unwrap_or(roms.keyboard.clone());
        // The 8087 core is picked from the environment when the machine is made.
        // The user's own I8087_MICROCODE (from before the first machine) is put back each time.
        static USER_ROM: std::sync::OnceLock<Option<std::ffi::OsString>> = std::sync::OnceLock::new();
        let user_rom = USER_ROM.get_or_init(|| std::env::var_os("I8087_MICROCODE"));
        match user_rom {
            Some(v) => std::env::set_var("I8087_MICROCODE", v),
            None => std::env::remove_var("I8087_MICROCODE"),
        }
        match fpu_microcode(cfg, &cfg.rom_dir(default_roms)) {
            Some(rom) => {
                std::env::set_var("FPU_CORE", "micro");
                match rom {
                    Some(rom) => std::env::set_var("I8087_MICROCODE", rom),
                    None => notes.push("8087 microcode: no ROM file named or found in the ROM folder; looking in roms/ (see docs/8087-microcode.md)".into()),
                }
            }
            None => std::env::remove_var("FPU_CORE"),
        }
        let mut m = GridMachine::new(model, &even, &odd);
        m.board_mut().e0000_dma =
            roms.e0000_dma || (model.split_board() && !model.wide_screen() && (prom.is_some() || prom_even.is_some()));
        m.board_mut().keyboard = Some(Keyboard::new(read(&kbd, "keyboard firmware")?));
        let drive_roms = cfg.drive_roms(default_roms);
        if let Some(p) = &drive_roms.fd2102 {
            m.board_mut().gpib.rom_2102 = Some(read(p, "2102 floppy firmware")?);
        }
        for (i, p) in drive_roms.hd2101.iter().enumerate() {
            if let Some(p) = p {
                m.board_mut().gpib.rom_2101[i] = Some(read(p, "2101 firmware")?);
            }
        }
        if let Some(p) = cfg.gpib_problems().first() {
            return Err(p.clone());
        }

        let devices = cfg.devices();
        let mut media = vec![];
        let mut units = vec![];
        let mut hostdir = None;
        // a folder unit at a 2101's floppy address takes the place of the box's floppy drive
        let folder_at: Vec<u8> = devices.iter().filter(|d| d.enabled && d.kind == "folder").map(|d| d.addr).collect();
        let floppy = |path: &str, notes: &mut Vec<String>| -> Result<(Vec<u8>, Media), String> {
            match some(path) {
                Some(p) => {
                    let (p, note) = crate::imd::floppy_path(&p)?;
                    notes.extend(note);
                    Ok((read(&p, "disk image")?, Media::Image(p)))
                }
                None => Ok((vec![], Media::Empty)),
            }
        };
        for (di, dev) in devices.iter().enumerate() {
            if !dev.enabled {
                continue;
            }
            let protocol = dev.level == "protocol";
            let rom = match some(&dev.rom) {
                Some(p) => Some(read(&p, "unit firmware")?),
                None => None,
            };
            match dev.kind.as_str() {
                "2101" => {
                    let (img, med) = match some(&dev.hd) {
                        Some(p) => (read(&p, "disk image")?, Media::Image(p)),
                        None => (vec![], Media::Empty),
                    };
                    let mut d = Drive::at(DriveKind::Hdd2101, dev.addr, img);
                    d.protocol = protocol;
                    d.rom = rom;
                    d.write_protect = dev.hd_protect || cfg.readonly;
                    m.board_mut().gpib.drives.push(d);
                    media.push(med);
                    units.push((di, 0u8));
                    if !folder_at.contains(&(dev.addr + 1)) {
                        let (img, med) = floppy(&dev.disk, &mut notes)?;
                        let mut d = Drive::at(DriveKind::Floppy2101, dev.addr + 1, img);
                        d.protocol = protocol;
                        d.write_protect = dev.disk_protect || cfg.readonly;
                        m.board_mut().gpib.drives.push(d);
                        media.push(med);
                        units.push((di, 1u8));
                    }
                }
                "2102" => {
                    let (img, med) = floppy(&dev.disk, &mut notes)?;
                    let mut d = Drive::at(DriveKind::Floppy2102, dev.addr, img);
                    d.protocol = protocol;
                    d.rom = rom;
                    d.write_protect = dev.disk_protect || cfg.readonly;
                    m.board_mut().gpib.drives.push(d);
                    media.push(med);
                    units.push((di, 0u8));
                }
                _ => {
                    let Some(dir) = some(&dev.folder) else { continue };
                    let (hd, vol) = grid_core::hostdir::HostDir::import(&dir).map_err(|e| format!("cannot use folder: {e}"))?;
                    notes.push(format!(
                        "folder {} is the {} (GPIB {}, {} KB free space)",
                        dir.display(),
                        grid_name(dev.addr),
                        dev.addr,
                        vol.pages() / 2
                    ));
                    // (a protected folder takes GRiD's writes in memory and is never written back)
                    let mut d = Drive::at(DriveKind::Floppy2101, dev.addr, vol.img);
                    d.write_protect = false;
                    m.board_mut().gpib.drives.push(d);
                    media.push(Media::Folder(dir));
                    units.push((di, 0u8));
                    hostdir = Some(hd);
                }
            }
        }
        for (slot, r) in cfg.roms.iter().enumerate() {
            if let Some(p) = some(r) {
                let chip = read(&p, "ROM chip")?;
                m.board_mut().insert_rom(slot, chip).map_err(|e| format!("{}: {e}", p.display()))?;
            }
        }
        if let Some(p) = some(&cfg.bubble) {
            let img = fs::read(&p).unwrap_or_else(|_| vec![0u8; 3 * grid_core::bubble::MBM_SIZE]);
            m.board_mut().bubble = Some(grid_core::bubble::Bubble::new(img));
        }
        let serial = match some(&cfg.serial) {
            Some(sp) => {
                let sp = sp.to_string_lossy().to_string();
                Some(host::SerialLink::open(&sp).map_err(|e| format!("cannot open serial {sp}: {e}"))?)
            }
            None => None,
        };
        let gridlink = match some(&cfg.gridlink) {
            Some(sp) => {
                let sp = sp.to_string_lossy().to_string();
                Some(host::GridLink::open(&sp).map_err(|e| format!("cannot open GRiDLink {sp}: {e}"))?)
            }
            None => None,
        };
        if gridlink.is_some() {
            let s = &mut m.board_mut().serial;
            s.cts_loop = cfg.gridlink_p2p;
            s.dcd_auto = !cfg.gridlink_p2p;
        }
        let audio = if cfg.mute { None } else { host::Audio::open() };
        match &audio {
            Some(a) => m.board_mut().sound = grid_core::sound::Sound::new(screen::CPU_HZ, a.rate),
            None => m.board_mut().sound.enabled = false,
        }
        if cfg.fast {
            m.set_fast(true);
        }
        m.board_mut().debug.catch.on = !cfg.no_catch;
        if let Some(p) = some(&cfg.symbols) {
            match m.load_symbols(&p) {
                Ok(r) => notes.push(r.trim_end().to_string()),
                Err(e) => notes.push(format!("symbols: {e}")),
            }
        }
        let mut cfg = cfg.clone();
        cfg.normalize();
        Ok(Session { m, cfg, media, units, hostdir, serial, gridlink, audio, notes, serial_out: vec![] })
    }

    /// Move sound samples to the speaker and serial bytes to and from the link.
    pub fn pump_io(&mut self) {
        let b = self.m.board_mut();
        if let Some(a) = &self.audio {
            a.push(b.sound.out.drain(..));
        }
        else {
            b.sound.out.clear();
        }
        if let Some(link) = &mut self.serial {
            self.serial_out.clear();
            self.serial_out.extend(b.serial.tx_out.drain(..).map(|(_, c)| c));
            let rate = b.serial.bit_rate();
            link.pump(&self.serial_out, &mut b.serial.rx_in, rate);
            let up = link.connected();
            b.serial.set_modem_lines(up, up);
        }
        else {
            b.serial.tx_out.clear();
        }
        if let Some(gl) = &mut self.gridlink {
            let frames: Vec<Vec<u8>> = b.serial.sdlc_out.drain(..).map(|(_, f)| f).collect();
            gl.pump(frames.into_iter(), &mut b.serial.sdlc_in);
            let up = gl.connected();
            // multidrop: CTS on (not looped) tells the driver it is on a multidrop line; DCD
            // follows the line (dcd_auto). Point-to-point: CTS comes from the Compass's RTS.
            b.serial.set_modem_lines(up, up && !self.cfg.gridlink_p2p);
        }
        else {
            b.serial.sdlc_out.clear();
        }
    }

    /// Write drive `i` back if GRiD changed it. Returns what was done.
    pub fn save_drive(&mut self, i: usize) -> Vec<String> {
        let mut log = vec![];
        if self.cfg.readonly || self.protected(i) {
            return log;
        }
        let Some(d) = self.m.board().gpib.drives.get(i) else { return log };
        if !d.dirty {
            return log;
        }
        match &self.media[i] {
            Media::Image(p) => match fs::write(p, &d.image) {
                Ok(_) => log.push(format!("saved {}", p.display())),
                Err(e) => log.push(format!("could not save {}: {e}", p.display())),
            },
            Media::Folder(_) => {
                if let Some(hd) = &self.hostdir {
                    match hd.export(&d.image) {
                        Ok(l) => log.extend(l.into_iter().map(|l| format!("folder: {l}"))),
                        Err(e) => log.push(format!("could not write the folder back: {e}")),
                    }
                }
            }
            Media::Empty => {}
        }
        if let Some(d) = self.m.board_mut().gpib.drives.get_mut(i) {
            d.dirty = false;
        }
        log
    }

    /// Write protect of drive `i`, as its unit says.
    pub fn protected(&self, i: usize) -> bool {
        match self.units.get(i).and_then(|&(u, slot)| self.cfg.gpib.get(u).map(|d| (d, slot))) {
            Some((d, slot)) => if d.kind == "2101" && slot == 0 { d.hd_protect } else { d.disk_protect },
            None => self.m.board().gpib.drives.get(i).is_some_and(|d| d.write_protect),
        }
    }

    /// Turn drive `i`'s write protect on or off while the machine runs.
    pub fn set_protected(&mut self, i: usize, on: bool) {
        if let Some(&(u, slot)) = self.units.get(i) {
            if let Some(d) = self.cfg.gpib.get_mut(u) {
                if d.kind == "2101" && slot == 0 {
                    d.hd_protect = on;
                }
                else {
                    d.disk_protect = on;
                }
            }
        }
        let folder = matches!(self.media.get(i), Some(Media::Folder(_)));
        if let Some(d) = self.m.board_mut().gpib.drives.get_mut(i) {
            d.write_protect = on && !folder;
        }
    }

    /// Bring files added or changed in the PC folder into the folder drive while it runs.
    /// Call it only when GRiD has not used the drive for a while.
    pub fn sync_folder(&mut self) -> Vec<String> {
        let Some(hd) = &mut self.hostdir else { return vec![] };
        if !hd.changed_on_pc() {
            return vec![];
        }
        let Some(i) = self.media.iter().position(|m| matches!(m, Media::Folder(_))) else { return vec![] };
        let Some(d) = self.m.board_mut().gpib.drives.get_mut(i) else { return vec![] };
        hd.sync_in(&mut d.image).into_iter().map(|l| format!("PC folder: {l}")).collect()
    }

    /// Write every changed disk, the folder and the bubble memory back.
    pub fn write_back(&mut self) -> Vec<String> {
        let mut log = vec![];
        for i in 0..self.media.len() {
            log.extend(self.save_drive(i));
        }
        if !self.cfg.readonly && !self.cfg.bubble_protect {
            if let (Some(p), Some(bub)) = (some(&self.cfg.bubble), self.m.board_mut().bubble.as_mut()) {
                if bub.dirty {
                    match fs::write(&p, &bub.image) {
                        Ok(_) => {
                            log.push(format!("saved {}", p.display()));
                            bub.dirty = false;
                        }
                        Err(e) => log.push(format!("could not save {}: {e}", p.display())),
                    }
                }
            }
        }
        log
    }

    /// The floppy drive a disk of `kind` goes into: the first such drive that is not the PC
    /// folder.
    pub fn floppy_drive(&self, kind: DriveKind) -> Option<usize> {
        let d = &self.m.board().gpib.drives;
        (0..d.len()).find(|&i| d[i].kind == kind && !matches!(self.media.get(i), Some(Media::Folder(_))))
    }

    /// Put a floppy image into a floppy drive of `kind` (None = take the disk out).
    pub fn insert_floppy(&mut self, kind: DriveKind, path: Option<&Path>) -> Result<Vec<String>, String> {
        if kind == DriveKind::Hdd2101 {
            return Err("the hard disk can only be changed with the machine off".into());
        }
        match self.floppy_drive(kind) {
            Some(i) => self.insert_floppy_at(i, path),
            None => Err(format!(
                "this machine has no {} drive (add it on the setup page)",
                if kind == DriveKind::Floppy2102 { "2102 floppy" } else { "2101 floppy" }
            )),
        }
    }

    /// Put a floppy image into GPIB drive `i` (None = take the disk out). The old disk is
    /// written back first if GRiD changed it.
    pub fn insert_floppy_at(&mut self, i: usize, path: Option<&Path>) -> Result<Vec<String>, String> {
        let Some(d) = self.m.board().gpib.drives.get(i) else { return Err("no such drive".into()) };
        if d.kind == DriveKind::Hdd2101 {
            return Err("the hard disk can only be changed with the machine off".into());
        }
        if matches!(self.media[i], Media::Folder(_)) {
            return Err("that drive is the PC folder; change it on the setup page".into());
        }
        let mut log = vec![];
        let path = match path {
            Some(p) => {
                let (p, note) = crate::imd::floppy_path(p)?;
                log.extend(note);
                Some(p)
            }
            None => None,
        };
        let path = path.as_deref();
        let img = match path {
            Some(p) => read(p, "floppy image")?,
            None => vec![],
        };
        log.extend(self.save_drive(i));
        let d = &mut self.m.board_mut().gpib.drives[i];
        d.image = img;
        d.dirty = false;
        let text = path.map(|p| p.display().to_string()).unwrap_or_default();
        if let Some(&(u, _)) = self.units.get(i) {
            if let Some(dev) = self.cfg.gpib.get_mut(u) {
                dev.disk = text;
            }
        }
        self.media[i] = match path {
            Some(p) => {
                log.push(format!("inserted {}", p.display()));
                Media::Image(p.to_path_buf())
            }
            None => {
                log.push("disk taken out".into());
                Media::Empty
            }
        };
        Ok(log)
    }

    /// Restart with the same settings (disks written back first).
    pub fn restart(&mut self, default_roms: &Path) -> Result<Vec<String>, String> {
        let log = self.write_back();
        let cfg = self.cfg.clone();
        // Free the serial port and audio device before opening them again.
        self.serial = None;
        self.gridlink = None;
        self.audio = None;
        *self = Session::build(&cfg, default_roms)?;
        Ok(log)
    }

    /// Quick state: the whole machine plus copies of its disks, kept in memory.
    pub fn snapshot(&mut self) -> Snapshot {
        Snapshot {
            state: self.m.save_state(),
            disks: self.m.board().gpib.drives.iter().map(|d| d.image.clone()).collect(),
            fast: self.m.is_fast(),
        }
    }

    /// Switch the 8087 between its microcode ROM and the instruction-level core while the
    /// machine runs.
    pub fn set_fpu_micro(&mut self, on: bool, default_roms: &Path) -> Result<String, String> {
        if !on {
            self.m.set_fpu_microcode(None);
            self.cfg.fpu_micro = false;
            return Ok("8087: instruction-level core".into());
        }
        let probe = MachineConfig { fpu_micro: true, ..self.cfg.clone() };
        let rom = match fpu_microcode(&probe, &self.cfg.rom_dir(default_roms)) {
            Some(Some(p)) => i8087::micro::MicroRom::load(&p)?,
            _ => i8087::micro::MicroRom::find().ok_or("8087 microcode: no ROM file (8087mc.txt in the ROM folder, or Machine setup > ROMs)")?,
        };
        self.m.set_fpu_microcode(Some(rom));
        self.cfg.fpu_micro = true;
        Ok("8087: running on the microcode ROM".into())
    }

    /// Go back to a quick state taken from this session.
    pub fn restore(&mut self, s: &Snapshot) -> Result<(), String> {
        self.restore_with(&s.state, Some(&s.disks), s.fast)
    }

    /// Load a state saved to a file. The disks stay as they are now (a state holds no disks).
    pub fn load_state_file(&mut self, data: &[u8]) -> Result<(), String> {
        let fast = self.m.is_fast();
        self.restore_with(data, None, fast)
    }

    fn restore_with(&mut self, state: &[u8], disks: Option<&Vec<Vec<u8>>>, fast: bool) -> Result<(), String> {
        let mut m = GridMachine::load_state(state)?;
        if m.model != self.m.model {
            return Err(format!("the state is of a {} (this machine is a {})", m.model.name(), self.m.model.name()));
        }
        let old = self.m.board();
        let now: Vec<Vec<u8>> = old.gpib.drives.iter().map(|d| d.image.clone()).collect();
        let disks = disks.unwrap_or(&now);
        if m.board().gpib.drives.len() != disks.len()
            || m.board().gpib.drives.iter().zip(old.gpib.drives.iter()).any(|(a, b)| a.kind != b.kind)
        {
            return Err("the state was taken with other drives attached".into());
        }
        for (d, (img, o)) in m.board_mut().gpib.drives.iter_mut().zip(disks.iter().zip(old.gpib.drives.iter())) {
            d.dirty = o.dirty || *img != o.image;
            d.image = img.clone();
        }
        let roms = old.rom_slots.clone();
        let bubble = old.bubble.as_ref().map(|b| b.image.clone());
        for (i, chip) in roms.into_iter().enumerate() {
            if !chip.is_empty() {
                let _ = m.board_mut().insert_rom(i, chip);
            }
        }
        if let (Some(b), Some(img)) = (m.board_mut().bubble.as_mut(), bubble) {
            b.image = img;
        }
        match &self.audio {
            Some(a) => m.board_mut().sound = grid_core::sound::Sound::new(screen::CPU_HZ, a.rate),
            None => m.board_mut().sound.enabled = false,
        }
        if fast {
            m.set_fast(true);
        }
        // the 8087 core stays as it is now (a state does not say which one ran)
        if let (Some(old), Some(new)) = (self.m.fpu_mut().map(|f| f.take_microcode()), m.fpu_mut()) {
            new.put_microcode(old);
        }
        // breakpoints, symbols and the crash catcher stay as they were
        m.board_mut().debug = std::mem::take(&mut self.m.board_mut().debug);
        self.m = m;
        Ok(())
    }
}

pub struct Snapshot {
    state: Vec<u8>,
    disks: Vec<Vec<u8>>,
    fast: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpib_spec_round_trip() {
        let d = GpibDevice::parse("2101|addr=12|hd=a b.img|disk=f.IMD|protocol|off").unwrap();
        assert_eq!((d.kind.as_str(), d.addr, d.enabled, d.level.as_str()), ("2101", 12, false, "protocol"));
        assert_eq!((d.hd.as_str(), d.disk.as_str()), ("a b.img", "f.IMD"));
        assert_eq!(d.addrs(), vec![12, 13]);
        let again = GpibDevice::parse(&d.spec()).unwrap();
        assert_eq!(again.spec(), d.spec());
        assert_eq!(GpibDevice::parse("2102").unwrap().addr, 6);
        assert_eq!(GpibDevice::parse("folder|folder=x").unwrap().addr, 13);
        let p = GpibDevice::parse("2101|protect-hd|protect-disk").unwrap();
        assert!(p.hd_protect && p.disk_protect);
        assert_eq!(GpibDevice::parse(&p.spec()).unwrap().spec(), p.spec());
        // --readonly turns into every unit's write protect
        let mut c = MachineConfig { hd: "a.img".into(), flop: "b.img".into(), readonly: true, ..Default::default() };
        c.normalize();
        assert!(!c.readonly && c.bubble_protect && c.gpib.iter().all(|d| d.hd_protect && d.disk_protect));
        assert!(GpibDevice::parse("2103").is_err());
        assert!(GpibDevice::parse("2101|addr=x").is_err());
    }

    #[test]
    fn old_profiles_become_units() {
        let c = MachineConfig { hd: "hd.img".into(), flop: "f.img".into(), folder: "dir".into(), fd_level: "protocol".into(), ..Default::default() };
        let d = c.devices();
        assert_eq!(d.len(), 3);
        assert_eq!((d[0].kind.as_str(), d[0].addr, d[0].hd.as_str()), ("2101", 4, "hd.img"));
        assert_eq!((d[1].kind.as_str(), d[1].addr, d[1].level.as_str()), ("2102", 6, "protocol"));
        // next to a 2101 the folder is its own unit at 13, so the box keeps its floppy drive
        assert_eq!((d[2].kind.as_str(), d[2].addr), ("folder", 13));
        assert!(c.gpib_problems().is_empty());
        let mut n = c.clone();
        n.normalize();
        assert!(n.hd.is_empty() && n.flop.is_empty() && n.folder.is_empty());
        assert_eq!(n.devices().len(), 3);
        // without a 2101 the folder is the Floppy Disk (5)
        let c = MachineConfig { folder: "dir".into(), ..Default::default() };
        assert_eq!(c.devices()[0].addr, 5);
    }

    #[test]
    fn two_boxes_and_clashes() {
        let mut c = MachineConfig::default();
        c.gpib = vec![GpibDevice::new("2101", 4), GpibDevice::new("2101", 12)];
        assert!(c.gpib_problems().is_empty());
        c.gpib.push(GpibDevice::new("folder", 13));
        assert_eq!(c.gpib_problems().len(), 1);
        c.gpib[2].enabled = false;
        assert!(c.gpib_problems().is_empty());
        let used: Vec<u8> = c.gpib.iter().filter(|d| d.enabled).flat_map(|d| d.addrs()).collect();
        assert_eq!(free_addr("2102", &used), 6);
        assert_eq!(grid_name(12), "Extra Hard Disk");
    }
}
