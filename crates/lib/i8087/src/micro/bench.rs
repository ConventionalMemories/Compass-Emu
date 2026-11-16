//! The 8087 Bench tests (the GRiD program that ran on a real Compass 1101 and 1137) replayed
//! on the emulator's 8087, test by test: FINIT, FLDCW, the loads, the test's own code,
//! FSTSW, FSTENV, two FSTP m80. `testdata/bench-table.txt` holds the tests,
//! `testdata/bench-real.txt` the lines of the real machines.

use super::rom::MicroRom;
use crate::{FpuMem, I8087};
use std::collections::HashMap;

struct Ram(Vec<u8>);
impl FpuMem for Ram {
    fn read_u8(&mut self, a: u32) -> u8 {
        self.0[a as usize]
    }
    fn write_u8(&mut self, a: u32, v: u8) {
        self.0[a as usize] = v
    }
}

fn hex(s: &str) -> Vec<u8> {
    if s == "-" {
        return Vec::new();
    }
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

pub struct Test {
    pub id: String,
    pub flags: u8,
    pub cw: u16,
    pub loads: Vec<Vec<u8>>,
    pub mem: Vec<u8>,
    pub code: Vec<u8>,
    /// FSTP ST(0) after the test in the timed loop, and in the loop it is compared with
    pub pops: u8,
    pub base: u8,
}

pub fn table(text: &str) -> Vec<Test> {
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            Test {
                id: f[0].to_string(),
                flags: f[2].parse().unwrap(),
                cw: u16::from_str_radix(f[3], 16).unwrap(),
                loads: if f[4] == "-" { Vec::new() } else { f[4].split(',').map(hex).collect() },
                mem: hex(f[5]),
                code: hex(f[6]),
                pops: f[7].parse().unwrap(),
                base: f[8].parse().unwrap(),
            }
        })
        .collect()
}

thread_local! {
    /// Per instruction of the last test's code: busy clocks, memory form, what the microcode did.
    static FEATURES: std::cell::RefCell<Vec<(u32, u8, u8, Option<super::core::Run>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

const SW_AT: u32 = 200;
const ENV_AT: u32 = 210;
const R0_AT: u32 = 230;
const R1_AT: u32 = 240;
const CW_AT: u32 = 250;

/// Runs one test; `sw0` = the condition bits the program's own arithmetic left behind.
/// Returns the report line without the clocks.
pub fn run(f: &mut I8087, t: &Test, sw0: u16) -> String {
    let mut m = Ram(vec![0; 256]);
    for (k, l) in t.loads.iter().enumerate() {
        m.0[k * 10..k * 10 + 10].copy_from_slice(l);
    }
    m.0[30..30 + t.mem.len()].copy_from_slice(&t.mem);
    m.0[CW_AT as usize] = t.cw as u8;
    m.0[CW_AT as usize + 1] = (t.cw >> 8) as u8;
    let mut now = 1_000_000u64;
    let mut esc = |f: &mut I8087, m: &mut Ram, op: u8, modrm: u8, ea: Option<u32>| {
        now = now.max(f.busy_until()) + 10;
        f.esc(m, now, op, modrm, ea, 0x1234);
        f.busy_until() - now
    };
    FEATURES.with(|v| v.borrow_mut().clear());
    f.sw = (f.sw & !0x4700) | (sw0 & 0x4700);
    esc(f, &mut m, 0xDB, 0xE3, None); // FINIT
    esc(f, &mut m, 0xD9, 0x2E, Some(CW_AT)); // FLDCW
    for k in 0..t.loads.len() {
        esc(f, &mut m, 0xDB, 0x6F, Some(k as u32 * 10));
    }
    let c = &t.code;
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            0x9B | 0x90 => i += 1,
            0xC7 => {
                assert_eq!(c[i + 1], 0x47);
                let d = c[i + 2] as usize;
                m.0[d] = c[i + 3];
                m.0[d + 1] = c[i + 4];
                i += 5;
            }
            0xD8..=0xDF => {
                let modrm = c[i + 1];
                f.micro_last = None;
                let busy = if modrm >= 0xC0 {
                    i += 2;
                    esc(f, &mut m, c[i - 2], modrm, None)
                }
                else {
                    assert_eq!(modrm & 0xC7, 0x47, "{}", t.id);
                    i += 3;
                    esc(f, &mut m, c[i - 3], modrm, Some(c[i - 1] as u32))
                };
                let first = if modrm >= 0xC0 { c[i - 2] } else { c[i - 3] };
                FEATURES.with(|v| v.borrow_mut().push((busy as u32, first, modrm, f.micro_last.map(|l| l.1))));
            }
            b => panic!("{}: byte {:02X} in the test code", t.id, b),
        }
    }
    esc(f, &mut m, 0xDD, 0x3E, Some(SW_AT)); // FSTSW
    esc(f, &mut m, 0xD9, 0x36, Some(ENV_AT)); // FSTENV
    if t.flags & 16 != 0 {
        esc(f, &mut m, 0xDB, 0xE2, None); // FCLEX
    }
    esc(f, &mut m, 0xDB, 0x3E, Some(R0_AT)); // FSTP m80
    esc(f, &mut m, 0xDB, 0x3E, Some(R1_AT));
    let num = |m: &Ram, a: u32| (0..10).rev().map(|k| format!("{:02X}", m.0[a as usize + k])).collect::<String>();
    let w = |m: &Ram, a: u32| m.0[a as usize] as u16 | (m.0[a as usize + 1] as u16) << 8;
    let second = if t.flags & 1 != 0 { num(&m, 40) } else { num(&m, R1_AT) };
    format!("{} {} {} {:04X} {:04X}", t.id, num(&m, R0_AT), second, w(&m, SW_AT), w(&m, ENV_AT + 4))
}

/// The first five fields of the real lines, by id.
pub fn real(text: &str) -> HashMap<String, String> {
    let mut h = HashMap::new();
    for l in text.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() >= 5 && f[0].len() == 4 && f[1].len() == 20 {
            h.insert(f[0].to_string(), f[..5].join(" "));
        }
    }
    h
}

/// Writes every line the emulator's 8087 gives to the file FB_OUT (a tool for fitting; does
/// nothing without FB_OUT).
#[test]
fn bench_dump() {
    let Ok(out) = std::env::var("FB_OUT") else { return };
    let dir = std::env::var("FB_DIR").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/testdata").to_string());
    let tests = table(&std::fs::read_to_string(format!("{dir}/bench-table.txt")).unwrap());
    let sw0 = std::env::var("FB_SW0").ok().map_or(0x0400, |s| u16::from_str_radix(&s, 16).unwrap());
    let mut f = I8087::new();
    if std::env::var("FB_OLD").is_err() {
        f.use_microcode(MicroRom::find().expect("the microcode ROM"));
    }
    let mut s = String::new();
    for t in &tests {
        if t.flags & 4 != 0 {
            continue;
        }
        let tr = std::env::var("FB_TRACE").map_or(false, |x| x == t.id);
        if let Some(m) = f.micro.as_mut() {
            m.trace = tr;
        }
        if tr {
            eprintln!("=== {}", t.id);
        }
        s += &run(&mut f, t, sw0);
        s.push('\n');
    }
    std::fs::write(&out, s).unwrap();
    // the timing features of every test, time-only ones too: id, pops - base, then per
    // instruction "busy mem kinds..." (kinds - when the microcode did not run it)
    let mut s = String::new();
    for t in &tests {
        run(&mut f, t, sw0);
        s += &format!("{} {}", t.id, t.pops as i32 - t.base as i32);
        FEATURES.with(|v| {
            for (busy, first, modrm, r) in v.borrow().iter() {
                s += &format!(" | {} {:02X}{:02X} {}", busy, first, modrm, r.map_or("-".to_string(), |r| r.kinds.map(|k| k.to_string()).join(",")));
            }
        });
        s.push('\n');
    }
    std::fs::write(format!("{out}.feat"), s).unwrap();
}

/// Lists micro-instructions FB_LIST=from-to (a tool for reading the ROM).
#[test]
fn rom_list() {
    let Ok(r) = std::env::var("FB_LIST") else { return };
    let rom = MicroRom::find().expect("the microcode ROM");
    let (a, b) = r.split_once('-').unwrap();
    for pc in a.parse::<usize>().unwrap()..=b.parse::<usize>().unwrap() {
        eprintln!("#{:04} {:04X} {:?}", pc, rom.0[pc], super::decode::decode(rom.0[pc]));
    }
}

/// Searches the constant ROM's last bits: for each arctangent (FB_FIT=atan) or logarithm
/// (FB_FIT=log) constant the change of -8..8 units that makes the most real results exact.
#[test]
fn fit_constants() {
    let Ok(which) = std::env::var("FB_FIT") else { return };
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata");
    let tests = table(&std::fs::read_to_string(format!("{dir}/bench-table.txt")).unwrap());
    let want = real(&std::fs::read_to_string(format!("{dir}/bench-real.txt")).unwrap());
    let ops: &[&str] = if which == "atan" { &["9bd9f2", "9bd9f3"] } else { &["9bd9f0", "9bd9f1", "9bd9f9"] };
    let tests: Vec<&Test> = tests.iter().filter(|t| t.flags & 4 == 0 && want.contains_key(&t.id) && ops.iter().any(|o| hex(o) == t.code)).collect();
    let mut f = I8087::new();
    f.use_microcode(MicroRom::find().expect("the microcode ROM"));
    let score = |f: &mut I8087| {
        tests.iter().filter(|t| {
            let l = run(f, t, 0x0400);
            let (a, b): (Vec<&str>, Vec<&str>) = (l.split(' ').collect(), want[&t.id].split(' ').collect());
            a[1] == b[1] && a[2] == b[2] && (u16::from_str_radix(a[3], 16).unwrap() ^ u16::from_str_radix(b[3], 16).unwrap()) & 0x023F == 0
        }).count()
    };
    let mut best = score(&mut f);
    eprintln!("{} tests, {} exact at the start", tests.len(), best);
    for _pass in 0..3 {
        for i in 0..if which == "frac" { 4 } else { 16 } {
            let get = |f: &I8087| {
                let m = f.micro.as_ref().unwrap();
                match which.as_str() {
                    "atan" => m.atan[i],
                    "frac" => m.fconst[i] as u128,
                    _ => m.log[i],
                }
            };
            let set = |f: &mut I8087, v: u128| {
                let m = f.micro.as_mut().unwrap();
                match which.as_str() {
                    "atan" => m.atan[i] = v,
                    "frac" => m.fconst[i] = v as u64,
                    _ => m.log[i] = v,
                }
            };
            let base = get(&f);
            let mut pick = 0i32;
            for d in -8i32..=8 {
                set(&mut f, (base as i128 + d as i128) as u128);
                let s = score(&mut f);
                if s > best || (s == best && d.abs() < pick.abs()) {
                    best = s;
                    pick = d;
                }
            }
            set(&mut f, (base as i128 + pick as i128) as u128);
            if pick != 0 {
                eprintln!("constant {} {:+}: {:#x}, {} exact", i, pick, get(&f), best);
            }
        }
    }
    eprintln!("{} exact at the end", best);
}

/// Tests the real machines and the microcode core still disagree on (the list shrinks as
/// the core is fitted): transcendentals a unit off or far outside their range, FXAM of an
/// empty register (it reports what the register last held), and p029 (registers never
/// written since power-on).
const KNOWN: &str = include_str!("../../testdata/bench-known.txt");

/// Every 8087 Bench line of the real Compass 1101 / 1137 against the microcode core.
#[test]
fn bench_real() {
    let Some(rom) = MicroRom::find() else { return };
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata");
    let tests = table(&std::fs::read_to_string(format!("{dir}/bench-table.txt")).unwrap());
    let want = real(&std::fs::read_to_string(format!("{dir}/bench-real.txt")).unwrap());
    let known: Vec<&str> = KNOWN.lines().filter(|l| !l.starts_with('#') && !l.is_empty()).collect();
    let mut f = I8087::new();
    f.use_microcode(rom);
    let (mut n, mut bad, mut fixed) = (0, Vec::new(), Vec::new());
    for t in &tests {
        let Some(w) = want.get(&t.id) else { continue };
        n += 1;
        let line = run(&mut f, t, 0x0400);
        let (a, b): (Vec<&str>, Vec<&str>) = (line.split(' ').collect(), w.split(' ').collect());
        // the condition bits the test program's own arithmetic left before the test are
        // C2 in most tests and C3 in a few (the first, and the unmasked ones): not the 8087
        let x = u16::from_str_radix(a[3], 16).unwrap() ^ u16::from_str_radix(b[3], 16).unwrap();
        let same = a[1] == b[1] && a[2] == b[2] && a[4] == b[4] && (x == 0 || x == 0x4400);
        match (same, known.contains(&t.id.as_str())) {
            (false, false) => bad.push(format!("{}\n   real {}\n   here {}", t.id, w, line)),
            (true, true) => fixed.push(t.id.clone()),
            _ => {}
        }
    }
    eprintln!("{} lines compared, {} known differences", n, known.len());
    assert!(n > 1500);
    assert!(fixed.is_empty(), "these now agree, take them out of bench-known.txt: {:?}", fixed);
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}
