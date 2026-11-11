//! A folder on the PC as a GRiD drive.
//!
//! At start the folder is turned into a GRiD (CCOS 3.x) volume image in memory, attached as a
//! floppy drive. Files in the folder become files in the volume's root, sub-folders become
//! subjects (one level, as on GRiD). When the emulator closes, files that GRiD made or changed
//! are written back to the folder and files GRiD erased are deleted there.
//!
//! The format follows the CCOS sources (Os.FsBitMap, Os.FsFar, Os.FsDirectory) as in
//! tools/gridfs.py: 512-byte pages of FID, FSN, 504 payload bytes and 4 spare; a bitmap of
//! 500 bytes a page from page 288 (where GRiD floppies keep it) with the root directory right
//! after it. Larger than a real floppy: the drive's status block reports the page count, and
//! GRiD-OS takes it from there for a volume without a partition table.
//!
//! Names: a host file whose name holds a GRiD kind (`hello.pas~Text~`) keeps it. A file without
//! one becomes `name~Text~` (lone LF line ends turned into CR LF) and is written back under its
//! own name. Characters Windows can't have in a file name are stored as %XX on the PC.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const PAGE: usize = 512;
const PAYLOAD: usize = 504;
const FAR0_SLOTS: usize = 146;
const FARN_SLOTS: usize = 246;
const FAR_OFF: usize = 0xCC;
const NULLW: u16 = 0xFFFF;
const BM_BYTES: usize = 500;
const BM_BITS: u32 = (BM_BYTES * 8) as u32;
/// Where GRiD floppies keep the bitmap; the root directory follows it.
pub const BITMAP_FID: u32 = 0x120;
const SUBJECT_NO_PASSWORD: [u8; 5] = [0x0C, 0x29, 0xFF, 0x47, 0xC7];
const ROOT_PASSWORD: [u8; 5] = [0x05, 0x29, 0xFF, 0x47, 0xC7];
/// Free space left on the volume beyond what the folder needs (4 MB).
const SPARE_PAGES: u32 = 8192;
pub const MAX_PAGES: u32 = 0xFFFF;

/// Root directory FID of a floppy-style volume of `pages` pages.
pub fn root_fid(pages: u32) -> u32 {
    BITMAP_FID + pages.div_ceil(BM_BITS)
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn put16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

/// CCOS MakeCheckSum: 16-bit sum of little-endian words (+ an odd byte).
fn checksum(b: &[u8]) -> u16 {
    let mut s: u32 = 0;
    let mut i = 0;
    while i + 1 < b.len() {
        s = s.wrapping_add(u16_at(b, i) as u32);
        i += 2;
    }
    if b.len() & 1 == 1 {
        s = s.wrapping_add(b[b.len() - 1] as u32);
    }
    s as u16
}

/// GRiD date: year, month, day, hour, minute, second, 0, weekday (Sunday 1), day of year.
pub fn grid_time(t: std::time::SystemTime) -> [u8; 11] {
    use chrono::{DateTime, Datelike, Local, Timelike};
    let d: DateTime<Local> = t.into();
    let mut o = [0u8; 11];
    put16(&mut o, 0, d.year() as u16);
    o[2] = d.month() as u8;
    o[3] = d.day() as u8;
    o[4] = d.hour() as u8;
    o[5] = d.minute() as u8;
    o[6] = d.second() as u8;
    o[8] = d.weekday().number_from_sunday() as u8;
    put16(&mut o, 9, d.ordinal() as u16);
    o
}

fn time_from_grid(b: &[u8]) -> Option<std::time::SystemTime> {
    use chrono::{Local, TimeZone};
    let y = u16_at(b, 0) as i32;
    let dt = Local.with_ymd_and_hms(y, b[2] as u32, b[3] as u32, b[4] as u32, b[5] as u32, b[6] as u32);
    dt.single().map(|d| d.into())
}

fn sort_key(name: &str) -> Vec<u8> {
    name.to_uppercase().replace('~', "\0").into_bytes()
}

#[derive(Debug)]
pub struct FsError(pub String);
impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
type R<T> = Result<T, FsError>;
fn err<T>(s: impl Into<String>) -> R<T> {
    Err(FsError(s.into()))
}

/// A floppy-style GRiD volume (no partition table) held in memory.
pub struct Volume {
    pub img: Vec<u8>,
    n: u32,
    root: u32,
}

impl Volume {
    /// Blank volume of `pages` pages named `name`.
    pub fn format(pages: u32, name: &str, now: [u8; 11]) -> Volume {
        let pages = pages.clamp(720, MAX_PAGES);
        let mut v = Volume { img: vec![0u8; pages as usize * PAGE], n: pages, root: root_fid(pages) };
        v.img[..13].copy_from_slice(b"System Disk!!");
        // Bitmap: pages past the end count as used, as on GRiD floppies.
        let nbm = pages.div_ceil(BM_BITS);
        for k in 0..nbm {
            let mut pg = vec![0u8; PAGE];
            put16(&mut pg, 0, (BITMAP_FID + k) as u16);
            put16(&mut pg, 2, k as u16);
            for r in 0..BM_BITS {
                if k * BM_BITS + r >= pages {
                    pg[8 + (r >> 3) as usize] |= 1 << (r & 7);
                }
            }
            v.put_page(BITMAP_FID + k, &pg);
            v.bm_seal(k);
        }
        let mut used = vec![0];
        used.extend((0..nbm).map(|k| BITMAP_FID + k));
        used.push(v.root);
        for p in used {
            v.mark(p, true);
        }
        v.adjust_alloc(0);
        // Root directory: header page named after the volume, one data page with the end mark.
        let data = v.alloc(1).unwrap()[0];
        let root = v.root;
        let mut hdr = vec![0u8; PAGE];
        put16(&mut hdr, 0, root as u16);
        put32(&mut hdr, 4, PAYLOAD as u32);
        let nm = name.as_bytes();
        let nm = &nm[..nm.len().min(80)];
        hdr[8] = nm.len() as u8;
        hdr[9..9 + nm.len()].copy_from_slice(nm);
        hdr[89..100].copy_from_slice(&now);
        put16(&mut hdr, 100, root as u16);
        hdr[102..113].copy_from_slice(&now);
        hdr[130] = 1;
        hdr[131..136].copy_from_slice(&ROOT_PASSWORD);
        put32(&mut hdr, 136, 1);
        v.write_fars(root, &mut hdr, &[data]).unwrap();
        v.seal(&mut hdr);
        v.put_page(root, &hdr);
        let mut d = vec![0xFF];
        d.resize(PAYLOAD, 0);
        v.write_data(root, &[data], &d);
        v
    }

    /// An existing image (as written back by the drive).
    pub fn open(img: Vec<u8>) -> R<Volume> {
        let n = (img.len() / PAGE) as u32;
        if n < 720 || &img[..13] != b"System Disk!!" {
            return err("not a GRiD floppy-style volume");
        }
        Ok(Volume { img, n, root: root_fid(n) })
    }

    pub fn pages(&self) -> u32 {
        self.n
    }

    fn page(&self, p: u32) -> &[u8] {
        &self.img[p as usize * PAGE..(p as usize + 1) * PAGE]
    }
    fn put_page(&mut self, p: u32, b: &[u8]) {
        self.img[p as usize * PAGE..(p as usize + 1) * PAGE].copy_from_slice(b);
    }

    // ---- bitmap ----
    fn bm_seal(&mut self, k: u32) {
        let p = BITMAP_FID + k;
        let mut pg = self.page(p).to_vec();
        let ck = checksum(&pg[6..8 + BM_BYTES]).wrapping_add(p as u16);
        put16(&mut pg, 4, ck);
        self.put_page(p, &pg);
    }
    fn mark(&mut self, p: u32, used: bool) {
        let k = p / BM_BITS;
        let r = (p % BM_BITS) as usize;
        let a = (BITMAP_FID + k) as usize * PAGE + 8 + (r >> 3);
        if used {
            self.img[a] |= 1 << (r & 7);
        }
        else {
            self.img[a] &= !(1 << (r & 7));
        }
        self.bm_seal(k);
    }
    fn is_used(&self, p: u32) -> bool {
        let k = p / BM_BITS;
        let r = (p % BM_BITS) as usize;
        self.img[(BITMAP_FID + k) as usize * PAGE + 8 + (r >> 3)] & (1 << (r & 7)) != 0
    }
    fn adjust_alloc(&mut self, delta: i32) {
        let a = BITMAP_FID as usize * PAGE + 6;
        let v = (u16_at(&self.img, a) as i32 + delta) as u16;
        put16(&mut self.img, a, v);
        // The allocated count is inside the checksummed range.
        self.bm_seal(0);
    }
    /// First fit, like CCOS Allocate.
    fn alloc(&mut self, count: usize) -> R<Vec<u32>> {
        let mut got = Vec::with_capacity(count);
        let mut p = 0;
        while got.len() < count && p < self.n {
            if !self.is_used(p) {
                got.push(p);
            }
            p += 1;
        }
        if got.len() < count {
            return err("the folder drive is full");
        }
        for &q in &got {
            self.mark(q, true);
        }
        self.adjust_alloc(got.len() as i32);
        Ok(got)
    }

    // ---- files ----
    fn seal(&self, hdr: &mut [u8]) {
        let ck = checksum(&hdr[0..202]);
        put16(hdr, 202, ck);
    }

    fn header(&self, fid: u32) -> R<Vec<u8>> {
        if fid >= self.n {
            return err(format!("file {fid} outside the volume"));
        }
        let h = self.page(fid).to_vec();
        if u16_at(&h, 0) as u32 != fid || u16_at(&h, 2) != 0 || h[8] == 0 || h[8] > 80 {
            return err(format!("page {fid} is not a file header"));
        }
        if u16_at(&h, 202) != checksum(&h[0..202]) {
            return err(format!("file header {fid} has a bad checksum"));
        }
        Ok(h)
    }

    fn write_fars(&mut self, fid: u32, hdr: &mut [u8], pages: &[u32]) -> R<()> {
        let mut chunks: Vec<&[u32]> = vec![&pages[..pages.len().min(FAR0_SLOTS)]];
        let mut rest = &pages[pages.len().min(FAR0_SLOTS)..];
        while !rest.is_empty() {
            let k = rest.len().min(FARN_SLOTS);
            chunks.push(&rest[..k]);
            rest = &rest[k..];
        }
        let extra = if chunks.len() > 1 { self.alloc(chunks.len() - 1)? } else { vec![] };
        let mut fars = vec![fid];
        fars.extend(&extra);
        for (k, chunk) in chunks.iter().enumerate() {
            let mut own = vec![0u8; PAGE];
            let pg: &mut [u8] = if k == 0 { hdr } else { &mut own };
            let (off, slots) = if k == 0 { (FAR_OFF, FAR0_SLOTS) } else { (0, FARN_SLOTS) };
            let nxt = fars.get(k + 1).map(|&p| p as u16).unwrap_or(NULLW);
            let prev = if k > 0 { fars[k - 1] as u16 } else { NULLW };
            put16(pg, off, fid as u16);
            put16(pg, off + 2, k as u16);
            put16(pg, off + 4, 0);
            put16(pg, off + 6, nxt);
            put16(pg, off + 8, fars[k] as u16);
            put16(pg, off + 10, prev);
            for s in 0..slots {
                put16(pg, off + 12 + 2 * s, chunk.get(s).map(|&p| p as u16).unwrap_or(NULLW));
            }
            let ck = checksum(&pg[off + 6..off + 12 + slots * 2]).wrapping_add(fid as u16).wrapping_add(k as u16);
            put16(pg, off + 4, ck);
            if k > 0 {
                self.put_page(fars[k], &own);
            }
        }
        Ok(())
    }

    fn write_data(&mut self, fid: u32, pages: &[u32], data: &[u8]) {
        for (i, &p) in pages.iter().enumerate() {
            let mut pg = vec![0u8; PAGE];
            put16(&mut pg, 0, fid as u16);
            put16(&mut pg, 2, i as u16);
            let a = (i * PAYLOAD).min(data.len());
            let b = ((i + 1) * PAYLOAD).min(data.len());
            pg[4..4 + b - a].copy_from_slice(&data[a..b]);
            self.put_page(p, &pg);
        }
    }

    /// (FAR page numbers after the header, data pages) of a file.
    fn file_pages(&self, fid: u32) -> R<(Vec<u32>, Vec<u32>)> {
        let mut fars = vec![];
        let mut data = vec![];
        let (mut p, mut k) = (fid, 0u16);
        loop {
            let pg = self.page(p);
            let (off, slots) = if k == 0 { (FAR_OFF, FAR0_SLOTS) } else { (0, FARN_SLOTS) };
            if k > 0 {
                if u16_at(pg, 0) as u32 != fid || u16_at(pg, 2) != k {
                    return err(format!("broken FAR chain in file {fid}"));
                }
                fars.push(p);
            }
            for s in 0..slots {
                let x = u16_at(pg, off + 12 + 2 * s);
                if x != 0 && x != NULLW {
                    data.push(x as u32);
                }
            }
            let nxt = u16_at(pg, off + 6);
            if nxt == NULLW || fars.len() > 300 {
                return Ok((fars, data));
            }
            p = nxt as u32;
            k += 1;
        }
    }

    pub fn read(&self, fid: u32) -> R<Vec<u8>> {
        let size = u32_at(&self.header(fid)?, 4) as usize;
        let (_, pages) = self.file_pages(fid)?;
        let mut out = Vec::with_capacity(pages.len() * PAYLOAD);
        for (i, &p) in pages.iter().enumerate() {
            if p >= self.n {
                return err(format!("file {fid} points outside the volume"));
            }
            let pg = self.page(p);
            if u16_at(pg, 0) as u32 != fid || u16_at(pg, 2) != i as u16 {
                return err(format!("data page {p} of file {fid} has a bad tag"));
            }
            out.extend_from_slice(&pg[4..4 + PAYLOAD]);
        }
        out.truncate(size);
        Ok(out)
    }

    /// Modified date of a file.
    pub fn modified(&self, fid: u32) -> Option<std::time::SystemTime> {
        self.header(fid).ok().and_then(|h| time_from_grid(&h[102..113]))
    }

    fn create(&mut self, name: &str, data: &[u8], dir: u32, subject: bool, created: [u8; 11], modified: [u8; 11]) -> R<u32> {
        let npages = if subject { 1 } else { data.len().div_ceil(PAYLOAD) };
        let fid = self.alloc(1)?[0];
        let pages = if npages > 0 { self.alloc(npages)? } else { vec![] };
        let mut hdr = vec![0u8; PAGE];
        put16(&mut hdr, 0, fid as u16);
        let nm: Vec<u8> = name.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect();
        let nm = &nm[..nm.len().min(80)];
        put32(&mut hdr, 4, if subject { PAYLOAD as u32 } else { data.len() as u32 });
        hdr[8] = nm.len() as u8;
        hdr[9..9 + nm.len()].copy_from_slice(nm);
        hdr[89..100].copy_from_slice(&created);
        put16(&mut hdr, 100, dir as u16);
        hdr[102..113].copy_from_slice(&modified);
        if subject {
            hdr[131..136].copy_from_slice(&SUBJECT_NO_PASSWORD);
        }
        else {
            hdr[131] = 5;
        }
        put32(&mut hdr, 136, 1);
        hdr[149] = 1;
        self.write_fars(fid, &mut hdr, &pages)?;
        self.seal(&mut hdr);
        self.put_page(fid, &hdr);
        if subject {
            let mut d = vec![0xFF];
            d.resize(PAYLOAD, 0);
            self.write_data(fid, &pages, &d);
        }
        else {
            self.write_data(fid, &pages, data);
        }
        Ok(fid)
    }

    /// Directory entries (FID, name).
    pub fn entries(&self, dir: u32) -> R<Vec<(u32, String)>> {
        let data = self.read(dir)?;
        let dl = u32_at(&self.header(dir)?, 136) as usize;
        let mut out = vec![];
        let mut i = 0;
        while i + 4 <= data.len() && i < dl && data[i] != 0xFF {
            let fid = u16_at(&data, i + 1) as u32;
            let ln = data[i + 3] as usize;
            if i + 4 + ln > data.len() {
                break;
            }
            out.push((fid, data[i + 4..i + 4 + ln].iter().map(|&b| b as char).collect()));
            i += 5 + ln;
        }
        Ok(out)
    }

    fn store_dir(&mut self, dir: u32, mut ents: Vec<(u32, String)>) -> R<()> {
        ents.sort_by_key(|e| sort_key(&e.1));
        let mut body = vec![];
        for (fid, nm) in &ents {
            let b: Vec<u8> = nm.chars().map(|c| c as u8).collect();
            body.push(0);
            body.extend_from_slice(&(*fid as u16).to_le_bytes());
            body.push(b.len() as u8);
            body.extend_from_slice(&b);
            body.push(b.len() as u8 + 4);
        }
        body.push(0xFF);
        let mut hdr = self.header(dir)?;
        let (_, mut pages) = self.file_pages(dir)?;
        if body.len() > pages.len() * PAYLOAD {
            let more = self.alloc((body.len() - pages.len() * PAYLOAD).div_ceil(PAYLOAD))?;
            pages.extend(more);
            if pages.len() > FAR0_SLOTS {
                return err("too many files in one directory");
            }
            self.write_fars(dir, &mut hdr, &pages)?;
        }
        put32(&mut hdr, 4, (pages.len() * PAYLOAD) as u32);
        body.resize(pages.len() * PAYLOAD, 0);
        self.write_data(dir, &pages, &body);
        let dl: usize = ents.iter().map(|e| e.1.len() + 5).sum::<usize>() + 1;
        put32(&mut hdr, 136, dl as u32);
        put16(&mut hdr, 140, ents.len() as u16);
        self.seal(&mut hdr);
        self.put_page(dir, &hdr);
        Ok(())
    }

    fn add(&mut self, dir: u32, name: &str, data: &[u8], subject: bool, created: [u8; 11], modified: [u8; 11]) -> R<u32> {
        let mut ents = self.entries(dir)?;
        if ents.iter().any(|e| e.1.eq_ignore_ascii_case(name)) {
            return err(format!("two files named {name}"));
        }
        let fid = self.create(name, data, dir, subject, created, modified)?;
        ents.push((fid, name.to_string()));
        self.store_dir(dir, ents)?;
        Ok(fid)
    }

    /// Every file: (subject name or "" for the root, file name, FID).
    pub fn files(&self) -> R<Vec<(String, String, u32)>> {
        let mut out = vec![];
        for (fid, nm) in self.entries(self.root)? {
            if is_subject(&nm) {
                for (f, n) in self.entries(fid)? {
                    out.push((nm.clone(), n, f));
                }
            }
            else {
                out.push((String::new(), nm, fid));
            }
        }
        Ok(out)
    }

    /// Subjects in the root.
    pub fn subjects(&self) -> R<Vec<String>> {
        Ok(self.entries(self.root)?.into_iter().map(|e| e.1).filter(|n| is_subject(n)).collect())
    }
}

fn is_subject(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with("~subject~")
}

fn subject_folder(name: &str) -> String {
    name[..name.len() - "~subject~".len()].to_string()
}

const BAD: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|', '%'];

/// GRiD name -> host file name.
fn host_name(grid: &str) -> String {
    let mut s = String::new();
    for c in grid.chars() {
        if BAD.contains(&c) || (c as u32) < 32 {
            s += &format!("%{:02X}", c as u32);
        }
        else {
            s.push(c);
        }
    }
    s
}

/// Host file name -> GRiD name (and whether a ~Text~ kind was added).
fn grid_name(host: &str) -> (String, bool) {
    let mut s = String::new();
    let mut it = host.chars().peekable();
    while let Some(c) = it.next() {
        if c == '%' {
            let h: String = it.clone().take(2).collect();
            if let Ok(v) = u8::from_str_radix(&h, 16) {
                s.push(v as char);
                it.next();
                it.next();
                continue;
            }
        }
        s.push(c);
    }
    if s.contains('~') {
        (s, false)
    }
    else {
        (s + "~Text~", true)
    }
}

fn to_crlf(b: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(b.len() + b.len() / 20);
    for (i, &c) in b.iter().enumerate() {
        if c == b'\n' && (i == 0 || b[i - 1] != b'\r') {
            o.push(b'\r');
        }
        o.push(c);
    }
    o
}

/// What was imported, to know what GRiD changed.
pub struct HostDir {
    pub path: PathBuf,
    /// (subject, GRiD name) -> (host path, contents as imported, added ~Text~)
    files: HashMap<(String, String), (PathBuf, Vec<u8>, bool)>,
    subjects: Vec<String>,
}

fn mtime(p: &Path) -> std::time::SystemTime {
    fs::metadata(p).and_then(|m| m.modified()).unwrap_or_else(|_| std::time::SystemTime::now())
}

fn key(subject: &str, name: &str) -> (String, String) {
    (subject.to_ascii_uppercase(), name.to_ascii_uppercase())
}

impl HostDir {
    /// Build a volume from the folder (made if missing).
    pub fn import(path: &Path) -> Result<(HostDir, Volume), String> {
        fs::create_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut plan: Vec<(String, Vec<(String, PathBuf, Vec<u8>, bool)>)> = vec![(String::new(), vec![])];
        let mut need: u32 = 300;
        let read_dir = |p: &Path| -> io::Result<Vec<PathBuf>> {
            let mut v: Vec<PathBuf> = fs::read_dir(p)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
            v.sort();
            Ok(v)
        };
        for p in read_dir(path).map_err(|e| format!("{}: {e}", path.display()))? {
            let fname = p.file_name().unwrap().to_string_lossy().to_string();
            if fname.starts_with('.') {
                continue;
            }
            if p.is_dir() {
                let (sname, _) = grid_name(&fname);
                let sname = sname.trim_end_matches("~Text~").to_string() + "~Subject~";
                let mut files = vec![];
                for q in read_dir(&p).map_err(|e| format!("{}: {e}", p.display()))? {
                    if q.is_file() {
                        let n = q.file_name().unwrap().to_string_lossy().to_string();
                        if n.starts_with('.') {
                            continue;
                        }
                        let (g, added) = grid_name(&n);
                        let mut data = fs::read(&q).map_err(|e| format!("{}: {e}", q.display()))?;
                        if added {
                            data = to_crlf(&data);
                        }
                        need += 2 + data.len().div_ceil(PAYLOAD) as u32;
                        files.push((g, q, data, added));
                    }
                }
                need += 4 + files.len() as u32 / 20;
                plan.push((sname, files));
            }
            else if p.is_file() {
                let (g, added) = grid_name(&fname);
                let mut data = fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                if added {
                    data = to_crlf(&data);
                }
                need += 2 + data.len().div_ceil(PAYLOAD) as u32;
                plan[0].1.push((g, p, data, added));
            }
        }
        if need + 300 > MAX_PAGES {
            return Err(format!("{} holds too much for one GRiD drive (32 MB at most)", path.display()));
        }
        let pages = (need + SPARE_PAGES).min(MAX_PAGES);
        let label = path
            .canonicalize()
            .ok()
            .and_then(|c| c.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "PC Folder".into());
        let now = grid_time(std::time::SystemTime::now());
        let mut v = Volume::format(pages, &label, now);
        let mut hd = HostDir { path: path.to_path_buf(), files: HashMap::new(), subjects: vec![] };
        let root = v.root;
        for (sname, files) in plan {
            let dir = if sname.is_empty() {
                root
            }
            else {
                hd.subjects.push(sname.clone());
                v.add(root, &sname, &[], true, now, now).map_err(|e| e.0)?
            };
            for (g, hp, data, added) in files {
                let t = grid_time(mtime(&hp));
                match v.add(dir, &g, &data, false, t, t) {
                    Ok(_) => {
                        hd.files.insert(key(&sname, &g), (hp, data, added));
                    }
                    Err(e) => eprintln!("folder drive: skipped {}: {e}", hp.display()),
                }
            }
        }
        Ok((hd, v))
    }

    /// Write GRiD's changes back to the folder. Returns lines describing what was done.
    pub fn export(&self, img: &[u8]) -> Result<Vec<String>, String> {
        let v = Volume::open(img.to_vec()).map_err(|e| e.0)?;
        let mut log = vec![];
        let mut seen = std::collections::HashSet::new();
        for s in v.subjects().map_err(|e| e.0)? {
            let dir = self.path.join(host_name(&subject_folder(&s)));
            if !dir.exists() {
                fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                log.push(format!("made folder {}", dir.display()));
            }
        }
        for (subject, name, fid) in v.files().map_err(|e| e.0)? {
            let k = key(&subject, &name);
            seen.insert(k.clone());
            let data = match v.read(fid) {
                Ok(d) => d,
                Err(e) => {
                    log.push(format!("could not read {name}: {e}"));
                    continue;
                }
            };
            let dir = if subject.is_empty() { self.path.clone() } else { self.path.join(host_name(&subject_folder(&subject))) };
            let target = match self.files.get(&k) {
                Some((hp, old, _)) if *old == data => {
                    let _ = hp;
                    continue;
                }
                Some((hp, _, _)) => hp.clone(),
                None => dir.join(host_name(&name)),
            };
            // The file gets the PC's time: GRiD-OS's clock has no real year.
            fs::write(&target, &data).map_err(|e| format!("{}: {e}", target.display()))?;
            log.push(format!("wrote {}", target.display()));
        }
        for (k, (hp, _, _)) in &self.files {
            if !seen.contains(k) && hp.exists() {
                fs::remove_file(hp).map_err(|e| format!("{}: {e}", hp.display()))?;
                log.push(format!("deleted {}", hp.display()));
            }
        }
        Ok(log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let base = std::env::temp_dir().join(format!("hostdir-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("Projects")).unwrap();
        fs::write(base.join("hello.pas"), b"line1\nline2\n").unwrap();
        fs::write(base.join("Projects").join("big~Run~"), vec![7u8; 200_000]).unwrap();
        let (hd, mut v) = HostDir::import(&base).unwrap();
        let files = v.files().unwrap();
        assert_eq!(files.len(), 2);
        let (_, _, fid) = files.iter().find(|f| f.1 == "hello.pas~Text~").unwrap().clone();
        assert_eq!(v.read(fid).unwrap(), b"line1\r\nline2\r\n");
        // GRiD adds a file and erases the big one.
        let root = v.root;
        let now = grid_time(std::time::SystemTime::now());
        v.add(root, "new/one~Text~", b"abc", false, now, now).unwrap();
        let subj = v.entries(root).unwrap().into_iter().find(|e| is_subject(&e.1)).unwrap().0;
        let mut ents = v.entries(subj).unwrap();
        ents.clear();
        v.store_dir(subj, ents).unwrap();
        let log = hd.export(&v.img).unwrap();
        assert!(log.iter().any(|l| l.contains("new%2Fone~Text~")), "{log:?}");
        assert!(!base.join("Projects").join("big~Run~").exists());
        assert_eq!(fs::read(base.join("new%2Fone~Text~")).unwrap(), b"abc");
        // hello.pas unchanged: not rewritten.
        assert_eq!(fs::read(base.join("hello.pas")).unwrap(), b"line1\nline2\n");
        let _ = fs::remove_dir_all(&base);
    }
}
