//! Transcendental functions in quad precision (113-bit significand), so that the results are
//! correctly rounded 80-bit values in almost all cases.

use rustc_apfloat::ieee::Quad;
use rustc_apfloat::{Float, Round};

const R: Round = Round::NearestTiesToEven;

fn q(s: &str) -> Quad {
    s.parse::<Quad>().unwrap()
}

fn ln2() -> Quad {
    q("0.693147180559945309417232121458176568075500134360255254120680009")
}

pub fn pi() -> Quad {
    q("3.14159265358979323846264338327950288419716939937510582097494459")
}

fn one() -> Quad {
    Quad::from_u128(1).value
}

fn add(a: Quad, b: Quad) -> Quad {
    a.add_r(b, R).value
}
fn sub(a: Quad, b: Quad) -> Quad {
    a.sub_r(b, R).value
}
fn mul(a: Quad, b: Quad) -> Quad {
    a.mul_r(b, R).value
}
fn div(a: Quad, b: Quad) -> Quad {
    a.div_r(b, R).value
}

/// Product in quad precision.
pub fn mul_q(a: Quad, b: Quad) -> Quad {
    mul(a, b)
}

fn small(t: Quad, s: Quad) -> bool {
    // |t| < |s| * 2^-120
    t.is_zero() || (!s.is_zero() && t.ilogb() < s.ilogb() - 120)
}

/// e^y - 1 (any y; best for |y| < 1).
pub fn expm1(y: Quad) -> Quad {
    let mut term = y;
    let mut sum = y;
    for k in 2..200u128 {
        term = div(mul(term, y), Quad::from_u128(k).value);
        sum = add(sum, term);
        if small(term, sum) {
            break;
        }
    }
    sum
}

/// 2^x - 1.
pub fn exp2m1(x: Quad) -> Quad {
    expm1(mul(x, ln2()))
}

/// 2 * atanh(t) = ln((1+t)/(1-t)), for |t| < 1/2.
fn two_atanh(t: Quad) -> Quad {
    let t2 = mul(t, t);
    let mut p = t;
    let mut sum = t;
    for k in 1..400u128 {
        p = mul(p, t2);
        let term = div(p, Quad::from_u128(2 * k + 1).value);
        sum = add(sum, term);
        if small(term, sum) {
            break;
        }
    }
    add(sum, sum)
}

/// ln(x), x > 0.
pub fn ln(x: Quad) -> Quad {
    let mut e = 0;
    let mut m = x.frexp(&mut e); // m in [0.5, 1)
    // move m to [sqrt(1/2), sqrt(2)) for a fast series
    let s2 = q("0.70710678118654752440084436210484903928483593768847403658833987");
    if m < s2 {
        m = add(m, m);
        e -= 1;
    }
    let t = div(sub(m, one()), add(m, one()));
    add(two_atanh(t), mul(Quad::from_i128(e as i128).value, ln2()))
}

/// ln(1 + x), accurate for small x.
pub fn ln1p(x: Quad) -> Quad {
    let two = Quad::from_u128(2).value;
    if x.abs() < q("0.4") {
        two_atanh(div(x, add(two, x)))
    }
    else {
        ln(add(one(), x))
    }
}

pub fn log2(x: Quad) -> Quad {
    div(ln(x), ln2())
}

pub fn log2_1p(x: Quad) -> Quad {
    div(ln1p(x), ln2())
}

fn sqrt(x: Quad) -> Quad {
    if x.is_zero() {
        return x;
    }
    // Newton from a double estimate
    let d: rustc_apfloat::StatusAnd<rustc_apfloat::ieee::Double> = rustc_apfloat::FloatConvert::convert(x, &mut false);
    let f = f64::from_bits(d.value.to_bits() as u64).sqrt();
    let dd = rustc_apfloat::ieee::Double::from_bits(f.to_bits() as u128);
    let mut y: Quad = rustc_apfloat::FloatConvert::convert(dd, &mut false).value;
    let half = div(one(), Quad::from_u128(2).value);
    for _ in 0..4 {
        y = mul(add(y, div(x, y)), half);
    }
    y
}

/// atan(z) for any z.
fn atan(z: Quad) -> Quad {
    if z.is_negative() {
        return -atan(-z);
    }
    if z > one() {
        return sub(div(pi(), Quad::from_u128(2).value), atan(div(one(), z)));
    }
    // two halvings: atan(z) = 2 atan(z / (1 + sqrt(1 + z^2)))
    let mut zz = z;
    for _ in 0..3 {
        zz = div(zz, add(one(), sqrt(add(one(), mul(zz, zz)))));
    }
    let z2 = mul(zz, zz);
    let mut p = zz;
    let mut sum = zz;
    for k in 1..400u128 {
        p = -mul(p, z2);
        let term = div(p, Quad::from_u128(2 * k + 1).value);
        sum = add(sum, term);
        if small(term, sum) {
            break;
        }
    }
    mul(sum, Quad::from_u128(8).value)
}

/// atan2(y, x) as FPATAN computes it: the angle of (x, y).
pub fn atan2(y: Quad, x: Quad) -> Quad {
    if x.is_zero() {
        let h = div(pi(), Quad::from_u128(2).value);
        return if y.is_negative() { -h } else { h };
    }
    let a = atan(div(y, x));
    if x.is_negative() {
        if y.is_negative() {
            sub(a, pi())
        }
        else {
            add(a, pi())
        }
    }
    else {
        a
    }
}

/// tan(x), with range reduction by pi.
pub fn tan(x: Quad) -> Quad {
    let p = pi();
    let k = div(x, p).round_to_integral(Round::NearestTiesToEven).value;
    let r = sub(x, mul(k, p));
    // sin and cos series
    let r2 = mul(r, r);
    let (mut s, mut c) = (r, one());
    let (mut ts, mut tc) = (r, one());
    for k in 1..200u128 {
        ts = -div(mul(ts, r2), Quad::from_u128((2 * k) * (2 * k + 1)).value);
        tc = -div(mul(tc, r2), Quad::from_u128((2 * k - 1) * (2 * k)).value);
        s = add(s, ts);
        c = add(c, tc);
        if small(ts, s) && small(tc, c) {
            break;
        }
    }
    div(s, c)
}
