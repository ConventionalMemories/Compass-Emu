//! Check the fast core against the SingleStepTests 8086 suite. Set SST_DIR to the suite's
//! v1 folder (with metadata.json); without it the test does nothing. SST_ONLY=XX limits to
//! files starting with XX; SST_SHOW=n prints the first n failures per file.

use std::io::Read;

use grid_core::fast86::{Bus86, Fast86};
use serde_json::Value;

struct Flat(Vec<u8>);

impl Bus86 for Flat {
    fn read8(&mut self, a: u32) -> u8 {
        self.0[(a & 0xFFFFF) as usize]
    }
    fn write8(&mut self, a: u32, v: u8) {
        self.0[(a & 0xFFFFF) as usize] = v;
    }
    fn io_in8(&mut self, _p: u16) -> u8 {
        0xFF
    }
    fn io_out8(&mut self, _p: u16, _v: u8) {}
}

const REGS: [(&str, usize); 8] = [("ax", 0), ("cx", 1), ("dx", 2), ("bx", 3), ("sp", 4), ("bp", 5), ("si", 6), ("di", 7)];
const SEGS: [(&str, usize); 4] = [("es", 0), ("cs", 1), ("ss", 2), ("ds", 3)];

fn get(c: &Fast86, n: &str) -> u16 {
    if n == "ip" {
        return c.ip;
    }
    if n == "flags" {
        return c.flags;
    }
    if let Some(&(_, i)) = REGS.iter().find(|r| r.0 == n) {
        return c.r[i];
    }
    c.s[SEGS.iter().find(|r| r.0 == n).unwrap().1]
}

fn set(c: &mut Fast86, n: &str, v: u16) {
    if n == "ip" {
        c.ip = v;
    }
    else if n == "flags" {
        c.flags = v;
    }
    else if let Some(&(_, i)) = REGS.iter().find(|r| r.0 == n) {
        c.r[i] = v;
    }
    else {
        c.s[SEGS.iter().find(|r| r.0 == n).unwrap().1] = v;
    }
}

#[test]
fn single_step_tests() {
    let Ok(dir) = std::env::var("SST_DIR") else {
        eprintln!("SST_DIR not set, skipped");
        return;
    };
    let only = std::env::var("SST_ONLY").unwrap_or_default();
    let show: usize = std::env::var("SST_SHOW").ok().and_then(|s| s.parse().ok()).unwrap_or(2);
    let meta: Value = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/metadata.json")).unwrap()).unwrap();
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".json.gz") && n.starts_with(&only))
        .collect();
    files.sort();
    let (mut total_bad, mut total) = (0, 0);
    let mut mem = Flat(vec![0; 1 << 20]);
    for f in files {
        let stem = f.trim_end_matches(".json.gz");
        let (op, reg) = stem.split_once('.').map(|(a, b)| (a, Some(b))).unwrap_or((stem, None));
        let mut m = &meta["opcodes"][op];
        if let Some(r) = reg {
            m = &m["reg"][r];
        }
        let status = m["status"].as_str().unwrap_or("");
        if status == "fpu" {
            continue;
        }
        let mask = m["flags-mask"].as_u64().unwrap_or(0xFFFF) as u16;
        let mut s = String::new();
        flate2::read::GzDecoder::new(std::fs::File::open(format!("{dir}/{f}")).unwrap()).read_to_string(&mut s).unwrap();
        let tests: Vec<Value> = serde_json::from_str(&s).unwrap();
        let mut bad = 0;
        for t in &tests {
            let mut c = Fast86::new();
            for (k, v) in t["initial"]["regs"].as_object().unwrap() {
                set(&mut c, k, v.as_u64().unwrap() as u16);
            }
            for e in t["initial"]["ram"].as_array().unwrap() {
                mem.0[e[0].as_u64().unwrap() as usize] = e[1].as_u64().unwrap() as u8;
            }
            let init = c;
            c.step(&mut mem);
            let mut errs = Vec::new();
            let fin = t["final"]["regs"].as_object().unwrap();
            for n in ["ax", "bx", "cx", "dx", "sp", "bp", "si", "di", "es", "cs", "ss", "ds", "ip", "flags"] {
                let want = fin.get(n).map(|v| v.as_u64().unwrap() as u16).unwrap_or(get(&init, n));
                let got = get(&c, n);
                let (w, g) = if n == "flags" { (want & mask, got & mask) } else { (want, got) };
                if w != g {
                    errs.push(format!("{n} want {want:04X} got {got:04X}"));
                }
            }
            // a divide error pushes the flags the microcode's divide loop left; the fast core
            // pushes its own (documented difference), so skip those two bytes
            let div_err = matches!(stem, "D4" | "F6.6" | "F6.7" | "F7.6" | "F7.7") && get(&c, "cs") != get(&init, "cs") || get(&c, "ip") != get(&init, "ip").wrapping_add(t["bytes"].as_array().unwrap().len() as u16) && matches!(stem, "D4" | "F6.6" | "F6.7" | "F7.6" | "F7.7");
            let fl = ((get(&c, "ss") as usize) << 4) + get(&c, "sp").wrapping_add(4) as usize;
            for e in t["final"]["ram"].as_array().unwrap() {
                let a = e[0].as_u64().unwrap() as usize;
                if div_err && (a == fl & 0xFFFFF || a == (fl + 1) & 0xFFFFF) {
                    continue;
                }
                let (w, g) = (e[1].as_u64().unwrap() as u8, mem.0[a]);
                if w != g {
                    errs.push(format!("[{a:05X}] want {w:02X} got {g:02X}"));
                }
            }
            if !errs.is_empty() {
                if bad < show {
                    eprintln!("{f} #{} {}: {}", t["test_num"], t["name"].as_str().unwrap(), errs.join(", "));
                }
                bad += 1;
            }
            // clean up so stray writes don't leak into the next test
            for e in t["initial"]["ram"].as_array().unwrap().iter().chain(t["final"]["ram"].as_array().unwrap()) {
                mem.0[e[0].as_u64().unwrap() as usize] = 0;
            }
        }
        if bad > 0 {
            eprintln!("{f} ({status}): {bad}/{} failed", tests.len());
        }
        total_bad += bad;
        total += tests.len();
    }
    eprintln!("total {total_bad}/{total} failed");
    assert_eq!(total_bad, 0);
}
