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
    check(&[(0xD9, 0xF2)], Kinds::All, &CWS);
    let mut r = Rng(0x8087_1980);
    let mut m = Micro8087::new(rom);
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
