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

#[derive(Clone, PartialEq)]
struct Ram([u8; 16]);
impl FpuMem for Ram {
    fn read_u8(&mut self, a: u32) -> u8 {
        self.0[a as usize & 15]
    }
    fn write_u8(&mut self, a: u32, v: u8) {
        self.0[a as usize & 15] = v
    }
}

/// A random memory operand for a memory form: reals and integers with the edge cases
/// (zero, smallest, largest, denormal, infinity, NaN) well represented.
fn operand(r: &mut Rng, first: u8, modrm: u8, kinds: Kinds) -> Ram {
    let reg = (modrm >> 3) & 7;
    let mut m = Ram([0xEE; 16]);
    let (bytes, v): (usize, u128) = match (first & 7, reg) {
        (3, 5) | (3, 7) => {
            let v = loop {
                if let Some(v) = value(r, kinds) {
                    break v;
                }
            };
            (10, v)
        }
        (7, 4) | (7, 6) => {
            // packed decimal: 18 digits and a sign byte
            let digits = [0, 1, 3, 18, 18][r.below(5) as usize];
            let mut v = 0u128;
            for d in 0..digits {
                v |= (r.below(10) as u128) << (4 * d);
            }
            (10, v | (r.below(2) as u128) << 79)
        }
        (0, _) | (1, _) | (4, _) | (5, _) => {
            let (ebits, fbits) = if first & 4 == 0 { (8, 23) } else { (11, 52) };
            let emax = (1u128 << ebits) - 1;
            let n = match kinds {
                Kinds::Plain => 6,
                Kinds::Special => 8,
                _ => 9,
            };
            let (e, f) = match r.below(n) {
                0 => (0, 0),
                1..=3 => (emax / 2 - 4 + r.below(9) as u128, r.next() as u128),
                4 => (1 + r.below(emax as u64 - 1) as u128, r.next() as u128),
                5 => ([1, emax - 1][r.below(2) as usize], [0, u64::MAX as u128, r.next() as u128][r.below(3) as usize]),
                6 => (emax, 0),
                7 => (emax, r.next() as u128 | 1),
                _ => (0, (r.next() >> r.below(52)) as u128 | 1),
            };
            ((ebits + fbits + 1) / 8, (r.below(2) as u128) << (ebits + fbits) | e << fbits | (f & ((1 << fbits) - 1)))
        }
        _ => {
            let bytes = match (first & 7, reg) {
                (7, 5) | (7, 7) => 8,
                (2, _) | (3, _) => 4,
                _ => 2,
            };
            let v = match r.below(6) {
                0 => 0,
                1 => r.below(200) as i64 - 100,
                2 => i64::MIN >> (64 - 8 * bytes),
                3 => i64::MAX >> (64 - 8 * bytes),
                _ => (r.next() as i64) >> r.below(64),
            };
            (bytes, v as u128)
        }
    };
    m.0[..bytes].copy_from_slice(&v.to_le_bytes()[..bytes]);
    m
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
    /// also denormals
    Denormal,
    /// also unnormals
    All,
}

fn value(r: &mut Rng, kinds: Kinds) -> Option<u128> {
    let sign = (r.below(2) as u128) << 79;
    let n = match kinds {
        Kinds::Plain => 10,
        Kinds::Special => 13,
        Kinds::Denormal => 15,
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
        7 => {
            // any exponent; often at the ends of the range, for overflow and underflow
            let e = match r.below(4) {
                0 => 0x7FFE - r.below(3),
                1 => 1 + r.below(3),
                _ => 1 + r.below(0x7FFE),
            };
            (e as u128) << 64 | (r.next() | 1 << 63) as u128
        }
        8 => (0x3FFFu128 + r.below(3) as u128) << 64 | 1 << 63,
        9 => (0x3FFFu128 << 64) | ((r.below(16) << 60) | 1 << 63) as u128,
        10 => (0x7FFFu128 << 64) | 1 << 63,
        11 => (0x7FFFu128 << 64) | (r.next() | 3 << 62) as u128,
        12 => (0x7FFFu128 << 64) | (r.next() | 1 << 63) as u128 | 1,
        13 => (r.next() >> 1) as u128 | 1,
        14 => (r.next() >> (1 + r.below(63))) as u128 | 1,
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

fn same(old: &I8087, new: &Micro8087, first: u8, modrm: u8) -> bool {
    // rounding writes C1 in the microcode (the instruction-level core leaves it alone)
    let store = first & 1 == 1 && modrm < 0xC0 && modrm & 0x10 != 0;
    let m = if (first & 1 == 0 && modrm & 0x30 != 0x10) || store || (first == 0xD9 && modrm >= 0xF0) { 0x7D7F } else { 0x7F7F };
    if old.sw & m != new.status_word() & m || old.tw != new.tag_word() {
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
            let mut before = format!("top {} st(i) r{}\n{}", new.top, (new.top + (modrm & 7)) & 7, describe(&old, &new));
            // FSCALE: the 8087 leaves the result undefined unless ST(1) is zero or
            // 1 <= |ST(1)| < 2^15 (the microcode shifts by a garbage count)
            let st1 = new.st[(new.top as usize + 1) & 7];
            let undefined = (first, modrm) == (0xD9, 0xFD) && st1.tag == TAG_VALID && !(0x3FFF..=0x400D).contains(&st1.exp);
            // FPREM by zero: the microcode has no check for it and runs the divide loop
            // with a zero divisor; what the hardware leaves is not known
            let undefined = undefined || ((first, modrm) == (0xD9, 0xF8) && st1.tag == TAG_ZERO);
            let mut ram = (Ram([0; 16]), Ram([0; 16]));
            let mut mem_denormal = false;
            let res = if modrm < 0xC0 {
                ram.0 = operand(&mut r, first, modrm, kinds);
                // a denormal short or long real in arithmetic stays unnormal in the
                // microcode and so does the result: the old core is no reference
                let d = u64::from_le_bytes(ram.0 .0[..8].try_into().unwrap());
                mem_denormal = modrm & 0x30 != 0x10 && match first {
                    0xD8 => d & 0x7F_FFFF != 0 && (d >> 23) & 0xFF == 0,
                    0xDC => d & 0xF_FFFF_FFFF_FFFF != 0 && (d >> 52) & 0x7FF == 0,
                    _ => false,
                };
                ram.1 = ram.0.clone();
                before += &format!("  mem {:02X?}\n", &ram.0 .0[..10]);
                old.esc(&mut ram.0, 0, first, modrm, Some(0), 0);
                new.exec_mem(first & 7, modrm, Some((&mut ram.1, 0)))
            }
            else {
                old.esc(&mut NoMem, 0, first, modrm, None, 0);
                new.exec(first & 7, modrm)
            };
            // unnormals: the instruction-level core is no reference for them, so only
            // check that the microcode runs to its end
            // Two places where the microcode core is on its own (to be checked on a real
            // chip): a masked overflow in a directed rounding mode (the microcode gives
            // infinity when chopping and a largest number with all 64 fraction bits set,
            // and always flags overflow), and an underflow at 24 or 53 bits (the
            // microcode rounds the denormal again from the top of the fraction).
            let flags = old.sw | new.sw;
            // Also: with overflow or underflow unmasked the microcode stores the result with
            // its exponent moved by 6000h (the old core stores nothing).
            // A product or quotient that underflows is rounded twice by the microcode: to 64
            // bits first, then again as a denormal (one unit off now and then).
            let muldiv = first & 1 == 0 && matches!((modrm >> 3) & 7, 1 | 6 | 7);
            let open = (flags & SW_OE != 0 && cw & 0x0C00 != 0) || (flags & SW_UE != 0 && (cw & 0x0100 == 0 || muldiv)) || flags & !cw & (SW_OE | SW_UE) != 0;
            // FBSTP of something that is no 18-digit number: the microcode writes whatever
            // is left in the sum register under the FFFF of the indefinite
            let undefined = undefined || ((first, modrm & 0xF8) == (0xDF, 0x30) && flags & SW_IE != 0);
            let ok = res.is_ok() && (kinds == Kinds::All || open || mem_denormal || undefined || (same(&old, &new, first, modrm) && ram.0 == ram.1));
            if !ok {
                fails += 1;
                if shown < 2 {
                    shown += 1;
                    out += &format!("{:02X} {:02X} cw {:04X}: {}\nbefore:\n{}after:\n{}", first, modrm, cw, match &res {
                        Err(f) => f.to_string(),
                        Ok(_) => "different result".into(),
                    }, before, describe(&old, &new));
                    if modrm < 0xC0 {
                        out += &format!("  mem old {:02X?}\n      new {:02X?}\n", &ram.0 .0[..10], &ram.1 .0[..10]);
                    }
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

/// The microcode alone on every kind of operand and control word: it must reach its end.
fn runs(rom: &MicroRom, first: u8, modrm: u8) {
    let mut r = Rng(0x1980_8087);
    for _ in 0..5000 {
        let cw = CWS[r.below(CWS.len() as u64) as usize];
        let (_, mut new) = setup(rom, &mut r, Kinds::All, cw);
        let res = new.exec(first & 7, modrm);
        assert!(res.is_ok(), "{:02X} {:02X}: {:?}", first, modrm, res);
    }
}

fn range(first: u8, from: u8, n: u8) -> Vec<(u8, u8)> {
    (0..n).map(|i| (first, from + i)).collect()
}

const MASKED: [u16; 1] = [0x03FF];
/// the four rounding modes at 64, 53 and 24 bits, all masked
const ROUNDING: [u16; 12] = [0x033F, 0x073F, 0x0B3F, 0x0F3F, 0x023F, 0x063F, 0x0A3F, 0x0E3F, 0x003F, 0x043F, 0x083F, 0x0C3F];
/// all masked (projective, affine), all unmasked, invalid unmasked, denormal unmasked
const CWS: [u16; 5] = [0x03FF, 0x13FF, 0x0300, 0x03FE, 0x03FD];

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
    check(&ops, Kinds::Denormal, &CWS);
    check(&ops, Kinds::All, &CWS);
}

#[test]
fn constants_and_signs() {
    let mut ops = range(0xD9, 0xE8, 7); // FLD1 .. FLDZ
    ops.extend([(0xD9, 0xE0), (0xD9, 0xE1)]); // FCHS, FABS
    check(&ops, Kinds::Plain, &MASKED);
    check(&ops, Kinds::Denormal, &CWS);
    check(&ops, Kinds::All, &CWS);
}

#[test]
fn examine_and_compare() {
    let mut ops = vec![(0xD9, 0xE5), (0xD9, 0xE4)]; // FXAM, FTST
    ops.extend(range(0xD8, 0xD0, 16)); // FCOM, FCOMP
    ops.push((0xDE, 0xD9)); // FCOMPP
    check(&ops, Kinds::Plain, &MASKED);
    check(&ops, Kinds::Denormal, &CWS);
    check(&ops, Kinds::All, &CWS);
}

#[test]
fn add_subtract() {
    let mut ops = range(0xD8, 0xC0, 8); // FADD ST,ST(i)
    ops.extend(range(0xD8, 0xE0, 16)); // FSUB, FSUBR
    ops.extend(range(0xDC, 0xC0, 8));
    ops.extend(range(0xDC, 0xE0, 16));
    ops.extend(range(0xDE, 0xC0, 8)); // FADDP
    ops.extend(range(0xDE, 0xE0, 16));
    check(&ops, Kinds::Plain, &MASKED);
    check(&ops, Kinds::Plain, &ROUNDING);
    check(&ops, Kinds::Special, &ROUNDING);
    check(&ops, Kinds::Special, &CWS);
    check(&ops, Kinds::All, &CWS);
}

/// modrm bytes of the memory forms /reg (mod 00, any rm: the microcode only sees the byte)
fn mem(first: u8, regs: &[u8]) -> Vec<(u8, u8)> {
    regs.iter().map(|&reg| (first, reg << 3 | 6)).collect()
}

#[test]
fn load_store_real() {
    let mut ops = mem(0xD9, &[0, 2, 3]); // FLD, FST, FSTP m32
    ops.extend(mem(0xDD, &[0, 2, 3])); // m64
    ops.extend(mem(0xDB, &[5, 7])); // FLD, FSTP m80
    check(&ops, Kinds::Plain, &MASKED);
    check(&ops, Kinds::Plain, &ROUNDING);
    check(&ops, Kinds::Special, &ROUNDING);
    check(&ops, Kinds::Denormal, &ROUNDING);
    check(&ops, Kinds::Denormal, &CWS);
    check(&ops, Kinds::All, &CWS);
}

#[test]
fn load_store_integer() {
    let mut ops = mem(0xDF, &[0, 2, 3, 5, 7]); // FILD/FIST/FISTP m16, FILD/FISTP m64
    ops.extend(mem(0xDB, &[0, 2, 3])); // m32
    check(&ops, Kinds::Plain, &MASKED);
    check(&ops, Kinds::Plain, &ROUNDING);
    check(&ops, Kinds::Denormal, &ROUNDING);
    check(&ops, Kinds::Denormal, &CWS);
    check(&ops, Kinds::All, &CWS);
}

#[test]
fn add_subtract_compare_memory() {
    let mut arith = Vec::new();
    let mut com = Vec::new();
    for first in [0xD8, 0xDC, 0xDA, 0xDE] {
        arith.extend(mem(first, &[0, 4, 5])); // FADD, FSUB, FSUBR
        com.extend(mem(first, &[2, 3])); // FCOM, FCOMP
    }
    check(&arith, Kinds::Plain, &MASKED);
    check(&arith, Kinds::Plain, &ROUNDING);
    check(&arith, Kinds::Special, &ROUNDING);
    check(&arith, Kinds::Special, &CWS);
    check(&arith, Kinds::All, &CWS);
    check(&com, Kinds::Special, &MASKED);
    check(&com, Kinds::Denormal, &CWS);
    check(&com, Kinds::All, &CWS);
}

#[test]
fn multiply_divide() {
    let mut ops = Vec::new();
    for first in [0xD8, 0xDC, 0xDE] {
        ops.extend(range(first, 0xC8, 8)); // FMUL
        ops.extend(range(first, 0xF0, 16)); // FDIV, FDIVR
    }
    for first in [0xD8, 0xDC, 0xDA, 0xDE] {
        ops.extend(mem(first, &[1, 6, 7]));
    }
    check(&ops, Kinds::Plain, &MASKED);
    check(&ops, Kinds::Plain, &ROUNDING);
    check(&ops, Kinds::Special, &ROUNDING);
    check(&ops, Kinds::Special, &CWS);
    check(&ops, Kinds::All, &CWS);
}

#[test]
fn square_root() {
    let ops = [(0xD9, 0xFA)];
    check(&ops, Kinds::Plain, &MASKED);
    check(&ops, Kinds::Denormal, &CWS);
    check(&ops, Kinds::Plain, &ROUNDING);
    check(&ops, Kinds::Special, &ROUNDING);
    check(&ops, Kinds::Special, &CWS);
    check(&ops, Kinds::All, &CWS);
}

#[test]
fn round_and_scale() {
    let ops = [(0xD9, 0xFC), (0xD9, 0xFD)]; // FRNDINT, FSCALE
    check(&ops, Kinds::Plain, &MASKED);
    check(&ops, Kinds::Plain, &ROUNDING);
    check(&ops, Kinds::Special, &ROUNDING);
    check(&ops, Kinds::Special, &CWS);
    check(&ops, Kinds::All, &CWS);
}

#[test]
fn extract_and_remainder() {
    let ops = [(0xD9, 0xF4), (0xD9, 0xF8)]; // FXTRACT, FPREM
    check(&ops, Kinds::Plain, &MASKED);
    check(&ops, Kinds::Plain, &ROUNDING);
    check(&ops, Kinds::Special, &ROUNDING);
    check(&ops, Kinds::Special, &CWS);
    check(&ops, Kinds::All, &CWS);
}

#[test]
fn packed_decimal() {
    let ops = mem(0xDF, &[4, 6]); // FBLD, FBSTP
    check(&ops, Kinds::Plain, &MASKED);
    check(&ops, Kinds::Plain, &ROUNDING);
    check(&ops, Kinds::Denormal, &CWS);
    check(&ops, Kinds::All, &CWS);
}

/// FPTAN against what a real 8087 (in a Compass 1101) left on the stack: X in ST, Y in ST(1).
/// Returns the worst difference in units of the last place.
#[test]
fn fptan_real_chip() {
    let Some(rom) = MicroRom::find() else { return };
    let mut worst = 0;
    let mut exact = 0;
    for &(x, rx, ry) in crate::fptan::REAL.iter() {
        let mut m = Micro8087::new(rom.clone());
        m.cw = 0x03FF;
        for r in m.st.iter_mut() {
            r.tag = TAG_EMPTY;
        }
        m.top = 0;
        m.st[0] = Reg::from_bits(x);
        let res = m.exec(1, 0xF2);
        let (gx, gy) = (m.st[7].bits(), m.st[0].bits());
        let (dx, dy) = (gx as i128 - rx as i128, gy as i128 - ry as i128);
        eprintln!("{:020X}: X {:020X} ({:+}) Y {:020X} ({:+}) {:?}", x, gx, dx, gy, dy, res.as_ref().err().map(|f| f.to_string()));
        assert!(res.is_ok());
        worst = worst.max(dx.abs()).max(dy.abs());
        if dx == 0 && dy == 0 {
            exact += 1;
        }
    }
    // One X is one unit off: the arctangent constants are computed, not the chip's own
    // (the constant ROM's bits are not published), and their last bits decide such cases.
    assert!(worst <= 1 && exact >= 18, "worst {} units, {} of 19 exact", worst, exact);
}

/// FPTAN over the whole argument range against the instruction-level core's model (which
/// was fitted to the same real results and is good to a few units).
#[test]
fn fptan_range() {
    let Some(rom) = MicroRom::find() else { return };
    // every kind of operand runs to the end of the microcode
    runs(&rom, 0xD9, 0xF2);
    let mut r = Rng(0x8087_1980);
    let mut m = Micro8087::new(rom.clone());
    for _ in 0..20000 {
        let e = 0x3FFF - r.below(70) as u128;
        let x = (r.below(2) as u128) << 79 | e << 64 | (r.next() | 1 << 63) as u128;
        let Some((y, xx, _)) = crate::fptan::fptan(x) else { continue };
        m.cw = 0x03FF;
        m.sw = 0;
        for reg in m.st.iter_mut() {
            reg.tag = TAG_EMPTY;
        }
        m.top = 0;
        m.st[0] = Reg::from_bits(x);
        let res = m.exec(1, 0xF2);
        assert!(res.is_ok(), "{:020X}: {:?}", x, res);
        let (gx, gy) = (m.st[7].bits(), m.st[0].bits());
        let (dx, dy) = ((gx as i128 - xx as i128).abs(), (gy as i128 - y as i128).abs());
        assert!(dx <= 32 && dy <= 32, "{:020X}: X {:020X} / {:020X}, Y {:020X} / {:020X}", x, gx, xx, gy, y);
    }
}

/// Search aid: which last-bit variations of the arctangent constants agree with the real chip.
#[test]
#[ignore]
fn fptan_constant_search() {
    let Some(rom) = MicroRom::find() else { return };
    let mut m = Micro8087::new(rom.clone());
    let base = m.atan;
    let run = |m: &mut Micro8087| -> u32 {
        let mut bad = 0;
        for &(x, rx, ry) in crate::fptan::REAL.iter() {
            m.cw = 0x03FF;
            for r in m.st.iter_mut() {
                r.tag = TAG_EMPTY;
            }
            m.top = 0;
            m.sw = 0;
            m.st[0] = Reg::from_bits(x);
            let _ = m.exec(1, 0xF2);
            if m.st[7].bits() != rx || m.st[0].bits() != ry {
                bad += 1;
            }
        }
        bad
    };
    // one constant at a time, -8..+8
    for n in 0..16 {
        let mut line = format!("n={:2}:", n);
        for d in -8i128..=8 {
            m.atan = base;
            m.atan[n] = (base[n] as i128 + d) as u128;
            line += &format!(" {}", run(&mut m));
        }
        eprintln!("{}", line);
    }
}

/// FPATAN on 0 < y < x against the quad precision arctangent.
#[test]
fn arctangent() {
    use rustc_apfloat::ieee::{Quad, X87DoubleExtended as F80};
    use rustc_apfloat::{Float, FloatConvert, Round};
    let Some(rom) = MicroRom::find() else { return };
    runs(&rom, 0xD9, 0xF3);
    let mut r = Rng(0x1980_0F3F);
    let mut m = Micro8087::new(rom.clone());
    let mut worst = 0i128;
    let mut hist = [0u32; 8];
    let mut by_d = [0i128; 80];
    for i in 0..20000 {
        // y at most x, up to 80 binary places smaller
        let ex = 0x3FFF + r.below(40) as u128 - 20;
        let d = if i % 4 == 0 { r.below(80) } else { r.below(18) } as u128;
        let x = ex << 64 | (r.next() | 1 << 63) as u128;
        let mut y = (ex - d) << 64 | (r.next() | 1 << 63) as u128;
        if y >= x {
            y = (ex - 1) << 64 | y & 0xFFFF_FFFF_FFFF_FFFF;
        }
        m.cw = 0x03FF;
        m.sw = 0;
        for reg in m.st.iter_mut() {
            reg.tag = TAG_EMPTY;
        }
        m.top = 0;
        m.st[0] = Reg::from_bits(x);
        m.st[1] = Reg::from_bits(y);
        let res = m.exec(1, 0xF3);
        assert!(res.is_ok(), "{:020X} {:020X}: {:?}", y, x, res);
        assert!(m.top == 1 && m.st[0].tag == TAG_EMPTY, "FPATAN pops");
        let g = m.st[1].bits();
        let (qy, qx): (Quad, Quad) = (F80::from_bits(y).convert(&mut false).value, F80::from_bits(x).convert(&mut false).value);
        let t: F80 = crate::trans::atan2(qy, qx).convert_r(Round::NearestTiesToEven, &mut false).value;
        let t = t.to_bits();
        let e = (g as i128 - t as i128).abs();
        hist[(e as usize).min(7)] += 1;
        let dd = ((x >> 64) - (y >> 64)) as usize;
        by_d[dd] = by_d[dd].max(e);
        if e > worst {
            worst = e;
            eprintln!("atan({:020X} / {:020X}) = {:020X}, true {:020X} ({} units)", y, x, g, t, e);
        }
    }
    eprintln!("worst by exponent gap: {:?}", by_d);
    eprintln!("FPATAN error histogram in units of the last place: {:?}", hist);
    // small ratios go through chopped fixed-point divisions and lose up to five units
    assert!(worst <= 5 && by_d[..13].iter().all(|&e| e <= 1), "worst error {} units", worst);
    // outside the documented 0 < y < x
    for &(y, x) in &[(2.0f64, 1.0f64), (1.0, 1.0), (1.5, 1.0), (1.0, -2.0), (-1.0, 2.0), (3.0, 2.5)] {
        let f = |v: f64| -> u128 {
            let b = v.to_bits();
            let e = ((b >> 52) & 0x7FF) as u128 + 0x3FFF - 1023;
            (b >> 63) as u128 * (1 << 79) | e << 64 | 1 << 63 | ((b & ((1 << 52) - 1)) as u128) << 11
        };
        m.cw = 0x03FF;
        m.sw = 0;
        for reg in m.st.iter_mut() {
            reg.tag = TAG_EMPTY;
        }
        m.top = 0;
        m.st[0] = Reg::from_bits(f(x));
        m.st[1] = Reg::from_bits(f(y));
        let res = m.exec(1, 0xF3);
        eprintln!("atan({} / {}) = {:020X} sw {:04X} {:?}", y, x, m.st[1].bits(), m.sw, res.err());
    }
}

/// Run one transcendental on random operands and count how far the results are from the
/// quad precision value, in units of the last place, by the exponent of ST(0).
fn against_quad(
    rom: &MicroRom,
    modrm: u8,
    gen: &mut dyn FnMut(&mut Rng) -> (u128, Option<u128>),
    truth: &dyn Fn(rustc_apfloat::ieee::Quad, rustc_apfloat::ieee::Quad) -> rustc_apfloat::ieee::Quad,
) -> (i128, [u32; 8]) {
    use rustc_apfloat::ieee::{Quad, X87DoubleExtended as F80};
    use rustc_apfloat::{Float, FloatConvert, Round};
    let mut r = Rng(0x1980_0000 + modrm as u64);
    let mut m = Micro8087::new(rom.clone());
    let mut worst = 0i128;
    let mut hist = [0u32; 8];
    for _ in 0..20000 {
        let (x, y) = gen(&mut r);
        m.cw = 0x03FF;
        m.sw = 0;
        for reg in m.st.iter_mut() {
            reg.tag = TAG_EMPTY;
        }
        m.top = 0;
        m.st[0] = Reg::from_bits(x);
        if let Some(y) = y {
            m.st[1] = Reg::from_bits(y);
        }
        let res = m.exec(1, modrm);
        assert!(res.is_ok(), "{:020X} {:020X?}: {:?}", x, y, res);
        let g = m.st[m.top as usize].bits();
        let q = |v: u128| -> Quad { F80::from_bits(v).convert(&mut false).value };
        let t: F80 = truth(q(x), q(y.unwrap_or(0))).convert_r(Round::NearestTiesToEven, &mut false).value;
        let t = t.to_bits();
        let e = (g as i128 - t as i128).abs();
        hist[(e as usize).min(7)] += 1;
        if e > worst {
            worst = e;
            eprintln!("D9 {:02X} of {:020X} {:020X?} = {:020X}, true {:020X} ({} units)", modrm, x, y, g, t, e);
        }
    }
    eprintln!("D9 {:02X} error histogram in units of the last place: {:?}", modrm, hist);
    (worst, hist)
}

/// F2XM1 on 0 < x <= 0.5 against the quad precision value.
#[test]
fn exponential() {
    let Some(rom) = MicroRom::find() else { return };
    runs(&rom, 0xD9, 0xF0);
    let (worst, _) = against_quad(
        &rom,
        0xF0,
        &mut |r| {
            let span = if r.below(4) == 0 { 70 } else { 18 };
            ((0x3FFD - r.below(span) as u128) << 64 | (r.next() | 1 << 63) as u128, None)
        },
        &|x, _| crate::trans::exp2m1(x),
    );
    assert!(worst <= 3, "worst error {} units", worst);
}

/// FYL2X and FYL2XP1 against the quad precision values.
#[test]
fn logarithm() {
    let Some(rom) = MicroRom::find() else { return };
    runs(&rom, 0xD9, 0xF1);
    runs(&rom, 0xD9, 0xF9);
    let real = |r: &mut Rng, lo: u64, span: u64, signed: bool| -> u128 {
        let e = (lo + r.below(span)) as u128;
        let s = if signed { r.below(2) as u128 } else { 0 };
        s << 79 | e << 64 | (r.next() | 1 << 63) as u128
    };
    // y = 1: the logarithm alone
    let (worst, _) = against_quad(&rom, 0xF1, &mut |r| (real(r, 0x3FFF - 40, 80, false), Some(0x3FFF_8000_0000_0000_0000)), &|x, _| crate::trans::log2(x));
    assert!(worst <= 3, "FYL2X with y = 1: worst error {} units", worst);
    let (worst, _) = against_quad(&rom, 0xF1, &mut |r| (real(r, 0x3FFF - 40, 80, false), Some(real(r, 0x3FFF - 20, 40, true))), &|x, y| crate::trans::mul_q(y, crate::trans::log2(x)));
    assert!(worst <= 3, "FYL2X: worst error {} units", worst);
    // |x| below 1 - sqrt(2)/2
    let (worst, _) = against_quad(&rom, 0xF9, &mut |r| {
        let x = real(r, 0x3FFC - 70, 71, true);
        let x = if x & !(1 << 79) > 0x3FFD_95F6_0000_0000_0000 { x - (1 << 64) } else { x };
        (x, Some(real(r, 0x3FFF - 20, 40, true)))
    }, &|x, y| crate::trans::mul_q(y, crate::trans::log2_1p(x)));
    assert!(worst <= 5, "FYL2XP1: worst error {} units", worst);
}

/// The emulator's switch: a stream of ordinary instructions through `I8087::esc` gives the
/// same registers, status and memory with the microcode core as without it.
#[test]
fn emulator_switch() {
    let Some(rom) = MicroRom::find() else { return };
    let mut old = crate::I8087::new();
    let mut new = crate::I8087::new();
    new.use_microcode(rom);
    let (mut ram_old, mut ram_new) = (Ram([0; 16]), Ram([0; 16]));
    let mut r = Rng(0x8087_0C0D);
    // (opcode, modrm, memory operand): loads, arithmetic, stores, control
    let ops: [(u8, u8, bool); 29] = [
        (0xDF, 0x06, true),  // FILD m16
        (0xDB, 0x06, true),  // FILD m32
        (0xD9, 0xE8, false), // FLD1
        (0xD9, 0xEB, false), // FLDPI
        (0xD9, 0xC1, false), // FLD ST(1)
        (0xD8, 0xC1, false), // FADD ST, ST(1)
        (0xD8, 0xE2, false), // FSUB ST, ST(2)
        (0xD8, 0xC9, false), // FMUL ST, ST(1)
        (0xDE, 0xC1, false), // FADDP
        (0xDE, 0xC9, false), // FMULP
        (0xDA, 0x06, true),  // FIADD m32
        (0xDE, 0x0E, true),  // FIMUL m16
        (0xD9, 0xE1, false), // FABS
        (0xD9, 0xFA, false), // FSQRT
        (0xD9, 0xE0, false), // FCHS
        (0xD9, 0xC9, false), // FXCH
        (0xD9, 0xFC, false), // FRNDINT
        (0xD8, 0xD1, false), // FCOM
        (0xD9, 0xE4, false), // FTST
        (0xDD, 0x16, true),  // FST m64
        (0xD9, 0x1E, true),  // FSTP m32
        (0xDB, 0x3E, true),  // FSTP m80
        (0xDD, 0x06, true),  // FLD m64
        (0xDB, 0x2E, true),  // FLD m80
        (0xDD, 0xD8, false), // FSTP ST(0)
        (0xDD, 0x3E, true),  // FSTSW (hardwired)
        (0xD9, 0x3E, true),  // FSTCW (hardwired)
        (0xD9, 0xF6, false), // FDECSTP
        (0xDD, 0x36, true),  // FSAVE (hardwired; too long for this memory, wraps the same way)
    ];
    let mut resets = 0;
    for i in 0..30000u32 {
        // keep a few registers in use, and a few free
        let depth = (0..8).filter(|k| new.st_bits(*k).is_some()).count();
        let (opcode, modrm, mem) = match depth {
            0..=2 => ops[r.below(4) as usize],
            6..=8 => (0xDD, 0xD8, false),
            _ => ops[r.below(ops.len() as u64 - (i % 50 != 0) as u64) as usize],
        };
        if mem && (modrm >> 3) & 7 == 0 && opcode != 0xDD {
            // a small integer to load
            let v = r.below(2000) as u16;
            ram_old.0 = [0; 16];
            ram_old.0[0] = v as u8;
            ram_old.0[1] = (v >> 8) as u8;
            ram_new.0 = ram_old.0;
        }
        let ea = mem.then_some(0);
        // a real load reads what the matching store just wrote, not leftovers
        let before = match (opcode, modrm) {
            (0xDD, 0x06) => Some((0xDD, 0x16)),
            (0xDB, 0x2E) => Some((0xDB, 0x3E)),
            _ => None,
        };
        if let Some((o, m)) = before {
            old.esc(&mut ram_old, 0, o, m, ea, 0);
            new.esc(&mut ram_new, 0, o, m, ea, 0);
        }
        old.esc(&mut ram_old, 0, opcode, modrm, ea, 0);
        new.esc(&mut ram_new, 0, opcode, modrm, ea, 0);
        let what = format!("step {} {:02X} {:02X}", i, opcode, modrm);
        // NaNs and infinities (from empty registers, mostly) are the other tests' business:
        // start again
        let special = |f: &crate::I8087| (0..8).any(|k| f.st_bits(k).is_some_and(|b| (b >> 64) & 0x7FFF == 0x7FFF));
        if special(&old) || special(&new) {
            old.esc(&mut ram_old, 0, 0xDB, 0xE3, None, 0);
            new.esc(&mut ram_new, 0, 0xDB, 0xE3, None, 0);
            ram_new.0 = ram_old.0;
            resets += 1;
            continue;
        }
        // C1 after rounding is the microcode's own (the old core leaves it alone)
        old.sw = (old.sw & !0x0200) | (new.sw & 0x0200);
        assert_eq!(old.sw, new.sw, "{} status", what);
        assert_eq!(old.tw, new.tw, "{} tags", what);
        assert_eq!(ram_old.0, ram_new.0, "{} memory", what);
        for k in 0..8 {
            assert_eq!(old.st_bits(k).map(|b| format!("{:020X}", b)), new.st_bits(k).map(|b| format!("{:020X}", b)), "{} ST({})", what, k);
        }
    }
    eprintln!("{} instructions on the microcode core, {} restarts", new.micro_runs, resets);
    assert_eq!(new.micro_faults, 0);
    assert!(new.micro_runs > 25000 && resets < 1000);
}

/// Data for the timing fit: for ordinary operands, the busy clocks of the instruction-level
/// core's tables (fitted to data sheets and to a real 1101) next to what the microcode did.
/// I8087_MICROCODE=... cargo test -p i8087 timing_samples -- --ignored --nocapture
#[test]
#[ignore]
fn timing_samples() {
    let Some(rom) = MicroRom::find() else { return };
    let mut r = Rng(0x7131_1980);
    // (opcode, modrm, kind of ST(0)/ST(1) preparation)
    let mut list: Vec<(u8, u8, u8)> = Vec::new();
    for reg in 0..8u8 {
        for op in [0xD8u8, 0xDA, 0xDC, 0xDE] {
            list.push((op, reg << 3 | 6, 0)); // memory arithmetic
        }
        list.push((0xD8, 0xC1 | reg << 3, 0));
        list.push((0xDC, 0xC1 | reg << 3, 0));
        if reg != 2 && reg != 3 {
            list.push((0xDE, 0xC1 | reg << 3, 0));
        }
    }
    for (op, modrm) in [
        (0xD9, 0x06), (0xDD, 0x06), (0xDB, 0x2E), (0xDF, 0x06), (0xDB, 0x06), (0xDF, 0x2E), (0xDF, 0x26), // loads
        (0xD9, 0x16), (0xD9, 0x1E), (0xDD, 0x16), (0xDD, 0x1E), (0xDB, 0x3E), (0xDF, 0x16), (0xDF, 0x1E), (0xDB, 0x16), (0xDB, 0x1E), (0xDF, 0x3E), (0xDF, 0x36), // stores
        (0xD9, 0xC1), (0xD9, 0xC9), (0xDD, 0xD1), (0xDD, 0xD9), (0xD9, 0xE0), (0xD9, 0xE1), (0xD9, 0xE4), (0xD9, 0xE5),
        (0xD9, 0xE8), (0xD9, 0xE9), (0xD9, 0xEA), (0xD9, 0xEB), (0xD9, 0xEC), (0xD9, 0xED), (0xD9, 0xEE),
        (0xD9, 0xF4), (0xD9, 0xF6), (0xD9, 0xF7), (0xD9, 0xFC), (0xDE, 0xD9), (0xDD, 0xC1),
    ] {
        list.push((op, modrm, 0));
    }
    list.push((0xD9, 0xFA, 1)); // FSQRT
    list.push((0xD9, 0xF8, 0)); // FPREM
    list.push((0xD9, 0xFD, 2)); // FSCALE
    list.push((0xD9, 0xF2, 3)); // FPTAN
    list.push((0xD9, 0xF3, 4)); // FPATAN
    list.push((0xD9, 0xF0, 5)); // F2XM1
    list.push((0xD9, 0xF1, 1)); // FYL2X
    list.push((0xD9, 0xF9, 6)); // FYL2XP1
    let real = |r: &mut Rng, lo: i32, span: u64, signed: bool| -> u128 {
        let e = (0x3FFF + lo + r.below(span) as i32) as u128;
        let s = if signed { r.below(2) as u128 } else { 0 };
        s << 79 | e << 64 | (r.next() | 1 << 63) as u128
    };
    println!("op,modrm,clocks,steps,{}", KIND_NAMES.join(","));
    for &(opcode, modrm, prep) in &list {
        let n = if prep >= 3 { 60 } else { 12 };
        for _ in 0..n {
            let mut f = crate::I8087::new();
            f.use_microcode(rom.clone());
            // seven registers in use, ST(7) free for a push
            let mut ram = Ram([0; 16]);
            for _ in 0..7 {
                let v = real(&mut r, -6, 12, true);
                for i in 0..10 {
                    ram.0[i] = (v >> (8 * i)) as u8;
                }
                f.esc(&mut ram, 0, 0xDB, 0x2E, Some(0), 0);
            }
            let st0 = match prep {
                1 => Some(real(&mut r, -6, 12, false)),
                3 => Some(real(&mut r, -12, 12, false)),
                4 | 5 => Some(real(&mut r, -14, 13, false)),
                6 => Some(real(&mut r, -14, 12, true)),
                _ => None,
            };
            if prep == 2 {
                // FSCALE: a small whole number in ST(1)
                f.esc(&mut ram, 0, 0xDD, 0xD8, None, 0);
                f.esc(&mut ram, 0, 0xDD, 0xD8, None, 0);
                ram.0 = [0; 16];
                ram.0[0] = 1 + r.below(30) as u8;
                f.esc(&mut ram, 0, 0xDF, 0x06, Some(0), 0);
                f.esc(&mut ram, 0, 0xD9, 0xEB, None, 0);
            }
            if let Some(v) = st0 {
                f.esc(&mut ram, 0, 0xDD, 0xD8, None, 0);
                if prep == 4 {
                    // FPATAN: 0 < ST(1) < ST(0) = 1
                    f.esc(&mut ram, 0, 0xDD, 0xD8, None, 0);
                }
                for i in 0..10 {
                    ram.0[i] = (v >> (8 * i)) as u8;
                }
                f.esc(&mut ram, 0, 0xDB, 0x2E, Some(0), 0);
                if prep == 4 {
                    f.esc(&mut ram, 0, 0xD9, 0xE8, None, 0);
                }
            }
            // an ordinary memory operand
            let mem = modrm < 0xC0;
            if mem {
                let reg = (modrm >> 3) & 7;
                ram.0 = [0; 16];
                let v: u128 = match (opcode, reg) {
                    (0xD8, _) | (0xD9, _) => ((r.next() as f64 / 1e17 - 90.0) as f32).to_bits() as u128,
                    (0xDC, _) | (0xDD, _) => (r.next() as f64 / 1e17 - 90.0).to_bits() as u128,
                    (0xDB, 5) | (0xDB, 7) => real(&mut r, -6, 12, true),
                    (0xDF, 4) | (0xDF, 6) => {
                        let mut d = 0u128;
                        for i in 0..(1 + r.below(17)) {
                            d |= (r.below(10) as u128) << (4 * i);
                        }
                        d | 1
                    }
                    (0xDF, 5) | (0xDF, 7) => (r.next() >> (1 + r.below(50))) as u128 | 1,
                    (0xDA, _) | (0xDB, _) => (r.next() >> (33 + r.below(24))) as u128 | 1,
                    _ => (r.next() >> (49 + r.below(10))) as u128 | 1,
                };
                for i in 0..10 {
                    ram.0[i] = (v >> (8 * i)) as u8;
                }
            }
            f.sw &= !0x3F;
            f.micro_last = None;
            f.esc(&mut ram, 0, opcode, modrm, mem.then_some(0), 0);
            if f.sw & 0x1D != 0 {
                continue; // an exception: not an ordinary case
            }
            if let Some((clocks, run)) = f.micro_last {
                println!("{:02X},{:02X},{},{},{}", opcode, modrm, clocks, run.steps, run.kinds.map(|k| k.to_string()).join(","));
            }
        }
    }
}

/// The transcendentals against a real 1101, bit for bit.
fn real_chip(rom: &MicroRom, modrm: u8, rows: &[(&[u128], u8, f32)]) -> (usize, i128) {
    let mut m = Micro8087::new(rom.clone());
    let (mut exact, mut worst) = (0, 0i128);
    for (v, status, _) in rows {
        m.cw = 0x03FF;
        m.sw = 0;
        for reg in m.st.iter_mut() {
            reg.tag = TAG_EMPTY;
        }
        m.top = 0;
        let (x, want) = if v.len() == 3 { (v[1], v[2]) } else { (v[0], v[1]) };
        m.st[0] = Reg::from_bits(x);
        if v.len() == 3 {
            m.st[1] = Reg::from_bits(v[0]);
        }
        let res = m.exec(1, modrm);
        assert!(res.is_ok(), "{:?}", res);
        let got = m.st[m.top as usize].bits();
        let d = (got as i128 - want as i128).abs();
        if d == 0 && m.sw as u8 & 0x3F == *status {
            exact += 1;
        }
        else {
            eprintln!("D9 {:02X} {:020X?}: {:020X} sw {:02X}, real {:020X} sw {:02X} ({} units)", modrm, &v[..v.len() - 1], got, m.sw as u8, want, status, got as i128 - want as i128);
        }
        worst = worst.max(d);
    }
    eprintln!("D9 {:02X}: {} of {} exact, worst {} units", modrm, exact, rows.len(), worst);
    (exact, worst)
}

#[test]
fn transcendentals_real_chip() {
    let Some(rom) = MicroRom::find() else { return };
    use super::real::*;
    let rows = |t: &'static [([u128; 3], u8, f32)]| -> Vec<(&'static [u128], u8, f32)> { t.iter().map(|(v, s, c)| (&v[..], *s, *c)).collect() };
    let rows2 = |t: &'static [([u128; 2], u8, f32)]| -> Vec<(&'static [u128], u8, f32)> { t.iter().map(|(v, s, c)| (&v[..], *s, *c)).collect() };
    // (exact results expected at least, of how many)
    for (modrm, t, least) in [(0xF3u8, rows(&FPATAN), 34), (0xF0, rows2(&F2XM1), 31), (0xF1, rows(&FYL2X), 15), (0xF9, rows(&FYL2XP1), 24), (0xFA, rows2(&FSQRT), 12)] {
        let (exact, worst) = real_chip(&rom, modrm, &t);
        assert!(exact >= least && worst <= 1, "D9 {:02X}: {} of {} exact, worst {} units", modrm, exact, t.len(), worst);
    }
}

/// Data for the timing fit from the real chip: the real 1101's clocks for each dumped input
/// next to what the microcode did. Run with --ignored --nocapture.
#[test]
#[ignore]
fn timing_real() {
    let Some(rom) = MicroRom::find() else { return };
    use super::real::*;
    let mut m = Micro8087::new(rom.clone());
    println!("name,clocks,steps,{}", KIND_NAMES.join(","));
    let mut one = |name: &str, modrm: u8, v: &[u128], clocks: f32| {
        m.cw = 0x03FF;
        m.sw = 0;
        for reg in m.st.iter_mut() {
            reg.tag = TAG_EMPTY;
        }
        m.top = 0;
        let x = if v.len() == 3 { v[1] } else { v[0] };
        m.st[0] = Reg::from_bits(x);
        if v.len() == 3 {
            m.st[1] = Reg::from_bits(v[0]);
        }
        let run = m.exec(1, modrm).unwrap();
        println!("{},{},{},{}", name, clocks, run.steps, run.kinds.map(|k| k.to_string()).join(","));
    };
    for (v, _, c) in FPATAN.iter() {
        one("FPATAN", 0xF3, v, *c);
    }
    for (v, _, c) in F2XM1.iter() {
        one("F2XM1", 0xF0, v, *c);
    }
    for (v, _, c) in FYL2X.iter() {
        one("FYL2X", 0xF1, v, *c);
    }
    for (v, _, c) in FYL2XP1.iter() {
        one("FYL2XP1", 0xF9, v, *c);
    }
    for (v, _, c) in FSQRT.iter() {
        one("FSQRT", 0xFA, v, *c);
    }
}
