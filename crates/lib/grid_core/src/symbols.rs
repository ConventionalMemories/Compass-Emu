//! Program symbols for the debugger, read from the Intel OMF-86 object files a debug build
//! leaves next to the program (gridbuild saves NAME.pas.obj and NAME.asm.obj): procedure
//! names and lengths (BLKDEF), public names (PUBDEF), assembler labels (DEBSYM) and
//! statement numbers (LINNUM), with the source text from the listings (.lst, .asm.lst).
//!
//! The bound .run file keeps none of this, and GRiD-OS loads a program wherever it finds
//! room, so each module's code is found in memory by its bytes (the bytes that fixups change
//! are left out of the comparison, and a WAIT may have been turned into INT 3 by the loader).

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

/// One name in a module: offset from the start of the module's code, and its length
/// (0 for a label).
#[derive(Clone, Debug)]
pub struct Sym {
    pub name: String,
    pub off: u32,
    pub len: u32,
}

/// Fixup location in the code that refers to an external name: what to read there to know
/// where the name is once the program is loaded.
#[derive(Clone, Debug)]
struct ExtRef {
    off: u32,
    /// true: 16-bit self-relative (CALL/JMP near); false: far pointer (CALL FAR)
    rel: bool,
    name: String,
}

/// Comparison mask values for the module's code bytes.
const UNKNOWN: u8 = 0;
const EXACT: u8 = 1;
/// A WAIT (9Bh): the GRiD-OS loader may have changed it into INT 3 (CCh).
const WAIT: u8 = 2;

#[derive(Clone, Debug, Default)]
pub struct Module {
    pub name: String,
    pub file: PathBuf,
    code: Vec<u8>,
    mask: Vec<u8>,
    /// Code segment alignment in bytes.
    align: u32,
    /// Procedures (with lengths) and labels, sorted by offset.
    pub syms: Vec<Sym>,
    /// (code offset, statement or line number), sorted by offset.
    pub lines: Vec<(u32, u32)>,
    /// Statement or line number -> "line  source text" from the listing.
    pub source: HashMap<u32, String>,
    ext: Vec<ExtRef>,
    /// Linear address of the module's code once found in memory.
    pub base: Option<u32>,
}

impl Module {
    /// "MODULE 27: text" for a statement or line number. A Pascal statement that shares a
    /// listing line with the one before it has no row of its own: that line is shown.
    pub fn source_text(&self, n: u32) -> String {
        let t = (0..8).filter_map(|k| n.checked_sub(k)).find_map(|k| self.source.get(&k));
        match t {
            Some(t) => format!("{} {t}", self.name),
            None => format!("{} line {n}", self.name),
        }
    }

    pub fn code_len(&self) -> u32 {
        self.code.len() as u32
    }

    /// Do the module's bytes match memory at `at`?
    fn matches(&self, mem: &[u8], at: usize) -> bool {
        if at + self.code.len() > mem.len() {
            return false;
        }
        self.code.iter().zip(self.mask.iter()).zip(&mem[at..at + self.code.len()]).all(|((&c, &k), &m)| match k {
            EXACT => c == m,
            WAIT => m == 0x9B || m == 0xCC,
            _ => true,
        })
    }

    /// The longest run of exactly known bytes (offset, length), to search memory with.
    fn anchor(&self) -> Option<(usize, usize)> {
        let mut best = (0, 0);
        let mut i = 0;
        while i < self.mask.len() {
            if self.mask[i] == EXACT {
                let s = i;
                while i < self.mask.len() && self.mask[i] == EXACT {
                    i += 1;
                }
                if i - s > best.1 {
                    best = (s, i - s);
                }
            }
            else {
                i += 1;
            }
        }
        (best.1 >= 6).then_some((best.0, best.1.min(32)))
    }
}

/// OMF index field (1 or 2 bytes).
fn index(b: &[u8], j: &mut usize) -> usize {
    let v = b.get(*j).copied().unwrap_or(0) as usize;
    *j += 1;
    if v & 0x80 != 0 {
        let lo = b.get(*j).copied().unwrap_or(0) as usize;
        *j += 1;
        ((v & 0x7F) << 8) | lo
    }
    else {
        v
    }
}

fn word(b: &[u8], j: &mut usize) -> u32 {
    let v = b.get(*j).copied().unwrap_or(0) as u32 | (b.get(*j + 1).copied().unwrap_or(0) as u32) << 8;
    *j += 2;
    v
}

fn name(b: &[u8], j: &mut usize) -> String {
    let l = b.get(*j).copied().unwrap_or(0) as usize;
    let s = b.get(*j + 1..(*j + 1 + l).min(b.len())).unwrap_or(&[]);
    *j += 1 + l;
    String::from_utf8_lossy(s).into_owned()
}

/// Read the modules of an OMF-86 object file (or library).
pub fn parse_obj(data: &[u8], file: &Path) -> Vec<Module> {
    let mut out = Vec::new();
    let mut m: Option<Module> = None;
    let mut lnames: Vec<String> = vec![String::new()];
    // per segment index (from 1): (name, class, length, align)
    let mut segs: Vec<(String, String, u32, u32)> = vec![Default::default()];
    let mut exts: Vec<String> = vec![String::new()];
    let mut code_seg = 0usize;
    // last LEDATA: (segment, offset) for the fixups that follow it
    let mut last_data: Option<(usize, u32)> = None;
    let mut threads_t = [(0u8, 0usize); 4];
    let mut i = 0;
    while i + 3 <= data.len() {
        let t = data[i];
        let n = data[i + 1] as usize | (data[i + 2] as usize) << 8;
        if n == 0 || i + 3 + n > data.len() {
            break;
        }
        let b = &data[i + 3..i + 3 + n - 1];
        i += 3 + n;
        match t {
            // THEADR / LHEADR
            0x80 | 0x82 => {
                let mut j = 0;
                m = Some(Module { name: name(b, &mut j), file: file.to_path_buf(), align: 1, ..Default::default() });
                lnames = vec![String::new()];
                segs = vec![Default::default()];
                exts = vec![String::new()];
                code_seg = 0;
                last_data = None;
            }
            // LNAMES
            0x96 => {
                let mut j = 0;
                while j < b.len() {
                    lnames.push(name(b, &mut j));
                }
            }
            // SEGDEF
            0x98 => {
                let a = b.first().copied().unwrap_or(0);
                let mut j = 1;
                if a >> 5 == 0 {
                    j += 3;
                }
                let len = word(b, &mut j);
                let sn = index(b, &mut j);
                let cn = index(b, &mut j);
                let align = match a >> 5 {
                    2 => 2,
                    3 => 16,
                    4 => 256,
                    _ => 1,
                };
                let sname = lnames.get(sn).cloned().unwrap_or_default();
                let cname = lnames.get(cn).cloned().unwrap_or_default();
                segs.push((sname.clone(), cname.clone(), len, align));
                if code_seg == 0 && len > 0 && (cname == "CODE" || sname == "CODE") {
                    code_seg = segs.len() - 1;
                    if let Some(m) = m.as_mut() {
                        m.code = vec![0; len as usize];
                        m.mask = vec![UNKNOWN; len as usize];
                        m.align = align;
                    }
                }
            }
            // EXTDEF
            0x8C => {
                let mut j = 0;
                while j < b.len() {
                    exts.push(name(b, &mut j));
                    index(b, &mut j);
                }
            }
            // PUBDEF
            0x90 => {
                let mut j = 0;
                let _g = index(b, &mut j);
                let s = index(b, &mut j);
                if s == 0 {
                    j += 2;
                }
                while j < b.len() {
                    let nm = name(b, &mut j);
                    let off = word(b, &mut j);
                    index(b, &mut j);
                    if s == code_seg && code_seg != 0 {
                        if let Some(m) = m.as_mut() {
                            m.syms.push(Sym { name: nm, off, len: 0 });
                        }
                    }
                }
            }
            // BLKDEF: a procedure (or the module's main block)
            0x7A => {
                let mut j = 0;
                let _g = index(b, &mut j);
                let s = index(b, &mut j);
                if s == 0 {
                    j += 2;
                }
                let nm = name(b, &mut j);
                let off = word(b, &mut j);
                let len = word(b, &mut j);
                if s == code_seg && code_seg != 0 && !nm.is_empty() {
                    if let Some(m) = m.as_mut() {
                        m.syms.push(Sym { name: nm, off, len });
                    }
                }
            }
            // DEBSYM: local symbols; the assembler's code labels are the ones in the code segment
            0x7E => {
                let mut j = 0;
                let method = b.first().copied().unwrap_or(0);
                j += 1;
                if method != 0 {
                    continue;
                }
                let g = index(b, &mut j);
                let s = index(b, &mut j);
                if g == 0 && s == 0 {
                    j += 2;
                }
                if g == 0 || s != code_seg || code_seg == 0 {
                    continue;
                }
                while j < b.len() {
                    let nm = name(b, &mut j);
                    let off = word(b, &mut j);
                    index(b, &mut j);
                    if let Some(m) = m.as_mut() {
                        m.syms.push(Sym { name: nm, off, len: 0 });
                    }
                }
            }
            // LINNUM
            0x94 => {
                let mut j = 0;
                let _g = index(b, &mut j);
                let s = index(b, &mut j);
                if s == 0 {
                    j += 2;
                }
                while j + 4 <= b.len() {
                    let line = word(b, &mut j);
                    let off = word(b, &mut j);
                    if s == code_seg && code_seg != 0 {
                        if let Some(m) = m.as_mut() {
                            m.lines.push((off, line));
                        }
                    }
                }
            }
            // LEDATA
            0xA0 => {
                let mut j = 0;
                let s = index(b, &mut j);
                let off = word(b, &mut j);
                last_data = Some((s, off));
                if s == code_seg && code_seg != 0 {
                    if let Some(m) = m.as_mut() {
                        for (k, &v) in b[j..].iter().enumerate() {
                            let a = off as usize + k;
                            if a < m.code.len() {
                                m.code[a] = v;
                                m.mask[a] = if v == 0x9B { WAIT } else { EXACT };
                            }
                        }
                    }
                }
            }
            // LIDATA: not expanded; its bytes stay unknown
            0xA2 => last_data = None,
            // FIXUPP
            0x9C => {
                let mut j = 0;
                while j < b.len() {
                    let c = b[j];
                    j += 1;
                    if c & 0x80 == 0 {
                        // THREAD
                        let d = c & 0x40 != 0;
                        let method = (c >> 2) & 7;
                        let num = (c & 3) as usize;
                        if d {
                            if method < 3 {
                                index(b, &mut j);
                            }
                        }
                        else {
                            let ix = if method & 3 == 3 { word(b, &mut j) as usize } else { index(b, &mut j) };
                            threads_t[num] = (method & 3, ix);
                        }
                        continue;
                    }
                    let lo = b.get(j).copied().unwrap_or(0);
                    j += 1;
                    let self_rel = c & 0x40 == 0;
                    let loc = (c >> 2) & 0xF;
                    let doff = ((c as u32 & 3) << 8) | lo as u32;
                    let fixdat = b.get(j).copied().unwrap_or(0);
                    j += 1;
                    let f = fixdat & 0x80 != 0;
                    let frame = (fixdat >> 4) & 7;
                    let tt = fixdat & 0x08 != 0;
                    let p = fixdat & 0x04 != 0;
                    let targt = (fixdat & 3) as usize;
                    if !f {
                        if frame < 3 {
                            index(b, &mut j);
                        }
                        else if frame == 3 {
                            j += 2;
                        }
                    }
                    let (tmethod, tindex) = if tt {
                        threads_t[targt]
                    }
                    else {
                        let meth = targt as u8;
                        let ix = if meth == 3 { word(b, &mut j) as usize } else { index(b, &mut j) };
                        (meth, ix)
                    };
                    if !p {
                        j += 2;
                    }
                    let Some((s, base)) = last_data
                    else {
                        continue;
                    };
                    if s != code_seg || code_seg == 0 {
                        continue;
                    }
                    let Some(m) = m.as_mut()
                    else {
                        continue;
                    };
                    let at = base + doff;
                    let size = match loc {
                        0 | 4 => 1,
                        1 | 2 | 5 => 2,
                        3 => 4,
                        _ => 4,
                    };
                    for k in 0..size {
                        // (8087 code has fixups on its WAITs, for an emulator library: keep
                        // knowing they were WAITs)
                        if let Some(x) = m.mask.get_mut((at + k) as usize) {
                            if *x != WAIT {
                                *x = UNKNOWN;
                            }
                        }
                    }
                    // a reference to an external name: CALL near (self-relative offset) or
                    // CALL FAR (pointer)
                    if tmethod == 2 {
                        if let Some(nm) = exts.get(tindex) {
                            if self_rel && (loc == 1 || loc == 5) {
                                m.ext.push(ExtRef { off: at, rel: true, name: nm.clone() });
                            }
                            else if loc == 3 {
                                m.ext.push(ExtRef { off: at, rel: false, name: nm.clone() });
                            }
                        }
                    }
                }
            }
            // MODEND
            0x8A => {
                if let Some(mut md) = m.take() {
                    if !md.code.is_empty() {
                        md.syms.sort_by_key(|s| (s.off, std::cmp::Reverse(s.len)));
                        md.syms.dedup_by(|a, b| a.off == b.off && a.name.eq_ignore_ascii_case(&b.name));
                        md.lines.sort();
                        out.push(md);
                    }
                }
                // libraries pad each module to a block: skip to the next record start
                while i < data.len() && data[i] == 0 {
                    i += 1;
                }
            }
            _ => {}
        }
    }
    out
}

/// Pascal-86 listing: "STMT LINE NESTING SOURCE" rows -> statement number -> "line: text".
fn read_pascal_listing(text: &str) -> HashMap<u32, String> {
    let mut map: HashMap<u32, String> = HashMap::new();
    for l in text.lines() {
        let mut w = l.split_whitespace();
        let (Some(a), Some(b), Some(c), Some(d)) = (w.next(), w.next(), w.next(), w.next())
        else {
            continue;
        };
        let (Ok(stmt), Ok(line), Ok(_), Ok(_)) = (a.parse::<u32>(), b.parse::<u32>(), c.parse::<u32>(), d.parse::<u32>())
        else {
            continue;
        };
        // the source starts after the four numbers
        let mut end = 0;
        for _ in 0..4 {
            let s = end + l[end..].len() - l[end..].trim_start().len();
            end = s + l[s..].find(char::is_whitespace).unwrap_or(l.len() - s);
        }
        let src = l[end..].trim();
        // a statement can start on a BEGIN line: show the line with the statement itself
        let filler = |t: &str| {
            let t = t.split_once(": ").map(|x| x.1).unwrap_or(t).trim().trim_end_matches(';').to_ascii_uppercase();
            matches!(t.as_str(), "" | "BEGIN" | "REPEAT" | "ELSE" | "DO" | "THEN" | "END")
        };
        match map.get(&stmt) {
            Some(old) if !filler(old.as_str()) => {}
            _ => {
                map.insert(stmt, format!("{line}: {src}"));
            }
        }
    }
    map
}

/// ASM86 listing: "LOC  OBJ  LINE  SOURCE" rows -> (code offset, line) and line -> text.
fn read_asm_listing(text: &str) -> (Vec<(u32, u32)>, HashMap<u32, String>) {
    let mut lines = Vec::new();
    let mut src = HashMap::new();
    for l in text.lines() {
        if l.len() < 28 || !l.is_char_boundary(27) {
            continue;
        }
        let loc = &l[..4];
        let Ok(off) = u32::from_str_radix(loc, 16)
        else {
            continue;
        };
        if l.as_bytes().get(4) != Some(&b' ') {
            continue;
        }
        // LINE: the number after the object bytes (column 27 on); "R" marks relocations
        let tail = &l[20.min(l.len())..];
        let mut it = tail.split_whitespace();
        let mut line = None;
        for w in it.by_ref() {
            if let Ok(n) = w.trim_start_matches('+').parse::<u32>() {
                line = Some(n);
                break;
            }
        }
        let Some(line) = line
        else {
            continue;
        };
        // the object bytes column must hold code (not a data definition of a later segment)
        let obj = l[4..20.min(l.len())].trim();
        if obj.is_empty() {
            continue;
        }
        let rest = l.splitn(2, &format!(" {line} ")).nth(1).unwrap_or("").trim();
        lines.push((off, line));
        src.entry(line).or_insert_with(|| format!("{line}: {}", rest.trim_start_matches(['+', '1', ' '])));
    }
    lines.sort();
    lines.dedup_by_key(|x| x.0);
    (lines, src)
}

/// All the symbols loaded for the debugger.
#[derive(Default)]
pub struct Symbols {
    pub modules: Vec<Module>,
    /// Where they were read from.
    pub from: Option<PathBuf>,
    /// Names of external routines (OS call stubs, Pascal library) by linear address, found
    /// from the calls in the program once it is in memory.
    ext: BTreeMap<u32, String>,
    /// Memory fingerprint of the last successful locate (to know when to look again).
    checked: Vec<(u32, u8)>,
}

impl Symbols {
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Read every object file in a folder (or the folder of a file such as the .run).
    pub fn load(path: &Path) -> Result<Symbols, String> {
        let dir = if path.is_dir() { path.to_path_buf() } else { path.parent().map(Path::to_path_buf).unwrap_or_default() };
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
                n.ends_with(".obj") || n.ends_with("~obj~")
            })
            .collect();
        files.sort();
        // a single .obj named directly
        if path.is_file() && path.extension().map(|e| e.eq_ignore_ascii_case("obj")).unwrap_or(false) {
            files = vec![path.to_path_buf()];
        }
        let mut s = Symbols { from: Some(path.to_path_buf()), ..Default::default() };
        for f in files {
            let data = std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            for mut m in parse_obj(&data, &f) {
                attach_listing(&mut m, &f);
                s.modules.push(m);
            }
        }
        if s.modules.is_empty() {
            return Err(format!(
                "no object files with code in {} (build with the debug option so NAME.pas.obj files are saved)",
                dir.display()
            ));
        }
        Ok(s)
    }

    /// Find each module in memory (`mem` = the RAM). Cheap when nothing has moved.
    pub fn locate(&mut self, mem: &[u8]) {
        if !self.checked.is_empty() && self.checked.iter().all(|&(a, v)| mem.get(a as usize) == Some(&v)) {
            return;
        }
        self.checked.clear();
        self.ext.clear();
        for i in 0..self.modules.len() {
            let m = &self.modules[i];
            // still where it was?
            if let Some(b) = m.base {
                if m.matches(mem, b as usize) {
                    continue;
                }
            }
            let mut found = None;
            if let Some((ao, al)) = m.anchor() {
                let pat = &m.code[ao..ao + al];
                let step = m.align.max(1) as usize;
                let mut at = 0usize;
                while at + m.code.len() <= mem.len() {
                    if mem[at + ao..at + ao + al] == *pat && m.matches(mem, at) {
                        found = Some(at as u32);
                        break;
                    }
                    at += step;
                }
            }
            self.modules[i].base = found;
        }
        for m in &self.modules {
            let Some(b) = m.base
            else {
                continue;
            };
            if let Some((ao, _)) = m.anchor() {
                for k in [ao, ao + 1, ao + 2, ao + 3] {
                    self.checked.push((b + k as u32, mem[b as usize + k]));
                }
            }
        }
        // The code group starts at the lowest module (the main program is linked first).
        let cg = self.modules.iter().filter_map(|m| m.base).min();
        if let Some(cg) = cg {
            let cg = cg & !0xF;
            for m in &self.modules {
                let Some(b) = m.base
                else {
                    continue;
                };
                for e in &m.ext {
                    let at = (b + e.off) as usize;
                    if at + 4 > mem.len() {
                        continue;
                    }
                    let target = if e.rel {
                        let ip = (b + e.off + 2 - cg) as u16;
                        let rel = u16::from_le_bytes([mem[at], mem[at + 1]]);
                        cg + ip.wrapping_add(rel) as u32
                    }
                    else {
                        let off = u16::from_le_bytes([mem[at], mem[at + 1]]) as u32;
                        let seg = u16::from_le_bytes([mem[at + 2], mem[at + 3]]) as u32;
                        ((seg << 4) + off) & 0xFFFFF
                    };
                    self.ext.entry(target).or_insert_with(|| e.name.clone());
                }
            }
        }
    }

    /// The module whose code holds `addr`.
    fn module_at(&self, addr: u32) -> Option<&Module> {
        self.modules.iter().find(|m| m.base.map(|b| addr >= b && addr < b + m.code_len()).unwrap_or(false))
    }

    /// Name for an address: "PROC" at its start or "PROC+12" inside it (an external routine
    /// such as an OS call stub counts too).
    pub fn name_at(&self, addr: u32) -> Option<String> {
        if let Some(m) = self.module_at(addr) {
            let off = addr - m.base.unwrap();
            // innermost procedure holding it, or the nearest label before it
            let mut best: Option<&Sym> = None;
            for s in &m.syms {
                if s.off > off {
                    break;
                }
                let inside = s.len == 0 || off < s.off + s.len;
                if inside {
                    best = match best {
                        Some(b) if b.off == s.off && b.len > 0 && s.len == 0 => Some(b),
                        _ => Some(s),
                    };
                }
            }
            return Some(match best {
                Some(s) if s.off == off => s.name.clone(),
                Some(s) => format!("{}+{:X}", s.name, off - s.off),
                None => format!("{}+{:X}", m.name, off),
            });
        }
        if let Some((&a, n)) = self.ext.range(..=addr).next_back() {
            if addr == a {
                return Some(n.clone());
            }
            // OS call stubs are 5 bytes (MOV AL,n; JMP)
            if addr - a < 6 {
                return Some(format!("{n}+{:X}", addr - a));
            }
        }
        None
    }

    /// Was the byte at `addr` a WAIT in the object file (which the GRiD-OS loader turns into
    /// INT 3)? None when the address isn't in the program.
    pub fn was_wait(&self, addr: u32) -> Option<bool> {
        let m = self.module_at(addr)?;
        let off = (addr - m.base.unwrap()) as usize;
        Some(m.mask.get(off) == Some(&WAIT))
    }

    /// The name that starts exactly at `addr` (for labels in a disassembly).
    pub fn label_at(&self, addr: u32) -> Option<String> {
        if let Some(m) = self.module_at(addr) {
            let off = addr - m.base.unwrap();
            return m.syms.iter().find(|s| s.off == off).map(|s| s.name.clone()).or_else(|| (off == 0).then(|| m.name.clone()));
        }
        self.ext.get(&addr).cloned()
    }

    /// Source line that starts at `addr`: "MODULE 27: Greet (3);".
    pub fn line_at(&self, addr: u32) -> Option<String> {
        let m = self.module_at(addr)?;
        let off = addr - m.base.unwrap();
        let i = m.lines.binary_search_by_key(&off, |l| l.0).ok()?;
        Some(m.source_text(m.lines[i].1))
    }

    /// The source line `addr` is in (nearest line start at or before it).
    pub fn line_of(&self, addr: u32) -> Option<String> {
        let m = self.module_at(addr)?;
        let off = addr - m.base.unwrap();
        let i = m.lines.partition_point(|l| l.0 <= off);
        let n = m.lines.get(i.checked_sub(1)?)?.1;
        Some(m.source_text(n))
    }

    /// Address of a name (procedure, label, external routine; any case). "MODULE.NAME" picks
    /// the module; "MODULE:LINE" a source line (statement number for Pascal).
    pub fn lookup(&self, name: &str) -> Option<u32> {
        let (modname, nm) = match name.split_once('.') {
            Some((a, b)) => (Some(a), b),
            None => (None, name),
        };
        if let Some((mn, line)) = name.split_once(':') {
            if let Ok(n) = line.parse::<u32>() {
                let m = self.modules.iter().find(|m| m.name.eq_ignore_ascii_case(mn))?;
                let b = m.base?;
                // the first code of that line, or of the next line that has code
                let by_line = m.lines.iter().filter(|l| l.1 >= n).min_by_key(|l| (l.1, l.0))?;
                // Pascal: lines are statement numbers; also accept the listing's LINE column
                let by_src = m
                    .source
                    .iter()
                    .filter_map(|(stmt, t)| t.split(':').next()?.trim().parse::<u32>().ok().filter(|&l| l == n).map(|_| *stmt))
                    .min()
                    .and_then(|stmt| m.lines.iter().filter(|l| l.1 >= stmt).min_by_key(|l| (l.1, l.0)));
                return Some(b + by_src.unwrap_or(by_line).0);
            }
        }
        for m in &self.modules {
            if let Some(mn) = modname {
                if !m.name.eq_ignore_ascii_case(mn) {
                    continue;
                }
            }
            let Some(b) = m.base
            else {
                continue;
            };
            if let Some(s) = m.syms.iter().find(|s| s.name.eq_ignore_ascii_case(nm)) {
                return Some(b + s.off);
            }
            if m.name.eq_ignore_ascii_case(nm) {
                // the main block of a Pascal module carries the module's name
                return Some(b);
            }
        }
        self.ext.iter().find(|(_, n)| n.eq_ignore_ascii_case(nm)).map(|(&a, _)| a)
    }

    /// Short status: modules and where they are.
    pub fn report(&self) -> String {
        let mut s = String::new();
        if let Some(f) = &self.from {
            s += &format!("symbols from {}\n", f.display());
        }
        for m in &self.modules {
            let procs = m.syms.iter().filter(|s| s.len > 0).count();
            s += &format!(
                "  {:<12} {:5} bytes, {:3} procedures, {:4} names, {:4} lines  {}\n",
                m.name,
                m.code.len(),
                procs,
                m.syms.len(),
                m.lines.len(),
                match m.base {
                    Some(b) => format!("at {:04X}:{:04X}", b >> 4, b & 0xF),
                    None => "not in memory".into(),
                }
            );
        }
        if !self.ext.is_empty() {
            s += &format!("  {} library routines named from the program's calls\n", self.ext.len());
        }
        s
    }

    /// Every procedure and label with its address (for the GUI's list), located ones only.
    pub fn all(&self) -> Vec<(String, u32)> {
        let mut v = Vec::new();
        for m in &self.modules {
            if let Some(b) = m.base {
                for s in &m.syms {
                    v.push((format!("{}.{}", m.name, s.name), b + s.off));
                }
            }
        }
        v
    }
}

/// Source text for a module from the listing next to its object file: NAME.pas.obj ->
/// NAME.lst (Pascal), NAME.asm.obj -> NAME.asm.lst (assembler).
fn attach_listing(m: &mut Module, obj: &Path) {
    let fname = obj.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    let lower = fname.to_ascii_lowercase();
    let dir = obj.parent().unwrap_or(Path::new("."));
    let stem = &fname[..lower.find('.').or_else(|| lower.find('~')).unwrap_or(fname.len())];
    let is_asm = lower.contains(".asm");
    let candidates: Vec<String> =
        if is_asm { vec![format!("{stem}.asm.lst"), format!("{stem}.lst")] } else { vec![format!("{stem}.lst"), format!("{stem}.pas.lst")] };
    for c in candidates {
        let Some(p) = find_ci(dir, &c)
        else {
            continue;
        };
        let Ok(raw) = std::fs::read(&p)
        else {
            continue;
        };
        let text = String::from_utf8_lossy(&raw);
        if is_asm || text.contains("MACRO ASSEMBLER") {
            let (lines, src) = read_asm_listing(&text);
            if m.lines.is_empty() {
                m.lines = lines;
            }
            m.source = src;
        }
        else {
            m.source = read_pascal_listing(&text);
        }
        return;
    }
}

/// A file in `dir` named `name`, any case.
fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let p = dir.join(name);
    if p.exists() {
        return Some(p);
    }
    std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
        p.file_name().and_then(|n| n.to_str()).map(|n| n.eq_ignore_ascii_case(name)).unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real debug builds: GRID_TEST_SYMS = a project folder with NAME.pas.obj files and the
    /// .run (gridbuild debug build).
    #[test]
    fn real_objects() {
        let Ok(dir) = std::env::var("GRID_TEST_SYMS")
        else {
            return;
        };
        let s = Symbols::load(Path::new(&dir)).unwrap();
        for m in &s.modules {
            println!("{} {} bytes, {} syms, {} lines, {} src, {} ext", m.name, m.code.len(), m.syms.len(), m.lines.len(), m.source.len(), m.ext.len());
            for x in m.syms.iter().take(4) {
                println!("   {:04X} {:4X} {}", x.off, x.len, x.name);
            }
            for l in m.lines.iter().take(3) {
                println!("   line {:?} {:?}", l, m.source.get(&l.1));
            }
        }
        // Lay the code out as the .run has it and find it again.
        let run = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().map(|e| e == "run").unwrap_or(false)).unwrap();
        let data = std::fs::read(run).unwrap();
        let mut mem = vec![0u8; 0x30000];
        let mut i = 0;
        while i + 3 <= data.len() {
            let t = data[i];
            let n = data[i + 1] as usize | (data[i + 2] as usize) << 8;
            let b = &data[i + 3..i + 2 + n];
            if t == 0x72 && b[1] == 1 {
                let off = b[2] as usize | (b[3] as usize) << 8;
                mem[0x12340 + off..0x12340 + off + b.len() - 4].copy_from_slice(&b[4..]);
            }
            i += 3 + n;
        }
        let mut s = s;
        s.locate(&mem);
        println!("{}", s.report());
        assert!(s.modules.iter().all(|m| m.base.is_some()));
        assert_eq!(s.modules.iter().filter_map(|m| m.base).min(), Some(0x12340));
        let first = &s.modules.iter().min_by_key(|m| m.base).unwrap();
        println!("{:?} {:?}", s.name_at(0x12345), s.line_of(0x12345 + 0x20));
        assert!(s.name_at(first.base.unwrap()).is_some());
        assert!(!s.ext.is_empty(), "no external routines named");
        println!("{:?}", s.ext.iter().take(8).collect::<Vec<_>>());
    }
}
