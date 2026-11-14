//! FPTAN the way the 8087 computes it (microcode as reverse-engineered by Ken Shirriff,
//! righto.com 2026/09 "8087 tangent"): CORDIC pseudo-division of the angle by up to 16
//! arctan(2^-i) constants (only the decisions are kept), a rational approximation of the small
//! rest, then the recorded rotations applied in reverse (pseudo-multiplication). The result is
//! the chip's partial tangent Y (ST1) / X (ST0), with X near 1.5.
//!
//! Fitted to FPTAN dumps from a real 1101 (GRiDBench 2.0, 2026-11-11, n00-n18): every X and Y
//! there within 3 ulps (most 0-1). What the dumps showed:
//! - the rest r gives X0 = 1.5 - r^2/2 and Y0 = 1.5 r (half of 3 - r^2 and 3r);
//! - X is kept with 63 fraction bits and every subtraction from it takes one unit more
//!   (one's complement), Y with 74 fraction bits and every addition one unit less;
//! - the angle has 80 fraction bits, the constants 72 bits rounded to nearest;
//! - the results are truncated, Y normalized, X only shifted once: X < 1 is stored with
//!   exponent 3FFE as it is, so for 1.5 (outside the documented range) X is an unnormal;
//! - exponents -16 .. -63 skip CORDIC: X = 1.5 - x^2/2 - 1 unit, Y = 1.5 x;
//! - exponent -64 and below: Y = x, X = 1;
//! - time: ~160 clocks + ~18 per rotation step + ~13 per subtraction.

/// round(arctan(2^-i) * 2^72): the constant ROM's CORDIC values.
const ATAN: [u128; 16] = [
    0xc90fdaa22168c234c5,
    0x76b19c1586ed3da2b8,
    0x3eb6ebf25901bac55b,
    0x1fd5ba9aac2f6dc659,
    0xffaaddb967ef4e36d,
    0x7ff556eea5d892a14,
    0x3ffeaab776e5356f0,
    0x1fffd555bbba972d0,
    0xffffaaaaddddb94c,
    0x7ffff55556eeeea6,
    0x3ffffeaaaab77777,
    0x1fffffd55555bbbc,
    0xffffffaaaaaadde,
    0x7ffffff5555556f,
    0x3ffffffeaaaaaab,
    0x1fffffffd555555,
];

/// Fraction bits of the angle, of X and of Y.
const FA: u32 = 80;
const FX: u32 = 63;
const FY: u32 = 74;

/// The chip's FPTAN: for a finite x with 0 < |x| < pi/2 (`bits` = 80-bit pattern), returns
/// (Y, X, clocks) as 80-bit patterns. None for other inputs (zero, specials, larger x): the
/// caller handles those.
pub fn fptan(bits: u128) -> Option<(u128, u128, u32)> {
    let sign = (bits >> 79) & 1;
    let bexp = ((bits >> 64) & 0x7FFF) as i32;
    let mant = bits as u64;
    if bexp == 0 || bexp == 0x7FFF || mant >> 63 == 0 {
        return None;
    }
    let e = bexp - 16383; // x = mant * 2^(e - 63)
    if e > 0 {
        return None;
    }
    let one = 0x3FFF_8000_0000_0000_0000u128;
    if e <= -64 {
        // tan(x) = x within the precision: Y = x, X = 1
        return Some((bits, one, CLK_TINY));
    }
    if e <= -16 {
        // no CORDIC: X = 1.5 - x^2/2 - 1 unit (63 fraction bits), Y = 1.5 x
        let m = mant as u128;
        let (hi, lo) = mul256(m, m);
        let x = (3u128 << 62) - shr256((hi, lo), (64 - 2 * e) as u32) - 1;
        let ybits = pack_trunc(sign, 3 * m, e - 64);
        return Some((ybits, pack_x(x)?, CLK_DIRECT));
    }
    // the angle with FA fraction bits (exact)
    let mut a: u128 = (mant as u128) << (FA as i32 + e - 63);
    let mut decisions = 0u32; // bit i: arctan(2^-i) was subtracted
    for i in ((-e - 1).max(0) as u32)..16 {
        let c = ATAN[i as usize] << (FA - 72);
        if a >= c {
            a -= c;
            decisions |= 1 << i;
        }
    }
    if decisions == 0 {
        return None;
    }
    let rx = a >> (FA - FX);
    let mut x: i128 = (3i128 << (FX - 1)) - (shr256(mul256(rx, rx), FX + 1) as i128);
    let mut y: i128 = (((3 * a) >> 1) >> (FA - FY)) as i128;
    let last = decisions.trailing_zeros();
    for i in (last..16).rev() {
        if decisions & (1 << i) != 0 {
            // Y += X * 2^-i, X -= Y * 2^-i (old Y), in each one's own units
            let xadd = if i >= FY - FX { x >> (i - (FY - FX)) } else { x << ((FY - FX) - i) };
            let ysub = y >> (FY - FX + i);
            y += xadd - 1;
            x -= ysub + 1;
        }
    }
    if x <= 0 || y <= 0 {
        return None;
    }
    let ybits = pack_trunc(sign, y as u128, -(FY as i32));
    let iters = 16 - last;
    let clocks = CLK_CORDIC + iters * CLK_MUL_STEP + decisions.count_ones() * CLK_SUB;
    Some((ybits, pack_x(x as u128)?, clocks))
}

/// X (FX fraction bits, below 2) as stored: exponent 3FFF when X >= 1, else 3FFE without
/// normalizing further.
fn pack_x(x: u128) -> Option<u128> {
    if x >> (FX + 1) != 0 {
        return None;
    }
    Some(if x >> FX != 0 { (0x3FFFu128 << 64) | x } else { (0x3FFEu128 << 64) | (x << 1) })
}

// Busy time (5 MHz clocks), fitted to the real 1101's n-lines (clocks seen there minus what
// the emulator adds around the instruction).
const CLK_TINY: u32 = 64;
/// tan(0): Y = 0, X = 1, no precision exception.
pub const CLK_ZERO: u32 = 64;
const CLK_DIRECT: u32 = 151;
const CLK_CORDIC: u32 = 161;
const CLK_MUL_STEP: u32 = 18;
const CLK_SUB: u32 = 13;

/// a * b as (high, low) 128-bit halves.
fn mul256(a: u128, b: u128) -> (u128, u128) {
    let m = u64::MAX as u128;
    let (a1, a0, b1, b0) = (a >> 64, a & m, b >> 64, b & m);
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    let mid = (p00 >> 64) + (p01 & m) + (p10 & m);
    let lo = (p00 & m) | ((mid & m) << 64);
    let hi = p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64);
    (hi, lo)
}

/// (hi, lo) >> s, truncated, as u128 (the result must fit).
fn shr256(v: (u128, u128), s: u32) -> u128 {
    let (hi, lo) = v;
    if s >= 128 {
        hi >> (s - 128)
    }
    else if s == 0 {
        lo
    }
    else {
        (hi << (128 - s)) | (lo >> s)
    }
}

/// v * 2^s as an 80-bit pattern, truncated to 64 bits.
fn pack_trunc(sign: u128, v: u128, s: i32) -> u128 {
    if v == 0 {
        return sign << 79;
    }
    let n = 128 - v.leading_zeros() as i32;
    let m = if n > 64 { v >> (n - 64) } else { v << (64 - n) };
    (sign << 79) | (((s + n - 1 + 16383) as u128) << 64) | m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(b: u128) -> f64 {
        let e = ((b >> 64) & 0x7FFF) as i32 - 16383;
        let m = b as u64 as f64 / 2f64.powi(63);
        let s = if b >> 79 & 1 == 1 { -1.0 } else { 1.0 };
        s * m * 2f64.powi(e)
    }

    fn f80(x: f64) -> u128 {
        let b = x.to_bits();
        let e = ((b >> 52) & 0x7FF) as i32 - 1023;
        let m = ((b & ((1 << 52) - 1)) | (1 << 52)) << 11;
        (((b >> 63) as u128) << 79) | (((e + 16383) as u128) << 64) | m as u128
    }

    #[test]
    fn ratios_are_tangents() {
        for &x in &[0.5, 0.95, 0.785, 0.1, 1e-3, 1e-5, 3e-6, 1e-12, 1e-30, 1.2, 0.3] {
            let (y, xx, c) = fptan(f80(x)).unwrap();
            let t = val(y) / val(xx);
            assert!(((t - x.tan()) / x.tan()).abs() < 1e-15, "{x}: {t} vs {}", x.tan());
            assert!(c >= CLK_TINY && c <= 600);
        }
        // small angles: X = 1.5 - x^2/2, Y = 1.5x; tiny ones: X = 1
        let (y, xx, _) = fptan(f80(1e-6)).unwrap();
        assert!((val(xx) - 1.5).abs() < 1e-11 && (val(y) - 1.5e-6).abs() < 1e-18);
        let (_, xx, _) = fptan(f80(1e-25)).unwrap();
        assert_eq!(val(xx), 1.0);
    }

    /// FPTAN on a real 1101 (GRiDBench 2.0 n00-n18): x, X (ST0), Y (ST1).
    const REAL: [(u128, u128, u128); 19] = [
        (0x3FFE8000000000000000, 0x3FFFBC7A42BFE564E79E, 0x3FFECDEE79A3D9A66CEE),
        (0x3FFEF333333333333333, 0x3FFF9F417080938092E8, 0x3FFFDEB34639BDCA3BFD),
        (0x3FFEC000000000000000, 0x3FFFA1FC01F6A9C71F24, 0x3FFF96E772FB4114861A),
        (0x3FFD999999999999999A, 0x3FFFBD30E042B5EF1885, 0x3FFDEA182D495E6E53CC),
        (0x3FFBCCCCCCCCCCCCCCCD, 0x3FFFBF82432B4AB800A2, 0x3FFC99B8458C823FB2FB),
        (0x3FF8A3D70A3D70A3D70A, 0x3FFFBFFF22F54320C32E, 0x3FF8F5C38D4F2C244638),
        (0x3FF583126E978D4FDF3B, 0x3FFFBFFFFFB5AE620FF7, 0x3FF5C49BA9E2BCD1ADA7),
        (0x3FF38000000000000000, 0x3FFFBFFFFFFFFFFFBFFF, 0x3FF3C000003FFFFFD998),
        (0x3FF0C000000000000000, 0x3FFFBFFFFFFE3FFFFFFA, 0x3FF1900000005FFFFFFD),
        (0x3FEFA000000000000000, 0x3FFFBFFFFFFF9BFFFFFF, 0x3FEFF000000000000000),
        (0x3FEEC000000000000000, 0x3FFFBFFFFFFFDBFFFFFF, 0x3FEF9000000000000000),
        (0x3FDDDBE6FECEBDEDD5BF, 0x3FFFBFFFFFFFFFFFFFFF, 0x3FDEA4ED3F1B0E72604F),
        (0x3FC0C000000000000000, 0x3FFFBFFFFFFFFFFFFFFF, 0x3FC19000000000000000),
        (0x3FBFC000000000000000, 0x3FFF8000000000000000, 0x3FBFC000000000000000),
        (0x3FFEC8F5C28F5C28F5C3, 0x3FFF9CD5B812CD0D8C52, 0x3FFF9CB5C26C2EF1384E),
        (0x3FFF8000000000000000, 0x3FFF9429643735D5AE2A, 0x3FFFE6BF86FAB2CB3E2B),
        (0x3FFFC000000000000000, 0x3FFE2C455576196CD5E2, 0x40009C120662E028A4BF),
        (0x3FFF8000000000000000, 0x3FFF9429643735D5AE2A, 0x3FFFE6BF86FAB2CB3E2B),
        (0xBFFE8000000000000000, 0x3FFFBC7A42BFE564E79E, 0xBFFECDEE79A3D9A66CEE),
    ];

    #[test]
    fn matches_the_real_chip() {
        let mut worst = 0;
        for &(x, rx, ry) in REAL.iter() {
            let (y, xx, _) = fptan(x).unwrap();
            let dx = (xx as i128 - rx as i128).abs();
            let dy = (y as i128 - ry as i128).abs();
            eprintln!("{x:020X}: X {dx} Y {dy} ulps");
            worst = worst.max(dx).max(dy);
        }
        assert!(worst <= 3, "worst {worst} ulps");
    }

    /// GRiDBench f02 on two real Compasses: FLD 0.5, FPTAN, FSTP qword X (rounds X to 53
    /// bits), FDIV qword X -> 3FFE 8BDA7ADF9A3A51D0. With X = 1 (the old emulation, and MAME)
    /// it is the correctly rounded ...5219: the chip's own X is what moves it 73 ulps.
    #[test]
    fn gridbench_f02() {
        use rustc_apfloat::ieee::{Double, X87DoubleExtended as F80};
        use rustc_apfloat::{Float, FloatConvert};
        let (y, x, _) = fptan(f80(0.5)).unwrap();
        let x53: Double = F80::from_bits(x).convert(&mut false).value;
        let x80: F80 = x53.convert(&mut false).value;
        let q = (F80::from_bits(y) / x80).value.to_bits();
        let real = 0x3FFE_8BDA_7ADF_9A3A_51D0u128;
        eprintln!("f02: model {q:020X}, real {real:020X}, {} ulps", q as i128 - real as i128);
        assert!((q as i128 - real as i128).abs() < 16);
    }

    #[test]
    fn times_follow_the_steps() {
        assert_eq!(fptan(f80(1e-25)).unwrap().2, CLK_TINY);
        assert_eq!(fptan(f80(1e-6)).unwrap().2, CLK_DIRECT);
        let t = |x: f64| fptan(f80(x)).unwrap().2;
        assert!(t(1e-4) < t(1e-2) && t(1e-2) < t(0.5));
        assert!((520..560).contains(&t(0.5)));
    }
}
