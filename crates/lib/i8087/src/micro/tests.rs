//! The microcode core against the instruction-level core, instruction by instruction, from
//! random register states. The tests need the microcode ROM (I8087_MICROCODE=path or
//! roms/8087mc.txt) and pass without doing anything when it is not there.

use super::core::*;
use super::rom::MicroRom;
use crate::{FpuMem, I8087};
use rustc_apfloat::ieee::X87DoubleExtended as F80;
use rustc_apfloat::Float;

struct NoMem;
impl FpuMem for NoMem {
    fn read_u8(&mut self, _: u32) -> u8 {
        0
    }
    fn write_u8(&mut self, _: u32, _: u8) {}
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 11) % n
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum Kinds {
    /// empty, zeros and normal numbers
    Plain,
    /// also infinities and NaNs
    Special,
    /// also denormals and unnormals
    All,
}

fn value(r: &mut Rng, kinds: Kinds) -> Option<u128> {
    let sign = (r.below(2) as u128) << 79;
    let n = match kinds {
        Kinds::Plain => 10,
        Kinds::Special => 13,
        Kinds::All => 16,
    };
    Some(sign | match r.below(n) {
        0 | 1 => return None,
        2 => 0,
        3..=6 => {
            // normal, exponents near each other so add/compare see every case
            let e = 0x3FFF - 4 + r.below(9) as u128;
            (e << 64) | (r.next() | 1 << 63) as u128
        }
        7 => ((1 + r.below(0x7FFE)) as u128) << 64 | (r.next() | 1 << 63) as u128,
        8 => (0x3FFFu128 + r.below(3) as u128) << 64 | 1 << 63,
        9 => (0x3FFFu128 << 64) | ((r.below(16) << 60) | 1 << 63) as u128,
        10 => (0x7FFFu128 << 64) | 1 << 63,
        11 => (0x7FFFu128 << 64) | (r.next() | 3 << 62) as u128,
        12 => (0x7FFFu128 << 64) | (r.next() | 1 << 63) as u128 | 1,
        13 => (r.next() >> 1) as u128 | 1,
        14 => (r.next() >> r.below(64)) as u128 | 1,
        _ => ((1 + r.below(0x7FFE)) as u128) << 64 | (r.next() >> 1) as u128 | 1,
    })
}

fn tag_of(bits: Option<u128>) -> u16 {
    match bits {
        None => 3,
        Some(b) => match Reg::from_bits(b).class() {
            TAG_VALID => 0,
            TAG_ZERO => 1,
            _ => 2,
        },
    }
}

/// Both cores in the same random state.
fn setup(rom: &MicroRom, r: &mut Rng, kinds: Kinds, cw: u16) -> (I8087, Micro8087) {
    let mut old = I8087::new();
    let mut new = Micro8087::new(rom.clone());
    let top = r.below(8) as u8;
    old.cw = cw;
    new.cw = cw;
    old.set_top(top as usize);
    new.top = top;
    let cc = (r.next() as u16) & 0x4700;
    old.sw |= cc;
    new.sw = cc;
    let mut tw = 0;
    for i in 0..8 {
        let v = value(r, kinds);
        tw |= tag_of(v) << (2 * i);
        match v {
            None => new.st[i].tag = TAG_EMPTY,
            Some(b) => {
                new.st[i] = Reg::from_bits(b);
                old.st[i] = F80::from_bits(b);
                let e = (b >> 64) & 0x7FFF;
                if e != 0 && e != 0x7FFF && b >> 63 & 1 == 0 {
                    old.raw[i] = Some(b);
                }
            }
        }
    }
    old.tw = tw;
    (old, new)
}

fn describe(old: &I8087, new: &Micro8087) -> String {
    let mut s = format!("old sw {:04X} tw {:04X} | new sw {:04X} tw {:04X}\n", old.sw & 0x7F7F, old.tw, new.status_word() & 0x7F7F, new.tag_word());
    for i in 0..8 {
        let o = if (old.tw >> (2 * i)) & 3 == 3 { None } else { Some(old.raw[i].unwrap_or(old.st[i].to_bits())) };
        let n = if new.st[i].tag == TAG_EMPTY { None } else { Some(new.st[i].bits()) };
        s += &format!("  r{} old {:>20} new {:>20}{}\n", i, o.map_or("empty".into(), |v| format!("{:020X}", v)), n.map_or("empty".into(), |v| format!("{:020X}", v)), if o == n { "" } else { "  <--" });
    }
    s
}

fn same(old: &I8087, new: &Micro8087) -> bool {
    if old.sw & 0x7F7F != new.status_word() & 0x7F7F || old.tw != new.tag_word() {
        return false;
    }
    (0..8).all(|i| (old.tw >> (2 * i)) & 3 == 3 || old.raw[i].unwrap_or(old.st[i].to_bits()) == new.st[i].bits())
}

/// Runs each (first byte, modrm) `rounds` times from random states; returns the failures
/// as text (empty = all the same).
pub fn compare(ops: &[(u8, u8)], kinds: Kinds, cws: &[u16], rounds: u32, seed: u64) -> Option<String> {
    let rom = MicroRom::find()?;
    let mut r = Rng(seed);
    let mut out = String::new();
    let mut bad = 0;
    for &(first, modrm) in ops {
        let mut shown = 0;
        let mut fails = 0;
        for _ in 0..rounds {
            let cw = cws[r.below(cws.len() as u64) as usize];
            let (mut old, mut new) = setup(&rom, &mut r, kinds, cw);
            let before = describe(&old, &new);
            old.esc(&mut NoMem, 0, first, modrm, None, 0);
            let res = new.exec(first & 7, modrm);
            let ok = res.is_ok() && same(&old, &new);
            if !ok {
                fails += 1;
                if shown < 2 {
                    shown += 1;
                    out += &format!("{:02X} {:02X} cw {:04X}: {}\nbefore:\n{}after:\n{}", first, modrm, cw, match &res {
                        Err(f) => f.to_string(),
                        Ok(_) => "different result".into(),
                    }, before, describe(&old, &new));
                }
            }
        }
        if fails > 0 {
            bad += 1;
            out += &format!("{:02X} {:02X}: {} of {} differ\n\n", first, modrm, fails, rounds);
        }
    }
    if bad == 0 {
        Some(String::new())
    }
    else {
        Some(out)
    }
}

fn check(ops: &[(u8, u8)], kinds: Kinds, cws: &[u16]) {
    match compare(ops, kinds, cws, 2000, 0x8087_1980) {
        None => eprintln!("8087 microcode ROM not found: test skipped"),
        Some(s) => assert!(s.is_empty(), "\n{}", s),
    }
}

fn range(first: u8, from: u8, n: u8) -> Vec<(u8, u8)> {
    (0..n).map(|i| (first, from + i)).collect()
}

const MASKED: [u16; 1] = [0x03FF];

#[test]
fn rom_parses() {
    assert!(MicroRom::parse(b"junk").is_err());
    if let Some(rom) = MicroRom::find() {
        assert_eq!(rom.0[1], 0xA200);
        assert_eq!(rom.0[1647], 0xFFFE);
    }
}

#[test]
fn stack_moves() {
    let mut ops = range(0xD9, 0xC0, 16); // FLD ST(i), FXCH
    ops.extend(range(0xDD, 0xC0, 8)); // FFREE
    ops.extend(range(0xDD, 0xD0, 16)); // FST, FSTP ST(i)
    ops.extend([(0xD9, 0xD0), (0xD9, 0xF6), (0xD9, 0xF7)]); // FNOP, FDECSTP, FINCSTP
    check(&ops, Kinds::Plain, &MASKED);
}

#[test]
fn constants_and_signs() {
    let mut ops = range(0xD9, 0xE8, 7); // FLD1 .. FLDZ
    ops.extend([(0xD9, 0xE0), (0xD9, 0xE1)]); // FCHS, FABS
    check(&ops, Kinds::Plain, &MASKED);
}

#[test]
fn examine_and_compare() {
    let mut ops = vec![(0xD9, 0xE5), (0xD9, 0xE4)]; // FXAM, FTST
    ops.extend(range(0xD8, 0xD0, 16)); // FCOM, FCOMP
    ops.push((0xDE, 0xD9)); // FCOMPP
    check(&ops, Kinds::Plain, &MASKED);
}
