//! FPTAN the way the 8087 computes it (microcode as reverse-engineered by Ken Shirriff,
//! righto.com 2026/09 "8087 tangent"): CORDIC pseudo-division of the angle by up to 16
//! arctan(2^-i) constants (only the decisions are kept), a rational approximation
//! tan(r) ~ 3r / (3 - r^2) of the small rest, then the recorded rotations applied in reverse
//! (pseudo-multiplication). The result is the chip's partial tangent: Y (ST1) / X (ST0),
//! with X near 3, not 1.
//!
//! Known: the algorithm, the 67-bit constants' values (arctan(2^-i), i = 0..15), the paths
//! by exponent, the implicit exponents of X (1) and Y (-14, +1 per step). Assumed until a
//! real chip is measured: constants rounded to nearest, X kept with WX bits and Y with WY
//! bits, shifted-out bits dropped, results rounded to nearest when stored.

/// arctan(2^-i) * 2^(i+67), rounded to nearest: the constant ROM's 67-bit CORDIC values.
const ATAN: [u128; 16] = [
    0x6487ed5110b4611a6,
    0x76b19c1586ed3da2b,
    0x7d6dd7e4b203758ab,
    0x7f56ea6ab0bdb7196,
    0x7fd56edcb3f7a71b6,
    0x7ff556eea5d892a14,
    0x7ffd556eedca6addf,
    0x7fff5556eeea5cb40,
    0x7fffd5556eeedca5e,
    0x7ffff55556eeeea5d,
    0x7ffffd55556eeeedd,
    0x7fffff555556eeeef,
    0x7fffffd555556eeef,
    0x7ffffff5555556eef,
    0x7ffffffd5555556ef,
    0x7fffffff55555556f,
];

/// Bits of X (implicit exponent 1) and of Y (implicit exponent -14 + step).
const WX: u32 = 64;
const WY: u32 = 68;
/// Fraction bits of the angle while the CORDIC runs (exact for every input it takes).
const F: u32 = 120;

/// The chip's FPTAN: for a finite x with 0 < |x| < 2 (`bits` = 80-bit pattern), returns
/// (Y, X, clocks) as 80-bit patterns. None for other inputs (zero, specials, |x| >= 2):
/// the caller handles those.
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
    // the angle in fixed point, exact (CORDIC path: e >= -16, lowest bit 2^-79; direct
    // path: 8 more fraction bits, lowest bit 2^-126)
    let mut decisions = 0u32; // bit i: arctan(2^-i) was subtracted
    let mut steps = 0u32;
    let r: u128;
    let mut r_shift = 0u32; // r = r_int * 2^-(F + r_shift)
    if e >= -16 {
        let mut a: u128 = (mant as u128) << (F as i32 + e - 63);
        let i0 = (-e - 1).max(0) as u32;
        for i in i0..16 {
            let c = ATAN[i as usize] << (F - 67 - i);
            if a >= c {
                a -= c;
                decisions |= 1 << i;
            }
            steps += 1;
        }
        r = a;
    }
    else {
        // exponent -17 .. -63: no CORDIC; keep x exact with extra fraction bits
        r_shift = 8;
        r = (mant as u128) << (F as i32 + r_shift as i32 + e - 63);
    }
    let fr = F + r_shift;
    // rational: X0 = 3 - r^2 (implicit exponent 1, WX bits: unit 2^-(WX-2)),
    //           Y0 = 3r (implicit exponent -14, WY bits: unit 2^-(WY-2+15))
    let r2 = shr256(mul256(r, r), 2 * fr - (WX - 2));
    let mut x: u128 = (3u128 << (WX - 2)) - r2;
    let yu = WY - 2 + 15;
    // (without CORDIC, Y = 3x keeps x's own exponent: x + x/2 with the exponent raised)
    let direct = e < -16;
    let mut y: u128 = if direct { 3 * r } else { (3 * r) >> (fr - yu) };
    let mut iters = 0u32;
    let last = if decisions == 0 { 16 } else { decisions.trailing_zeros() };
    let mut i = 15i32;
    while decisions != 0 && i >= last as i32 {
        let iu = i as u32;
        if decisions & (1 << iu) != 0 {
            // Y += X * 2^-i, X -= Y * 2^-i (old Y); the implicit exponents make the first
            // a plain add, the second a shift by 2i
            let xadd = x << (WY - WX);
            let xsub = y >> (WY - WX + 2 * iu);
            y += xadd;
            x -= xsub;
        }
        iters += 1;
        if iu > last {
            y >>= 1;
        }
        i -= 1;
    }
    // values: X = x * 2^-(WX-2); Y = y * 2^-(WY-2+15-(15-last)) = y * 2^-(WY-2+last)
    let ys = if direct {
        -(fr as i32)
    }
    else if decisions == 0 {
        -(yu as i32)
    }
    else {
        -((WY - 2 + last) as i32)
    };
    let ybits = pack(sign, y, ys);
    let xbits = pack(0, x, -((WX - 2) as i32));
    let clocks = if e >= -16 { CLK_BASE + CLK_RATIONAL + steps * CLK_DIV_STEP + iters * CLK_MUL_STEP } else { CLK_BASE + CLK_RATIONAL };
    Some((ybits, xbits, clocks))
}

// Busy time (5 MHz clocks). The article's split for 0.95 (33% pseudo-division, 15% rational,
// 47% pseudo-multiplication, 5% overhead of ~450) scaled so tan 0.5 takes 527 clocks: with
// GRiD-OS's INT 3 for each WAIT that gives the 609 GRiDBench measured on a real 1101.
const CLK_TINY: u32 = 36;
/// tan(0): Y = 0, X = 1, no precision exception.
pub const CLK_ZERO: u32 = 36;
const CLK_BASE: u32 = 29;
const CLK_RATIONAL: u32 = 82;
const CLK_DIV_STEP: u32 = 11;
const CLK_MUL_STEP: u32 = 16;

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

/// v * 2^s as an 80-bit pattern, rounded to 64 bits (nearest, ties to even).
fn pack(sign: u128, v: u128, s: i32) -> u128 {
    if v == 0 {
        return sign << 79;
    }
    let n = 128 - v.leading_zeros() as i32;
    let (mut m, mut e) = if n > 64 {
        let sh = (n - 64) as u32;
        let mut m = v >> sh;
        let rem = v & ((1u128 << sh) - 1);
        let half = 1u128 << (sh - 1);
        if rem > half || (rem == half && m & 1 == 1) {
            m += 1;
        }
        (m, s + n - 1)
    }
    else {
        (v << (64 - n), s + n - 1)
    };
    if m >> 64 != 0 {
        m >>= 1;
        e += 1;
    }
    (sign << 79) | (((e + 16383) as u128) << 64) | m
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
        // small angles: X = 3 - x^2, Y = 3x; tiny ones: X = 1
        let (y, xx, _) = fptan(f80(1e-6)).unwrap();
        assert!((val(xx) - 3.0).abs() < 1e-11 && (val(y) - 3e-6).abs() < 1e-18);
        let (_, xx, _) = fptan(f80(1e-25)).unwrap();
        assert_eq!(val(xx), 1.0);
    }

    /// GRiDBench f02 on two real Compasses: FLD 0.5, FPTAN, FSTP qword X (rounds X to 53
    /// bits), FDIV qword X -> 3FFE 8BDA7ADF9A3A51D0. With X = 1 (the old emulation, and MAME)
    /// it is the correctly rounded ...5219: the chip's X near 3 is what moves it 73 ulps.
    /// Prints how far this model is (the last bits wait for a real-hardware FPTAN dump).
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
    fn half_takes_the_measured_time() {
        assert_eq!(fptan(f80(0.5)).unwrap().2, 527);
    }
}
