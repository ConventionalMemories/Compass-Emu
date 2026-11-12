//! Low-level 2101: the real firmware (8088 on the box's board) answering a 9914 host driven
//! like GRiD's GPIB driver, with the S1410 behind it at command level.
//! Needs the 2101 ROM: HD2101_ROM=path (300205-05, the 20 MB unit's); skipped without it.

use grid_core::gpib::{Drive, DriveKind, Gpib};

const CPU_HZ: u64 = 5_016_000;
const PAGE: usize = 512;
const PAGES: usize = 41616;

struct Host {
    addr: u8,
    g: Gpib,
    t: u64,
    acc0: u8,
    acc1: u8,
}

impl Host {
    fn tick(&mut self, n: u64) {
        self.t += n;
        self.g.advance(self.t);
        self.acc0 |= self.g.read(0);
        self.acc1 |= self.g.read(1);
    }

    fn wait0(&mut self, mask: u8, what: &str) {
        let start = self.t;
        while self.acc0 & mask == 0 {
            self.tick(20);
            if self.t - start > CPU_HZ * 3 {
                panic!("timeout waiting for {what} at {:.3} s: {}", self.t as f64 / CPU_HZ as f64, self.g.drives[0].lle2101.as_ref().unwrap().debug());
            }
        }
        self.acc0 &= !mask;
    }

    fn aux(&mut self, v: u8) {
        self.g.write(3, v);
    }

    fn cmd_byte(&mut self, b: u8) {
        // GRiD's driver needs some 30 us between bytes (the 2102 needs about 12 us between
        // one handshake and holding NDAC for the next).
        self.tick(150);
        self.acc0 &= !0x10;
        self.g.write(7, b);
        self.wait0(0x10, &format!("BO after {b:02X}"));
    }

    fn reset(&mut self) {
        self.aux(0x80);
        self.aux(0x8F); // sic
        self.aux(0x00);
        self.tick(500);
        self.aux(0x0F);
        self.tick(500);
    }

    fn send(&mut self, data: &[u8]) {
        self.aux(0x0C); // take control (from standby) if needed
        self.aux(0x09); // listen off
        for b in [0x3F, 0x5F, 0x20 | self.addr] {
            self.cmd_byte(b);
        }
        self.aux(0x8A); // talk
        self.aux(0x0B); // gts
        for (i, &b) in data.iter().enumerate() {
            if i + 1 == data.len() {
                self.aux(0x08);
            }
            self.cmd_byte(b);
        }
        self.aux(0x0C);
        self.aux(0x0A);
        self.cmd_byte(0x3F);
    }

    /// Talk-address the box and read bytes until END.
    fn receive(&mut self, cmds: &[u8], max: usize) -> Vec<u8> {
        self.aux(0x0C);
        self.aux(0x0A);
        for &b in cmds {
            self.cmd_byte(b);
        }
        self.aux(0x89); // listen
        self.aux(0x0B);
        let mut v = Vec::new();
        loop {
            self.wait0(0x20, "BI");
            let end = self.acc0 & 0x08 != 0;
            self.acc0 &= !0x08;
            v.push(self.g.read(7));
            if end || v.len() == max {
                break;
            }
        }
        self.aux(0x0C);
        self.aux(0x09);
        v
    }

        fn request(&mut self, cmd: u8, page: u32, count: u16, ctrl: u8) {
        let p = page.to_le_bytes();
        let c = count.to_le_bytes();
        self.send(&[cmd, 0, 0, p[0], p[1], p[2], p[3], c[0], c[1], ctrl]);
    }

    fn wait_srq(&mut self) {
        let start = self.t;
        while self.g.read(3) & 0x04 == 0 {
            self.tick(200);
            assert!(self.t - start < CPU_HZ * 150, "no SRQ: {}", self.g.drives[0].lle2101.as_ref().unwrap().debug());
        }
        let s = self.receive(&[0x3F, 0x18, 0x40 | self.addr], 1);
        self.cmd_byte(0x19);
        self.cmd_byte(0x5F);
        assert_eq!(s[0] & 0x40, 0x40, "serial poll byte {:02X}", s[0]);
    }

    /// Wait for the box's service request, then talk-address it and read its reply.
    fn reply(&mut self) -> Vec<u8> {
        self.wait_srq();
        self.reply_now()
    }

    /// Read the reply at once (get status answers without a service request).
    fn reply_now(&mut self) -> Vec<u8> {
        let v = self.receive(&[0x3F, 0x40 | self.addr], usize::MAX);
        self.cmd_byte(0x5F);
        v
    }

    fn ms(&self, t0: u64) -> f64 {
        (self.t - t0) as f64 / CPU_HZ as f64 * 1000.0
    }
}

fn host() -> Option<Host> {
    host_with(true)
}

fn fd_ok(d: &[u8], p: usize) -> bool {
    d.iter().enumerate().all(|(i, &b)| b == (p * 5 + i + 1) as u8)
}

fn host_with(floppy: bool) -> Option<Host> {
    let rom = std::fs::read(std::env::var("HD2101_ROM").ok()?).ok()?;
    let mut img = vec![0u8; PAGES * PAGE];
    for (i, b) in img.iter_mut().enumerate() {
        let p = i / PAGE;
        *b = (p * 7 + (i % PAGE) * 3 + (p >> 8)) as u8;
    }
    let mut g = Gpib::new();
    g.rom_2101[0] = Some(rom);
    g.drives.push(Drive::new(DriveKind::Hdd2101, img));
    if floppy {
        let f: Vec<u8> = (0..720 * PAGE).map(|i| ((i / PAGE) * 5 + i % PAGE + 1) as u8).collect();
        g.drives.push(Drive::new(DriveKind::Floppy2101, f));
    }
    g.add_2101_floppy_slot();
    let mut h = Host { addr: 4, g, t: 0, acc0: 0, acc1: 0 };
    h.tick(CPU_HZ / 10);
    eprintln!("box after power on: {} log {:?}", h.g.drives[0].lle2101.as_ref().unwrap().debug(), h.g.drives[0].lle2101.as_ref().unwrap().log);
    h.reset();
    Some(h)
}

fn page_ok(d: &[u8], p: usize) -> bool {
    d.iter().enumerate().all(|(i, &b)| b == (p * 7 + i * 3 + (p >> 8)) as u8)
}

#[test]
fn lle_2101_hd() {
    let Some(mut h) = host()
    else {
        eprintln!("HD2101_ROM not set: skipped");
        return;
    };
    let t0 = h.t;
    h.request(1, 0, 52, 0);
    let st = h.reply_now();
    eprintln!("status {:.1} ms: {} bytes {:02X?}", h.ms(t0), st.len(), st);
    assert_eq!(st.len(), 52);
    assert_eq!(u16::from_le_bytes([st[4], st[5]]) as usize, PAGES);

    for page in [0u32, 1, 2, 3, 17, 68, 1000, 41615, 41615, 5, 6] {
        let t0 = h.t;
        h.request(4, page, 512, 0);
        let d = h.reply();
        eprintln!("read page {page}: {:.1} ms, {} bytes", h.ms(t0), d.len());
        assert_eq!(d.len(), 512, "page {page}: {:02X?}", &d[..d.len().min(16)]);
        assert!(page_ok(&d, page as usize), "page {page} data");
    }

    // Write page 300 and read it back.
    let t0 = h.t;
    h.request(5, 300, 512, 0);
    let data: Vec<u8> = (0..512).map(|i| (i * 11 + 5) as u8).collect();
    h.send(&data);
    let r = h.reply();
    eprintln!("write {:.1} ms reply {:02X?}", h.ms(t0), r);
    assert_eq!(r.len(), 7);
    assert_eq!(r[0], 0, "write error {:02X?}", r);
    assert_eq!(&h.g.drives[0].image[300 * PAGE..301 * PAGE], &data[..]);
    h.request(4, 300, 512, 0);
    assert_eq!(h.reply(), data);

    // Past the end: invalid page.
    h.request(4, PAGES as u32, 512, 0);
    let r = h.reply();
    eprintln!("bad page reply {:02X?}", r);
    assert_eq!(r.len(), 7);
    assert_ne!(r[0], 0);
}

#[test]
fn lle_2101_floppy() {
    let Some(mut h) = host()
    else {
        eprintln!("HD2101_ROM not set: skipped");
        return;
    };
    h.addr = 5;
    let t0 = h.t;
    h.request(1, 0, 52, 0);
    let st = h.reply_now();
    eprintln!("floppy status {:.1} ms: {:02X?}", h.ms(t0), st);
    assert_eq!(st.len(), 52);
    for page in [0u32, 1, 9, 17, 18, 100, 719, 2] {
        let t0 = h.t;
        h.request(4, page, 512, 0);
        let d = h.reply();
        eprintln!("floppy read {page}: {:.1} ms {} bytes", h.ms(t0), d.len());
        assert_eq!(d.len(), 512, "{:02X?}", &d[..d.len().min(8)]);
        assert!(fd_ok(&d, page as usize), "floppy page {page}");
    }
    let t0 = h.t;
    h.request(5, 40, 512, 0);
    let data: Vec<u8> = (0..512).map(|i| (i * 13 + 2) as u8).collect();
    h.send(&data);
    let r = h.reply();
    eprintln!("floppy write {:.1} ms reply {:02X?}", h.ms(t0), r);
    assert_eq!(r[0], 0);
    assert_eq!(&h.g.drives[1].image[40 * PAGE..41 * PAGE], &data[..]);
    h.request(4, 40, 512, 0);
    assert_eq!(h.reply(), data);
    // The hard disk still answers in between.
    h.addr = 4;
    h.request(4, 77, 512, 0);
    assert!(page_ok(&h.reply(), 77));
    // Whole-disk format (2101 format: its own interleave), then read back.
    h.addr = 5;
    let t0 = h.t;
    h.request(0x11, 0, 0, 0);
    let r = h.reply();
    eprintln!("floppy format {:.1} s reply {:02X?}", h.ms(t0) / 1000.0, r);
    assert_eq!(r[0], 0, "{:02X?}", r);
    {
        let b = h.g.drives[0].lle2101.as_ref().unwrap();
        let fd = &h.g.drives[1].image;
        for (c, hd) in [(0, 0), (0, 1), (1, 0), (1, 1), (2, 0)] {
            eprintln!("layout cyl {c} head {hd}: {:?}", b.floppy_layout(fd, c, hd));
        }
    }
    for page in [0u32, 10, 719] {
        let t0 = h.t;
        h.request(4, page, 512, 0);
        let d = h.reply();
        eprintln!("read {page} after format: {:.1} ms, first bytes {:02X?}", h.ms(t0), &d[..8]);
        assert_eq!(d.len(), 512);
    }
}

#[test]
fn lle_2101_empty_floppy_slot() {
    let Some(mut h) = host_with(false)
    else {
        eprintln!("HD2101_ROM not set: skipped");
        return;
    };
    h.addr = 5;
    let t0 = h.t;
    h.request(4, 0, 512, 0);
    let r = h.reply();
    eprintln!("empty slot read {:.1} ms reply {:02X?}", h.ms(t0), r);
    assert_eq!(r.len(), 7);
    assert_ne!(r[0], 0);
}
