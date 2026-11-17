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

/// Read `pages` pages from page 0 by polling; Abort after `abort_at` bytes (if Some). Returns
/// the bytes read and the final status.
fn bubble_poll_read(b: &mut Bubble, t: &mut u64, pages: u16, abort_at: Option<usize>) -> (usize, u8) {
    params(b, 0x1000 | pages, 0, *t);
    b.write(1, 0x12, *t);
    let mut n = 0;
    loop {
        *t += 50;
        if abort_at == Some(n) {
            b.write(1, 0x19, *t);
            while b.read(1, *t) & 0x80 != 0 {
                *t += 50;
            }
            b.write(1, 0x1D, *t); // Reset FIFO
            *t += 2_000;
            break;
        }
        let s = b.read(1, *t);
        if s & 1 != 0 {
            b.read(0, *t);
            n += 1;
        }
        else if s & 0x80 == 0 {
            break;
        }
    }
    let s = b.read(1, *t);
    (n, s)
}

/// GRiDProbe B7 on a real 1101: writes end with status 42; an Abort between two pages of a
/// read leaves multi-page reads stopping after one page with status 2C until a Software Reset.
#[test]
fn bubble_write_status_and_stuck_abort() {
    let mut b = Bubble::new(vec![0; 3 * 128 * 1024]);
    let mut t = 0u64;
    params(&mut b, 0x1000, 0, t);
    b.write(1, 0x11, t);
    t += 200_000;
    b.read(1, t);
    // one page written: status 42, FSA status marks MBM 0
    params(&mut b, 0x1001, 0, t);
    b.write(1, 0x13, t);
    let mut n = 0;
    loop {
        t += 50;
        let s = b.read(1, t);
        if s & 0x80 == 0 {
            assert_eq!(s & 0x7E, 0x42);
            break;
        }
        if s & 1 != 0 && n < 64 {
            b.write(0, 0, t);
            n += 1;
        }
    }
    b.write(1, 0x18, t);
    t += 5_000;
    let fsa: Vec<u8> = (0..6).map(|_| b.read(0, t)).collect();
    assert_eq!(fsa, [0xFF, 0x28, 0x20, 0x20, 0x20, 0x20]);
    // Abort after 1 byte: harmless
    assert_eq!(bubble_poll_read(&mut b, &mut t, 16, Some(1)).0, 1);
    assert_eq!(bubble_poll_read(&mut b, &mut t, 4, None), (256, 0x40));
    // Abort after a whole page: the next multi-page reads stop after 64 bytes, failed
    assert_eq!(bubble_poll_read(&mut b, &mut t, 16, Some(64)).0, 64);
    assert_eq!(bubble_poll_read(&mut b, &mut t, 16, None), (64, 0x2C));
    assert_eq!(bubble_poll_read(&mut b, &mut t, 4, None), (64, 0x2C));
    b.write(1, 0x1F, t); // Software Reset
    t += 2_000;
    b.read(1, t);
    assert_eq!(bubble_poll_read(&mut b, &mut t, 4, None), (256, 0x40));
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

/// SDLC the way GRiDLink~Device~ drives the 8274: send a short frame by underrun with CRC,
/// receive a frame with address search, CRC bytes, End of Frame and the extra byte.
#[test]
fn serial_sdlc_frames() {
    use grid_core::serial::crc_sdlc;
    let mut s = Serial8274::new();
    s.baud_count = 20; // 250 kbit/s, x1
    let cc = 20 * 8;
    let wr = |s: &mut Serial8274, r: u8, v: u8, t: u64| {
        s.write(2, r, t);
        s.write(2, v, t);
    };
    s.write(2, 0x18, 0); // channel reset
    wr(&mut s, 4, 0x20, 0);
    wr(&mut s, 1, 0x0C, 0);
    wr(&mut s, 6, 0xC0, 0);
    wr(&mut s, 7, 0x7E, 0);
    wr(&mut s, 3, 0xDD, 0);
    wr(&mut s, 5, 0x6B, 0);
    // transmit: address, reset EOM latch, control, then wait for the latch
    s.write(0, 0xC0, 100);
    s.write(2, 0xC0, 110);
    assert_eq!(s.read(2, 120) & 0x40, 0);
    s.write(0, 0x73, 200);
    s.advance(100_000);
    assert_ne!(s.read(2, 100_000) & 0x40, 0);
    assert_eq!(s.sdlc_out.len(), 1);
    assert_eq!(s.sdlc_out[0].1, vec![0xC0, 0x73]);
    // receive: one frame for another station (ignored), one for us
    s.sdlc_in.push_back(vec![0x81, 0x11]);
    s.sdlc_in.push_back(vec![0xC0, 0xBF]);
    let mut t = 200_000;
    s.advance(t);
    let mut got = vec![];
    let mut eof_at = None;
    while got.len() < 5 && t < 400_000 {
        t += 40;
        s.advance(t);
        if s.read(2, t) & 1 != 0 {
            s.write(2, 1, t);
            if s.read(2, t) & 0x80 != 0 && eof_at.is_none() {
                eof_at = Some(got.len());
            }
            got.push(s.read(0, t));
        }
    }
    let crc = crc_sdlc(&[0xC0, 0xBF]).to_le_bytes();
    assert_eq!(got, vec![0xC0, 0xBF, crc[0], crc[1], 0x7E]);
    assert_eq!(eof_at, Some(3), "EOF shows with the second CRC byte at the top");
    assert!(t < 200_000 + 2 * 6 * cc + 2000);
    // still latched until Error Reset
    s.write(2, 1, t);
    assert_ne!(s.read(2, t) & 0x80, 0);
    s.write(2, 0x30, t);
    s.write(2, 1, t);
    assert_eq!(s.read(2, t) & 0x80, 0);
    // CTS follows RTS on a point-to-point cable
    s.cts_loop = true;
    wr(&mut s, 5, 0x00, t);
    assert_eq!(s.read(2, t) & 0x20, 0);
    wr(&mut s, 5, 0x02, t);
    assert_ne!(s.read(2, t) & 0x20, 0);
}

/// Count modules the way the Compass II ROM driver (sized bigbubbles.plm, 1984) does: register
/// test, Abort, Software Reset, Reset FIFO, Read FSA Status, then read the data port while the
/// status shows FIFO ready, counting the bytes that are not FFH, halved.
fn rom_driver_count(b: &mut Bubble, t: &mut u64) -> usize {
    b.write(1, 0x0A, *t);
    b.write(0, 0x5A, *t);
    b.write(1, 0x0A, *t);
    assert_eq!(b.read(0, *t), 0x5A); // the RAC now points at the BLR
    let mut n = 0;
    for cmd in [0x19, 0x1F, 0x1D, 0x18] {
        b.write(1, cmd, *t);
        while b.read(1, *t) & 0x80 != 0 {
            *t += 100;
        }
    }
    let mut guard = 0;
    while b.read(1, *t) & 1 != 0 && guard < 100 {
        if b.read(0, *t) != 0xFF {
            n += 1;
        }
        guard += 1;
    }
    n / 2
}

/// GRiD-OS 3.1.5D on a 113x sizes the bubble from Read FSA Status. A command must point the
/// RAC back at the FIFO, and on the Compass II board every channel answers, or the driver
/// counts too many (or too few) modules and Initialize Media fails past the last page.
#[test]
fn bubble_module_count_for_compass_ii_driver() {
    for mbms in 1..=4 {
        let mut b = Bubble::new(vec![0; mbms * 128 * 1024]);
        b.fsa_first_ff = false;
        let mut t = 0u64;
        assert_eq!(rom_driver_count(&mut b, &mut t), mbms, "{mbms} modules");
    }
    // the 1101 keeps its measured FF for the first channel
    let mut b = Bubble::new(vec![0; 3 * 128 * 1024]);
    let mut t = 0u64;
    assert_eq!(rom_driver_count(&mut b, &mut t), 2);
}

/// Initialize naming a module that is not fitted never finds its bootloop: busy until Abort.
#[test]
fn bubble_initialize_missing_module_stays_busy() {
    let mut b = Bubble::new(vec![0; 3 * 128 * 1024]);
    let mut t = 0u64;
    params(&mut b, 0x1000, 3 << 11, t);
    b.write(1, 0x11, t);
    t += 10_000_000;
    assert_eq!(b.read(1, t) & 0x80, 0x80);
    b.write(1, 0x19, t);
    t += 10_000;
    assert_eq!(b.read(1, t) & 0xC0, 0x40);
    params(&mut b, 0x1000, (2 << 11) | 0x7FC, t);
    b.write(1, 0x11, t);
    t += 200_000;
    assert_eq!(b.read(1, t) & 0xC0, 0x40);
}

/// A Bubble Fix 2 backup of one module: 64-byte header, 2048 pages of `psize` bytes, status
/// bytes, tail with the page shift.
fn bubfix_file(module: u8, psize: usize, found: u8, shift: u16, fill: impl Fn(usize) -> u8) -> Vec<u8> {
    let mut d = b"BUBBACK1".to_vec();
    d.extend([module, if psize == 68 { b'R' } else { b'C' }, psize as u8, found, 0x00, 0x08, 0, 0, 0]);
    d.resize(64, 0);
    for p in 0..2048 {
        let start = d.len();
        d.extend((0..64).map(|i| fill(p * 64 + i)));
        d.resize(start + psize, 0xEE); // raw check bytes
    }
    d.extend((0..2048).map(|p| if p == 7 { 0 } else { 0x40 }));
    d.extend(b"BUBTAIL1");
    d.extend([0, 8, 0, 0, 0, 0]);
    d.extend(shift.to_le_bytes());
    d
}

#[test]
fn bubble_fix_backups_load_as_one_image() {
    let dir = std::env::temp_dir().join(format!("compass-bubfix-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pat = |m: usize| move |i: usize| (i / 64 * 3 + i + m) as u8;
    // module 1 raw (68-byte pages, found by Initialize), module 3 corrected, found from its own
    // map with the pages shifted by 5
    std::fs::write(dir.join("Bubble Module 1~Raw~"), bubfix_file(0, 68, 1, 0xFFFF, pat(0))).unwrap();
    std::fs::write(dir.join("Bubble Module 3~Bin~"), bubfix_file(2, 64, 2, 5, pat(2))).unwrap();
    // only one module named: its siblings with the same file type are found
    let l = grid_core::bubble::load_image(&dir.join("Bubble Module 1~Raw~"), 3 * 128 * 1024).unwrap();
    assert_eq!(l.image.len(), 3 * 128 * 1024);
    assert_eq!(l.save_to, dir.join("Bubble bubble.img"));
    assert!(l.note.contains("module 1 of 3"), "{}", l.note);
    assert!((0..128 * 1024).filter(|i| i / 64 != 7).all(|i| l.image[i] == pat(0)(i)));
    assert!(l.image[7 * 64..8 * 64].iter().all(|&v| v == 0), "a page not read stays blank");
    assert!(l.image[128 * 1024..256 * 1024].iter().all(|&v| v == 0));
    // the ~Bin~ name of module 3 finds no ~Raw~ siblings except by its own name
    let l3 = grid_core::bubble::load_image(&dir.join("Bubble Module 3~Bin~"), 0).unwrap();
    let m3 = &l3.image[2 * 128 * 1024..];
    for p in [0usize, 100, 2047] {
        if p == 7 {
            continue;
        }
        let q = (p + 5) % 2048;
        assert_eq!(&m3[q * 64..q * 64 + 64], &(0..64).map(|i| pat(2)(p * 64 + i)).collect::<Vec<u8>>()[..]);
    }
    // a version 1 copy (pages + status bytes) and a raw image load too
    let mut v1: Vec<u8> = (0..128 * 1024).map(|i| (i % 251) as u8).collect();
    v1.extend([0x40; 2048]);
    std::fs::write(dir.join("Old Module 2"), &v1).unwrap();
    let l = grid_core::bubble::load_image(&dir.join("Old Module 2"), 0).unwrap();
    assert_eq!(&l.image[128 * 1024..], &v1[..128 * 1024]);
    let raw = vec![0x55u8; 2 * 128 * 1024];
    std::fs::write(dir.join("raw.img"), &raw).unwrap();
    let l = grid_core::bubble::load_image(&dir.join("raw.img"), 0).unwrap();
    assert!(l.image == raw && l.note.is_empty() && l.save_to == dir.join("raw.img"));
    let l = grid_core::bubble::load_image(&dir.join("new.img"), 3 * 128 * 1024).unwrap();
    assert_eq!(l.image.len(), 3 * 128 * 1024);
    // once saved, the raw image next to the backups is used
    std::fs::write(dir.join("Bubble bubble.img"), &raw).unwrap();
    let l = grid_core::bubble::load_image(&dir.join("Bubble Module 1~Raw~"), 0).unwrap();
    assert!(l.image == raw && l.note.contains("using Bubble bubble.img"), "{}", l.note);
    let _ = std::fs::remove_dir_all(&dir);
}
