use grid_core::bubble::Bubble;
use grid_core::serial::Serial8274;
use grid_core::sound::Sound;

fn params(b: &mut Bubble, blr: u16, ar: u16, t: u64) {
    b.write(1, 0x0B, t);
    for v in [blr as u8, (blr >> 8) as u8, 0x28, ar as u8, (ar >> 8) as u8] {
        b.write(0, v, t);
    }
}

#[test]
fn bubble_write_then_read() {
    let mut b = Bubble::new(vec![0; 3 * 128 * 1024]);
    let mut t = 0u64;
    // register test as GRiD's driver does it
    b.write(1, 0x0A, t);
    b.write(0, 0x5A, t);
    b.write(1, 0x0A, t);
    assert_eq!(b.read(0, t), 0x5A);
    params(&mut b, 0x1000, 0x17FC, t);
    b.write(1, 0x11, t); // Initialize
    t += 100_000;
    assert_eq!(b.read(1, t) & 0xC0, 0x40);
    // write 2 bubble pages (128 bytes) at logical page 3 (AR = 12)
    params(&mut b, 0x1002, 12, t);
    b.write(1, 0x13, t);
    let mut n = 0u32;
    while n < 128 {
        t += 50;
        let s = b.read(1, t);
        if s & 0x80 == 0 {
            break;
        }
        if s & 1 != 0 {
            b.write(0, (n * 7) as u8, t);
            n += 1;
        }
    }
    while b.read(1, t) & 0x80 != 0 {
        t += 1000;
    }
    assert_eq!(n, 128);
    assert!(b.dirty);
    assert_eq!(b.image[12 * 64 + 5], 35);
    // read it back
    params(&mut b, 0x1002, 12, t);
    b.write(1, 0x12, t);
    let mut got = Vec::new();
    loop {
        t += 50;
        let s = b.read(1, t);
        if s & 1 != 0 {
            got.push(b.read(0, t));
        }
        else if s & 0x80 == 0 {
            break;
        }
    }
    assert_eq!(got.len(), 128);
    assert!(got.iter().enumerate().all(|(i, &v)| v == (i as u32 * 7) as u8));
}

#[test]
fn serial_loopback_timing() {
    let mut s = Serial8274::new();
    s.baud_count = 33; // ~9470 bit/s with x16
    // WR4: x16, 1 stop, no parity; WR3: rx 8 bits enable; WR5: tx 8 bits, tx enable, DTR, RTS
    for (r, v) in [(4u8, 0x44u8), (3, 0xC1), (5, 0xEA), (1, 0x12)] {
        s.write(2, r, 0);
        s.write(2, v, 0);
    }
    // a byte reaches the shift register one transmit clock (33) after the write; the buffer
    // is free again then, so B goes in after that
    s.write(0, b'A', 0);
    s.write(0, b'B', 100);
    s.advance(1_000_000);
    let out: Vec<_> = s.tx_out.iter().cloned().collect();
    assert_eq!(out.len(), 2);
    let cc = 33 * 16 * 10;
    assert_eq!(out[0], (33 + cc, b'A'));
    assert_eq!(out[1], (33 + 2 * cc, b'B'));
    s.rx_in.extend(b"hi");
    s.advance(1_000_000);
    s.advance(1_000_000 + cc);
    s.advance(1_000_000 + 2 * cc);
    assert!(s.int_line());
    assert_eq!(s.read(0, 1_000_000 + 2 * cc), b'h');
    assert_eq!(s.read(0, 1_000_000 + 2 * cc), b'i');
}

#[test]
fn sound_square_and_dtmf() {
    let mut s = Sound::new(5_000_000.0, 44100);
    // 1 kHz square wave for 0.1 s
    let half = 2500u64;
    for i in 0..200u64 {
        s.write_dac(if i % 2 == 0 { 200 } else { 0 }, i * half);
    }
    s.advance(200 * half);
    let n = s.out.len();
    assert!((4400..=4420).contains(&n), "{n}");
    let peak = s.out.iter().fold(0f32, |a, &b| a.max(b.abs()));
    assert!(peak > 0.3);
    s.write_8255(1, 0x76 | 0x80, 200 * half); // '1'
    assert_eq!(s.dtmf(), Some((697.0, 1209.0)));
}

/// GRiDProbe B1/B3 on a real 1101: 1-page reads in a given address order, timed from the
/// command to busy off (ms), polled every 205 clocks like GRiDProbe's loop.
fn bubble_ops(b: &mut Bubble, t: &mut u64, addrs: &[u16], pages: u16) -> Vec<f64> {
    let mut out = Vec::new();
    for &a in addrs {
        params(b, 0x1000 | pages, a, *t);
        let t0 = *t;
        b.write(1, 0x12, *t);
        loop {
            *t += 205;
            let s = b.read(1, *t);
            if s & 1 != 0 {
                b.read(0, *t);
            }
            else if s & 0x80 == 0 {
                break;
            }
        }
        out.push((*t - t0) as f64 / 5000.0);
        *t += 20_000;
    }
    out
}

#[test]
fn bubble_seek_times_like_a_real_1101() {
    let mut b = Bubble::new(vec![0; 3 * 128 * 1024]);
    let mut t = 0u64;
    params(&mut b, 0x1000, 0, t);
    b.write(1, 0x11, t);
    t += 200_000;
    let avg = |v: &[f64]| v[1..].iter().sum::<f64>() / (v.len() - 1) as f64;
    let seq = |f: &dyn Fn(u16) -> u16| (0..16).map(f).collect::<Vec<u16>>();
    // real: b31 87, b32 12.8, b33 20.3, b34 41.9, b35 46.3, b36 46 (80/12.5), b37 80, b38 25.9
    let cases: [(&str, Vec<u16>, f64); 8] = [
        ("same", seq(&|_| 0), 87.0),
        ("step 1", seq(&|i| i), 12.8),
        ("step 2", seq(&|i| i * 2), 20.3),
        ("step 16", seq(&|i| i * 16), 41.9),
        ("0/1024", seq(&|i| (i & 1) * 1024), 46.3),
        ("0/2047", seq(&|i| (i & 1) * 2047), 46.3),
        ("2047 down", seq(&|i| 2047 - i), 80.0),
        ("step 512", seq(&|i| (i * 512) & 2047), 25.9),
    ];
    for (name, addrs, real) in cases {
        let v = bubble_ops(&mut b, &mut t, &addrs, 1);
        let a = avg(&v);
        eprintln!("{name}: {a:.2} ms (real {real})");
        assert!((a - real).abs() / real < 0.08, "{name}: {a:.2} ms, real {real}");
    }
    // b13/b14: 16 and 64 pages from page 0, repeated (real 1101 170.7 and 515.7 ms)
    let p16 = bubble_ops(&mut b, &mut t, &[0; 8], 16);
    let p64 = bubble_ops(&mut b, &mut t, &[0; 4], 64);
    let (a16, a64) = (p16.iter().sum::<f64>() / 8.0, p64.iter().sum::<f64>() / 4.0);
    eprintln!("16 pages {a16:.1} ms, 64 pages {a64:.1} ms, a page {:.3}", (a64 - a16) / 48.0);
    assert!((a16 - 170.7).abs() < 10.0 && (a64 - 515.7).abs() < 25.0);
}
