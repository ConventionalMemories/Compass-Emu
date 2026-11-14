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
    pub readonly: bool,
    /// Fast CPU core (whole instructions, approximate timing).
    pub fast: bool,
    /// Program symbols: the project folder of a debug build (NAME.pas.obj files).
    pub symbols: String,
    /// Crash catcher off (it is on unless this is set).
    pub no_catch: bool,
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

    /// A one-line description for lists.
    pub fn summary(&self) -> String {
        let mut v = vec![format!("{}", self.model().name())];
        if !self.hd.is_empty() {
            v.push("2101".into());
        }
        if !self.flop.is_empty() {
            v.push("2102".into());
        }
        if !self.folder.is_empty() {
            v.push("PC folder".into());
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
        opt("--hd", &self.hd);
        opt("--flop", &self.flop);
        opt("--flop2101", &self.flop2101);
        opt("--folder", &self.folder);
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
        if self.readonly {
            a.push("--readonly".into());
        }
        if self.fast {
            a.push("--fast".into());
        }
        let mut opt = |flag: &str, v: &str| {
            if !v.trim().is_empty() {
                a.push(flag.to_string());
                a.push(v.trim().to_string());
            }
        };
        opt("--symbols", &self.symbols);
        if self.no_catch {
            a.push("--no-catch".into());
        }
        a
    }
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
        let mut m = GridMachine::new(model, &even, &odd);
        m.board_mut().e0000_dma =
            roms.e0000_dma || (model.split_board() && !model.wide_screen() && (prom.is_some() || prom_even.is_some()));
        m.board_mut().keyboard = Some(Keyboard::new(read(&kbd, "keyboard firmware")?));
        if let Some(p) = &roms.fd2102 {
            m.board_mut().gpib.rom_2102 = Some(read(p, "2102 floppy firmware")?);
        }
        for (i, p) in roms.hd2101.iter().enumerate() {
            if let Some(p) = p {
                m.board_mut().gpib.rom_2101[i] = Some(read(p, "2101 firmware")?);
            }
        }

        let mut media = vec![];
        for (kind, path) in [(DriveKind::Hdd2101, &cfg.hd), (DriveKind::Floppy2102, &cfg.flop), (DriveKind::Floppy2101, &cfg.flop2101)] {
            if let Some(p) = some(path) {
                let img = read(&p, "disk image")?;
                m.board_mut().gpib.drives.push(Drive::new(kind, img));
                media.push(Media::Image(p));
            }
        }
        let mut hostdir = None;
        if let Some(dir) = some(&cfg.folder) {
            if some(&cfg.flop2101).is_some() {
                return Err("a PC folder and a 2101 floppy image both use the 2101 floppy drive; pick one".into());
            }
            let (hd, vol) = grid_core::hostdir::HostDir::import(&dir).map_err(|e| format!("cannot use folder: {e}"))?;
            notes.push(format!("folder {} is the Floppy Disk ({} KB free space)", dir.display(), vol.pages() / 2));
            m.board_mut().gpib.drives.push(Drive::new(DriveKind::Floppy2101, vol.img));
            media.push(Media::Folder(dir));
            hostdir = Some(hd);
        }
        let n = m.board().gpib.drives.len();
        m.board_mut().gpib.add_2101_floppy_slot();
        if m.board().gpib.drives.len() > n {
            media.push(Media::Empty);
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
        Ok(Session { m, cfg: cfg.clone(), media, hostdir, serial, gridlink, audio, notes, serial_out: vec![] })
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
        if self.cfg.readonly {
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

    /// Write every changed disk, the folder and the bubble memory back.
    pub fn write_back(&mut self) -> Vec<String> {
        let mut log = vec![];
        for i in 0..self.media.len() {
            log.extend(self.save_drive(i));
        }
        if !self.cfg.readonly {
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

    /// Put a floppy image into a floppy drive (None = take the disk out). The old disk is
    /// written back first if GRiD changed it.
    pub fn insert_floppy(&mut self, kind: DriveKind, path: Option<&Path>) -> Result<Vec<String>, String> {
        if kind == DriveKind::Hdd2101 {
            return Err("the hard disk can only be changed with the machine off".into());
        }
        let img = match path {
            Some(p) => read(p, "floppy image")?,
            None => vec![],
        };
        let i = match self.m.board().gpib.drives.iter().position(|d| d.kind == kind) {
            Some(i) => i,
            None => {
                self.m.board_mut().gpib.drives.push(Drive::new(kind, vec![]));
                self.media.push(Media::Empty);
                self.media.len() - 1
            }
        };
        let mut log = self.save_drive(i);
        if matches!(self.media[i], Media::Folder(_)) {
            self.hostdir = None;
            self.cfg.folder.clear();
        }
        let d = &mut self.m.board_mut().gpib.drives[i];
        d.image = img;
        d.dirty = false;
        let text = path.map(|p| p.display().to_string()).unwrap_or_default();
        match kind {
            DriveKind::Floppy2102 => self.cfg.flop = text,
            _ => self.cfg.flop2101 = text,
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
