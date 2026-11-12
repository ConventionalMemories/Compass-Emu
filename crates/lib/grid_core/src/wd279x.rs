//! WD2797 floppy controller with a 5.25" double-density drive and disk, as wired in the
//! GRiD 2102: 1 MHz clock (5.25" timings: steps of 6/12/20/30 ms, 30 ms settle), MFM at
//! 250 kbit/s (32 us a byte, 6250 bytes a track at 300 rpm), READY and HLT tied high.
//!
//! Time is in microseconds (one 8051 machine cycle at 12 MHz).
//!
//! The disk is the flat image (page order: cylinder, head, sector) plus a layout per track.
//! A track nobody formatted gets the standard GRiD layout: 9 sectors of 512 bytes, written
//! with interleave 5 and side 1 shifted 7 slots (what the protocol-level model was fitted
//! to), IDs 1-9 on head 0 and 10-18 on head 1, or 1-9 on both for a PC disk. Formatting a
//! track (write track) stores the IDs and positions the firmware actually wrote; sectors that
//! fit the flat image keep their data there.

use serde::{Deserialize, Serialize};

pub const BYTE_US: u64 = 32;
pub const TRACK_BYTES: u64 = 6250;
pub const ROT_US: u64 = BYTE_US * TRACK_BYTES;
/// Index pulse width.
const INDEX_US: u64 = 4_000;
const SPT: usize = 9;
const PAGE: usize = 512;
pub const CYLS: u8 = 40;
/// The drive's head can go a little past the last formatted cylinder.
const MAX_CYL: u8 = 41;

/// One sector on a track: its ID field, where it is and where its data lives.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SecLayout {
    pub id: [u8; 4],
    /// Byte offset from the index of the ID address mark (FE).
    pub id_pos: u16,
    /// Byte offset of the first data byte (0 = no data field).
    pub data_pos: u16,
    pub deleted: bool,
    /// Data kept here instead of in the flat image (None = flat image page).
    pub data: Option<Vec<u8>>,
    /// Flat image page for this sector's data.
    pub page: Option<u32>,
}

/// Standard GRiD layout constants: 134 bytes before the first sector, 602 bytes a sector.
const FIRST_ID: u16 = 149;
const SEC_LEN: u16 = 602;
const DATA_OFF: u16 = 45;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Disk {
    /// Formatted tracks: ((cyl, head), sectors in rotation order).
    pub layouts: Vec<((u8, u8), Vec<SecLayout>)>,
    /// Side-1 IDs 1-9 (PC disk) instead of 10-18.
    pub ibm: bool,
    pub il: u32,
    pub skew: u32,
}

impl Disk {
    pub fn new(image: &[u8]) -> Disk {
        let env = |k: &str, d: u32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        // A PC boot sector (jump + 55AA) marks a PC disk.
        let ibm = image.len() >= 512 && image[510] == 0x55 && image[511] == 0xAA && matches!(image[0], 0xEB | 0xE9);
        Disk { layouts: Vec::new(), ibm, il: env("FD_IL", 5), skew: env("FD_SKEW", 7) }
    }

    fn standard(&self, image: &[u8], cyl: u8, head: u8) -> Vec<SecLayout> {
        let first = (cyl as usize * 2 + head as usize) * SPT;
        if cyl >= CYLS || (first + SPT) * PAGE > image.len() {
            return Vec::new();
        }
        let mut v = Vec::with_capacity(SPT);
        for slot in 0..SPT as u32 {
            let s = (0..SPT as u32).find(|&s| (s * self.il + head as u32 * self.skew) % SPT as u32 == slot).unwrap_or(slot);
            let r = s as u8 + 1 + if head == 1 && !self.ibm { SPT as u8 } else { 0 };
            let id_pos = FIRST_ID + SEC_LEN * slot as u16;
            v.push(SecLayout {
                id: [cyl, head, r, 2],
                id_pos,
                data_pos: id_pos + DATA_OFF,
                deleted: false,
                data: None,
                page: Some((first + s as usize) as u32),
            });
        }
        v
    }

    pub fn track(&self, image: &[u8], cyl: u8, head: u8) -> Vec<SecLayout> {
        match self.layouts.iter().find(|(k, _)| *k == (cyl, head)) {
            Some((_, l)) => l.clone(),
            None => self.standard(image, cyl, head),
        }
    }

    /// Flat image page for a sector with ID record `r` on the physical track.
    fn page_for(&self, image: &[u8], cyl: u8, head: u8, r: u8, n: u8) -> Option<u32> {
        if n != 2 || cyl >= CYLS {
            return None;
        }
        let s = match (head, r) {
            (_, 1..=9) => r - 1,
            (1, 10..=18) => r - 10,
            _ => return None,
        };
        let p = (cyl as usize * 2 + head as usize) * SPT + s as usize;
        ((p + 1) * PAGE <= image.len()).then_some(p as u32)
    }

    pub fn read_data(&self, image: &[u8], s: &SecLayout) -> Vec<u8> {
        let len = 128usize << (s.id[3] & 3);
        if let Some(d) = &s.data {
            return d.clone();
        }
        let mut v = vec![0u8; len];
        if let Some(p) = s.page {
            let o = p as usize * PAGE;
            let n = len.min(image.len().saturating_sub(o));
            v[..n].copy_from_slice(&image[o..o + n]);
        }
        v
    }

    pub fn write_data(&mut self, image: &mut [u8], cyl: u8, head: u8, idx: usize, data: &[u8], deleted: bool) -> bool {
        let mut t = self.track(image, cyl, head);
        let Some(s) = t.get_mut(idx)
        else {
            return false;
        };
        s.deleted = deleted;
        let changed_layout = deleted || s.data.is_some();
        match s.page {
            Some(p) if s.data.is_none() => {
                let o = p as usize * PAGE;
                let n = data.len().min(PAGE);
                image[o..o + n].copy_from_slice(&data[..n]);
            }
            _ => s.data = Some(data.to_vec()),
        }
        if changed_layout || self.layouts.iter().any(|(k, _)| *k == (cyl, head)) {
            self.set_layout(cyl, head, t);
        }
        true
    }

    fn set_layout(&mut self, cyl: u8, head: u8, t: Vec<SecLayout>) {
        self.layouts.retain(|(k, _)| *k != (cyl, head));
        self.layouts.push(((cyl, head), t));
    }

    /// A write track: the bytes as the controller wrote them (F5 -> A1 sync, F6 -> C2,
    /// F7 -> two CRC bytes, already expanded by the caller into (byte, is_mark) pairs).
    pub fn format_track(&mut self, image: &mut [u8], cyl: u8, head: u8, raw: &[(u8, bool)]) {
        let mut secs = Vec::new();
        let mut i = 0;
        while i < raw.len() {
            // An ID mark: A1 sync byte(s) followed by FE.
            if raw[i].1 && raw[i].0 == 0xA1 && i + 1 < raw.len() && !raw[i + 1].1 && raw[i + 1].0 == 0xFE {
                if i + 6 > raw.len() {
                    break;
                }
                let id = [raw[i + 2].0, raw[i + 3].0, raw[i + 4].0, raw[i + 5].0];
                let id_pos = (i + 1) as u16;
                let mut j = i + 6;
                let mut data_pos = 0u16;
                let mut deleted = false;
                // The data mark must follow within 43 bytes of the ID field's CRC.
                while j < raw.len() && j < i + 6 + 2 + 50 {
                    if raw[j].1 && raw[j].0 == 0xA1 && j + 1 < raw.len() && matches!(raw[j + 1].0, 0xFB | 0xF8) && !raw[j + 1].1 {
                        data_pos = (j + 2) as u16;
                        deleted = raw[j + 1].0 == 0xF8;
                        break;
                    }
                    j += 1;
                }
                let len = 128usize << (id[3] & 3);
                let mut s = SecLayout { id, id_pos, data_pos, deleted, data: None, page: None };
                if data_pos > 0 {
                    let d: Vec<u8> = (0..len).map(|k| raw.get(data_pos as usize + k).map(|x| x.0).unwrap_or(0)).collect();
                    // Map by ID record (the IDs the firmware wrote) to the flat image.
                    s.page = self.page_for(image, cyl, head, id[2], id[3]);
                    match s.page {
                        Some(p) if id[0] == cyl => {
                            let o = p as usize * PAGE;
                            image[o..o + PAGE].copy_from_slice(&d[..PAGE]);
                        }
                        _ => {
                            s.page = None;
                            s.data = Some(d);
                        }
                    }
                    i = data_pos as usize + len;
                }
                else {
                    i += 6;
                }
                secs.push(s);
                continue;
            }
            i += 1;
        }
        if secs.iter().any(|s| s.page.is_some()) && head == 1 && secs.iter().any(|s| s.id[2] >= 10) {
            self.ibm = false;
        }
        else if head == 1 && secs.iter().any(|s| (1..=9).contains(&s.id[2])) {
            self.ibm = true;
        }
        if std::env::var_os("FDCLOG").is_some() {
            let ids: Vec<String> = secs.iter().map(|s| format!("{}@{}", s.id[2], s.id_pos)).collect();
            eprintln!("FDC format c{cyl} h{head}: {}", ids.join(" "));
        }
        self.set_layout(cyl, head, secs);
    }
}

/// The drive's mechanics.
#[derive(Clone, Serialize, Deserialize)]
pub struct Mech {
    pub cyl: u8,
    /// Rotation phase: angle = (t + phase) mod ROT_US.
    pub phase: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum St {
    Idle,
    /// Type I: next step pulse at `next`.
    Step,
    /// Type I verify: settle, then look for an ID with the right track.
    Verify,
    /// Type II/III after the E delay: look for the ID field.
    Search,
    /// Read sector: data byte `i` comes at `t0 + i * 32`.
    ReadData,
    /// Write sector: the ID field passes at `next` (DRQ goes up then).
    WriteId,
    /// Write sector: waiting for the first byte.
    WriteFirst,
    WriteData,
    /// Read address: 6 ID bytes.
    ReadAddr,
    /// Write track: first byte must arrive within 3 byte times.
    WtFirst,
    /// Write track: waiting for the index pulse.
    WtIndex,
    WtData,
    /// Status is final: drop busy and raise INTRQ at `next`.
    Finish,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Wd2797 {
    pub status: u8,
    pub track: u8,
    pub sector: u8,
    pub data: u8,
    cmd: u8,
    pub intrq: bool,
    pub drq: bool,
    pub side: u8,
    st: St,
    next: u64,
    /// Type I direction: true = in (towards higher cylinders).
    dir_in: bool,
    steps: u32,
    /// Force interrupt on every index pulse (I2).
    index_int: bool,
    type1: bool,
    /// Index pulses seen since the search began, and the time of the last counted one.
    idx_seen: u32,
    last_index: u64,
    /// Sector being transferred: index in the track layout, its data, count.
    sec: usize,
    buf: Vec<u8>,
    i: usize,
    t0: u64,
    /// Write track bytes so far and the pending CRC half.
    raw: Vec<(u8, bool)>,
    /// Time of the next index edge checked for I2 interrupts.
    next_index: u64,
    /// Status read clears INTRQ unless the I3 condition is set.
    pub log: bool,
}

/// Everything around the controller: selected drive, disk image, time.
pub struct Env<'a> {
    pub now: u64,
    /// The selected drive's motor turns and a disk is in it.
    pub spinning: bool,
    pub selected: bool,
    pub mech: &'a mut Mech,
    pub disk: &'a mut Disk,
    pub image: &'a mut Vec<u8>,
    pub dirty: &'a mut bool,
    pub write_protect: bool,
    /// The READY input (tied high on the 2102; the drive's ready line on the 2101, low with
    /// no disk).
    pub ready: bool,
}

impl Env<'_> {
    fn angle(&self, t: u64) -> u64 {
        (t + self.mech.phase) % ROT_US
    }
    fn index(&self) -> bool {
        self.spinning && self.angle(self.now) < INDEX_US
    }
    /// Time of the first index pulse start strictly after t.
    fn next_index_after(&self, t: u64) -> u64 {
        let a = self.angle(t);
        t + (ROT_US - a)
    }
    /// First time >= t at which byte position `pos` of the track passes the head.
    fn at_pos(&self, t: u64, pos: u64) -> u64 {
        let a = self.angle(t);
        let p = (pos * BYTE_US) % ROT_US;
        t + (p + ROT_US - a) % ROT_US
    }
}

impl Default for Wd2797 {
    fn default() -> Self {
        Self::new()
    }
}

impl Wd2797 {
    pub fn new() -> Wd2797 {
        Wd2797 {
            status: 0,
            track: 0,
            sector: 1,
            data: 0,
            cmd: 0,
            intrq: false,
            drq: false,
            side: 0,
            st: St::Idle,
            next: u64::MAX,
            dir_in: true,
            steps: 0,
            index_int: false,
            type1: true,
            idx_seen: 0,
            last_index: 0,
            sec: 0,
            buf: Vec::new(),
            i: 0,
            t0: 0,
            raw: Vec::new(),
            next_index: u64::MAX,
            log: std::env::var_os("FDCLOG").is_some(),
        }
    }

    pub fn reset(&mut self) {
        let log = self.log;
        *self = Wd2797::new();
        self.log = log;
        self.track = 0xFF;
    }

    /// When the controller next needs to run.
    pub fn next_event(&self) -> u64 {
        self.next.min(self.next_index)
    }

    pub fn busy(&self) -> bool {
        self.status & 1 != 0
    }

    fn step_us(&self) -> u64 {
        [6_000, 12_000, 20_000, 30_000][(self.cmd & 3) as usize]
    }

    fn done(&mut self, now: u64) {
        self.st = St::Idle;
        self.next = u64::MAX;
        self.status &= !0x01;
        self.drq = false;
        self.status &= !0x02;
        self.intrq = true;
        if self.log {
            eprintln!("FDC {now} done cmd {:02X} status {:02X} trk {}", self.cmd, self.status, self.track);
        }
    }

    pub fn read(&mut self, reg: u8, e: &mut Env) -> u8 {
        self.advance(e);
        match reg & 3 {
            0 => {
                self.intrq = false;
                let mut s = self.status & 0x7F;
                if !e.ready {
                    s |= 0x80;
                }
                if self.type1 {
                    s &= !0x46;
                    if e.index() {
                        s |= 0x02;
                    }
                    if e.selected && e.mech.cyl == 0 {
                        s |= 0x04;
                    }
                    if e.write_protect && e.selected {
                        s |= 0x40;
                    }
                }
                else {
                    s = (s & !0x02) | if self.drq { 0x02 } else { 0 };
                }
                s
            }
            1 => self.track,
            2 => self.sector,
            _ => {
                self.drq = false;
                self.data
            }
        }
    }

    pub fn write(&mut self, reg: u8, v: u8, e: &mut Env) {
        self.advance(e);
        match reg & 3 {
            0 => self.command(v, e),
            1 => {
                if !self.busy() {
                    self.track = v;
                }
            }
            2 => {
                if !self.busy() {
                    self.sector = v;
                }
            }
            _ => {
                self.data = v;
                self.drq = false;
            }
        }
    }

    fn command(&mut self, v: u8, e: &mut Env) {
        let now = e.now;
        if self.log {
            eprintln!("FDC {now} cmd {v:02X} trk {} sec {} data {} cyl {}", self.track, self.sector, self.data, e.mech.cyl);
        }
        if v & 0xF0 == 0xD0 {
            // Force interrupt.
            let was_busy = self.busy();
            self.st = St::Idle;
            self.next = u64::MAX;
            self.status &= !0x01;
            self.drq = false;
            if !was_busy {
                self.type1 = true;
                self.status &= 0x01;
            }
            self.index_int = v & 0x04 != 0;
            self.next_index = if self.index_int { e.next_index_after(now) } else { u64::MAX };
            if v & 0x08 != 0 {
                self.intrq = true;
            }
            else {
                self.intrq = false;
            }
            return;
        }
        if self.busy() {
            return;
        }
        self.cmd = v;
        self.intrq = false;
        self.drq = false;
        self.index_int = false;
        self.next_index = u64::MAX;
        self.status = 0x01;
        self.idx_seen = 0;
        self.last_index = now;
        self.i = 0;
        if v & 0x80 == 0 {
            // Type I.
            self.type1 = true;
            if v & 0x08 != 0 {
                self.status |= 0x20;
            }
            match v >> 4 {
                0x0 => {
                    // Restore.
                    self.track = 0xFF;
                    self.data = 0;
                    self.dir_in = false;
                    self.steps = 0;
                }
                0x1 => {
                    self.steps = 0;
                }
                0x2 | 0x3 => {}
                0x4 | 0x5 => self.dir_in = true,
                _ => self.dir_in = false,
            }
            self.st = St::Step;
            self.next = now;
            self.type1_step(e);
            return;
        }
        self.type1 = false;
        if !e.ready {
            // Type II/III with the drive not ready: ends at once.
            self.status = 0x80;
            self.done(now);
            self.status |= 0x80;
            return;
        }
        self.side = (v >> 1) & 1;
        let delay = if v & 0x04 != 0 { 30_000 } else { 0 };
        match v >> 4 {
            0x8..=0xB | 0xC => {
                if v >> 5 == 5 && e.write_protect {
                    self.status = 0x40;
                    self.done(now);
                    return;
                }
                self.st = St::Search;
                self.next = now + delay;
                self.last_index = now + delay;
            }
            0xF => {
                if e.write_protect {
                    self.status = 0x40;
                    self.done(now);
                    return;
                }
                self.st = St::WtFirst;
                self.next = now + delay;
            }
            _ => {
                // Read track (E0): not used by the 2102; finish at once with nothing found.
                self.status = 0x10;
                self.done(now);
            }
        }
    }

    /// One type I step decision at time `self.next`.
    fn type1_step(&mut self, e: &mut Env) {
        let now = self.next;
        let kind = self.cmd >> 4;
        let stepping = match kind {
            0x0 => {
                if e.selected && e.mech.cyl == 0 {
                    self.track = 0;
                    false
                }
                else if self.steps >= 255 {
                    self.status |= 0x10;
                    self.finish_type1(e, now, false);
                    return;
                }
                else {
                    true
                }
            }
            0x1 => {
                if self.track == self.data {
                    false
                }
                else {
                    self.dir_in = self.data > self.track;
                    true
                }
            }
            _ => {
                if self.steps > 0 {
                    false
                }
                else {
                    true
                }
            }
        };
        if !stepping {
            self.finish_type1(e, now, true);
            return;
        }
        // One step pulse: update the track register (seek, restore, or step with U set).
        let update = kind <= 1 || self.cmd & 0x10 != 0;
        if update {
            self.track = if self.dir_in { self.track.wrapping_add(1) } else { self.track.wrapping_sub(1) };
        }
        if e.selected {
            if self.dir_in {
                e.mech.cyl = (e.mech.cyl + 1).min(MAX_CYL);
            }
            else {
                e.mech.cyl = e.mech.cyl.saturating_sub(1);
            }
        }
        self.steps += 1;
        self.next = now + self.step_us();
    }

    fn finish_type1(&mut self, e: &mut Env, now: u64, ok: bool) {
        if ok && self.cmd & 0x04 != 0 {
            self.st = St::Verify;
            self.next = now + 30_000;
            self.last_index = self.next;
            let _ = e;
            return;
        }
        self.done(now);
    }

    /// Run to e.now.
    pub fn advance(&mut self, e: &mut Env) {
        loop {
            let t = self.next_event();
            if t > e.now {
                return;
            }
            if self.next_index <= self.next {
                // I2: an interrupt on every index pulse while the drive turns.
                let ti = self.next_index;
                if e.spinning {
                    self.intrq = true;
                }
                self.next_index = ti + ROT_US;
                continue;
            }
            self.run_state(e);
        }
    }

    fn run_state(&mut self, e: &mut Env) {
        let now = self.next;
        match self.st {
            St::Idle => self.next = u64::MAX,
            St::Step => self.type1_step(e),
            St::Verify | St::Search => self.search(e, now),
            St::ReadData => {
                if self.i < self.buf.len() {
                    if self.drq {
                        self.status |= 0x04;
                    }
                    self.data = self.buf[self.i];
                    self.drq = true;
                    self.i += 1;
                    self.next = self.t0 + (self.i as u64 + 1) * BYTE_US;
                }
                else {
                    // The data CRC has passed.
                    self.drq = false;
                    self.done(now);
                }
            }
            St::ReadAddr => {
                if self.i < 6 {
                    if self.drq {
                        self.status |= 0x04;
                    }
                    self.data = self.buf[self.i];
                    self.drq = true;
                    self.i += 1;
                    self.next = self.t0 + (self.i as u64 + 1) * BYTE_US;
                }
                else {
                    self.sector = self.buf[0];
                    self.done(now);
                }
            }
            St::WriteId => {
                self.drq = true;
                self.st = St::WriteFirst;
                self.next = now + 22 * BYTE_US;
            }
            St::WriteFirst => {
                // 22 bytes after the ID: the first byte must be there by now.
                if self.drq {
                    self.status |= 0x04;
                    self.done(now);
                    return;
                }
                self.st = St::WriteData;
                self.buf.clear();
                self.i = 0;
                // Gap 12 x 00, 3 x A1, mark: data starts 16 bytes later.
                self.t0 = now + 16 * BYTE_US;
                self.next = self.t0;
            }
            St::WriteData => {
                let len = 128usize << (self.sec_n(e) & 3);
                if self.i < len {
                    // Byte i goes to the disk now; ask for the next one.
                    let b = if self.drq {
                        if self.log && self.status & 0x04 == 0 {
                            eprintln!("FDC {now} write lost data at byte {}", self.i);
                        }
                        self.status |= 0x04;
                        0
                    }
                    else {
                        self.data
                    };
                    self.buf.push(b);
                    self.i += 1;
                    if self.i < len {
                        self.drq = true;
                    }
                    self.next = self.t0 + self.i as u64 * BYTE_US;
                }
                else {
                    let cyl = e.mech.cyl;
                    let deleted = self.cmd & 1 != 0;
                    let data = std::mem::take(&mut self.buf);
                    if e.disk.write_data(e.image, cyl, self.side, self.sec, &data, deleted) {
                        *e.dirty = true;
                    }
                    // Then the two CRC bytes.
                    self.finish_at(now + 2 * BYTE_US);
                }
            }
            St::WtFirst => {
                // DRQ was raised when the delay ended; 3 byte times to load the first byte.
                if self.i == 0 {
                    self.drq = true;
                    self.i = 1;
                    self.next = now + 3 * BYTE_US;
                    return;
                }
                if self.drq {
                    self.status |= 0x04;
                    self.done(now);
                    return;
                }
                self.st = St::WtIndex;
                self.next = if e.spinning { e.next_index_after(now) } else { u64::MAX };
                if !e.spinning {
                    // No index pulses ever: the 2102's own timer gives up.
                    self.next = u64::MAX;
                }
            }
            St::WtIndex => {
                self.st = St::WtData;
                self.raw.clear();
                self.t0 = now;
                self.i = 0;
                self.wt_byte(now);
            }
            St::WtData => {
                if (self.raw.len() as u64) >= TRACK_BYTES {
                    let raw = std::mem::take(&mut self.raw);
                    let cyl = e.mech.cyl;
                    if e.selected {
                        e.disk.format_track(e.image, cyl, self.side, &raw);
                        *e.dirty = true;
                    }
                    self.done(now);
                    return;
                }
                self.wt_byte(now);
            }
            St::Finish => self.done(now),
        }
    }

    fn sec_n(&self, e: &Env) -> u8 {
        e.disk.track(e.image, e.mech.cyl, self.side).get(self.sec).map(|s| s.id[3]).unwrap_or(2)
    }

    /// Write track: put the data register's byte on the disk at `now`.
    fn wt_byte(&mut self, now: u64) {
        let b = if self.drq {
            self.status |= 0x04;
            0
        }
        else {
            self.data
        };
        match b {
            0xF5 => self.raw.push((0xA1, true)),
            0xF6 => self.raw.push((0xC2, true)),
            0xF7 => {
                self.raw.push((0, false));
                self.raw.push((0, false));
            }
            _ => self.raw.push((b, false)),
        }
        self.drq = true;
        self.next = self.t0 + self.raw.len() as u64 * BYTE_US;
        let _ = now;
    }

    /// Look for the next ID field from `now` (type I verify, type II/III).
    fn search(&mut self, e: &mut Env, now: u64) {
        let track = if e.spinning { e.disk.track(e.image, e.mech.cyl, self.side_for_search(e)) } else { Vec::new() };
        // The 5th index pulse after the search began ends it with record not found.
        let mut idx = if e.spinning { e.next_index_after(now.max(1) - 1) } else { u64::MAX };
        if idx < now {
            idx += ROT_US;
        }
        let mut seen = self.idx_seen;
        let read_addr = self.st == St::Search && self.cmd >> 4 == 0xC;
        if track.is_empty() {
            // No ID fields: wait for 5 index pulses (or forever with no rotation).
            if !e.spinning {
                self.next = u64::MAX;
                return;
            }
            self.status |= 0x10;
            self.finish_at(idx + (4u32.saturating_sub(seen)) as u64 * ROT_US);
            return;
        }
        let order: Vec<(u64, usize)> = {
            let mut v: Vec<(u64, usize)> = track.iter().enumerate().map(|(k, s)| (e.at_pos(now, s.id_pos as u64 + 7), k)).collect();
            v.sort();
            v
        };
        let mut rev = 0u64;
        loop {
            for &(tt, k) in &order {
                let t = tt + rev * ROT_US;
                while idx <= t {
                    seen += 1;
                    if seen >= 5 {
                        self.idx_seen = seen;
                        self.status |= 0x10;
                        self.finish_at(idx);
                        return;
                    }
                    idx += ROT_US;
                }
                let s = &track[k];
                let ok = if self.st == St::Verify {
                    s.id[0] == self.track
                }
                else if read_addr {
                    true
                }
                else {
                    s.id[0] == self.track && s.id[2] == self.sector
                };
                if ok {
                    self.idx_seen = seen;
                    self.found(e, t, k, s.clone());
                    return;
                }
            }
            rev += 1;
        }
    }

    fn side_for_search(&self, _e: &Env) -> u8 {
        if self.type1 {
            self.side
        }
        else {
            (self.cmd >> 1) & 1
        }
    }

    fn found(&mut self, e: &mut Env, t: u64, k: usize, s: SecLayout) {
        self.sec = k;
        if self.st == St::Verify {
            self.finish_at(t);
            return;
        }
        match self.cmd >> 5 {
            4 => {
                if s.data_pos == 0 {
                    // ID without data: keep looking (the WD gives RNF after 5 revolutions).
                    self.status |= 0x10;
                    self.finish_at(t + 5 * ROT_US);
                    return;
                }
                if s.deleted {
                    self.status |= 0x20;
                }
                self.buf = e.disk.read_data(e.image, &s);
                self.i = 0;
                self.st = St::ReadData;
                // Data byte i is assembled 32 us after it starts passing the head.
                self.t0 = e.at_pos(t, s.data_pos as u64);
                self.next = self.t0 + BYTE_US;
            }
            5 => {
                self.st = St::WriteId;
                self.next = t;
            }
            _ => {
                // Read address.
                self.buf = s.id.to_vec();
                self.buf.extend([0, 0]);
                self.i = 0;
                self.st = St::ReadAddr;
                self.t0 = t - 7 * BYTE_US + BYTE_US;
                self.next = self.t0 + BYTE_US;
            }
        }
    }

    fn finish_at(&mut self, t: u64) {
        self.st = St::Finish;
        self.next = t;
    }
}
