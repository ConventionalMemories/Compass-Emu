//! Low-level 2102: the real firmware answering a 9914 host driven like GRiD's GPIB driver.
//! Needs the 2102 ROM: FD2102_ROM=path (300237-01 or -02); skipped without it.

use grid_core::gpib::{Drive, Gpib};

const CPU_HZ: u64 = 5_016_000;
const PAGE: usize = 512;

struct Host {
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
                panic!("timeout waiting for {what} at {:.3} s: {}", self.t as f64 / CPU_HZ as f64, self.g.drives[0].lle.as_ref().unwrap().debug());
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
        for b in [0x3F, 0x5F, 0x26] {
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

    fn wait_srq(&mut self) {
        let start = self.t;
        while self.g.read(3) & 0x04 == 0 {
            self.tick(200);
            assert!(self.t - start < CPU_HZ * 150, "no SRQ: {}", self.g.drives[0].lle.as_ref().unwrap().debug());
        }
        let s = self.receive(&[0x3F, 0x18, 0x46], 1);
        self.cmd_byte(0x19);
        self.cmd_byte(0x5F);
        assert_eq!(s[0] & 0x40, 0x40, "serial poll byte {:02X}", s[0]);
    }

    fn request(&mut self, cmd: u8, page: u32, count: u16, ctrl: u8) -> f64 {
        let p = page.to_le_bytes();
        let c = count.to_le_bytes();
        let t0 = self.t;
        self.send(&[cmd, 0, 0, p[0], p[1], p[2], p[3], c[0], c[1], ctrl]);
        // Get status answers at once, and a write takes its data at once: no service request.
        if cmd != 1 && cmd != 5 {
            self.wait_srq();
        }
        (self.t - t0) as f64 / CPU_HZ as f64 * 1000.0
    }

    fn reply(&mut self) -> Vec<u8> {
        let v = self.receive(&[0x3F, 0x46], usize::MAX);
        self.cmd_byte(0x5F);
        v
    }
}

fn host() -> Option<Host> {
    let rom = std::fs::read(std::env::var("FD2102_ROM").ok()?).ok()?;
    let mut img = vec![0u8; 720 * PAGE];
    for p in 0..720 {
        for i in 0..PAGE {
            img[p * PAGE + i] = (p * 3 + i) as u8;
        }
    }
    let mut g = Gpib::new();
    g.rom_2102 = Some(rom);
    g.drives.push(Drive::new(grid_core::gpib::DriveKind::Floppy2102, img));
    let mut h = Host { g, t: 0, acc0: 0, acc1: 0 };
    // The box's self test and drive probe take a while after power-on (the box starts 4 s
    // ahead, as if switched on first, unless GRID_2102_EARLY says otherwise).
    h.tick(CPU_HZ / 10);
    h.reset();
    Some(h)
}

#[test]
fn lle_2102_read_write_status() {
    let Some(mut h) = host()
    else {
        eprintln!("FD2102_ROM not set: skipped");
        return;
    };
    let ms = h.request(1, 0, 52, 0);
    let st = h.reply();
    eprintln!("status {:.1} ms: {} bytes {:02X?}", ms, st.len(), &st[..st.len().min(16)]);
    assert_eq!(st.len(), 52);

    for page in [0u32, 1, 9, 17, 25, 100, 719, 719, 719, 719, 5, 6, 7, 8] {
        let ms = h.request(4, page, 512, 0);
        let d = h.reply();
        eprintln!("read page {page}: {:.1} ms, {} bytes", ms, d.len());
        assert_eq!(d.len(), 512, "page {page}: {:02X?}", d);
        assert!(d.iter().enumerate().all(|(i, &b)| b == (page as usize * 3 + i) as u8), "page {page} data");
    }

    // Write page 30 and read it back.
    let ms = h.request(5, 30, 512, 0);
    eprintln!("write cmd {ms:.1} ms");
    let data: Vec<u8> = (0..512).map(|i| (i * 7 + 1) as u8).collect();
    h.send(&data);
    h.wait_srq();
    let r = h.reply();
    eprintln!("write reply {:02X?}", r);
    assert_eq!(r.len(), 7);
    assert_eq!(r[0], 0, "write error {:02X?}", r);
    assert!(h.g.drives[0].dirty);
    assert_eq!(&h.g.drives[0].image[30 * PAGE..31 * PAGE], &data[..]);
    h.request(4, 30, 512, 0);
    assert_eq!(h.reply(), data);

    // Page 720 passes the firmware's range check (it refuses only 721 and up) and seeks to
    // cylinder 40, where there is no track: record not found (68h).
    h.request(4, 720, 512, 0);
    let r = h.reply();
    eprintln!("page 720 reply {:02X?}", r);
    assert_eq!(r[0], 0x68);
    // A saved state brings the box back as it was (the image is attached again).
    let img = h.g.drives[0].image.clone();
    let js = serde_json::to_string(&h.g).unwrap();
    h.g = serde_json::from_str(&js).unwrap();
    h.g.drives[0].image = img;
    h.request(4, 30, 512, 0);
    assert_eq!(h.reply(), data);
    // Page 721: invalid page (66h).
    h.request(4, 721, 512, 0);
    let r = h.reply();
    eprintln!("bad page reply {:02X?}", r);
    assert_eq!(r.len(), 7);
    assert_eq!(r[0], 0x66);
}

#[test]
fn lle_2102_format() {
    let Some(mut h) = host()
    else {
        eprintln!("FD2102_ROM not set: skipped");
        return;
    };
    // Format one track (cylinder of page 40 = cylinder 2, head 0), then the whole disk.
    let ms = h.request(0x16, 40, 0, 0);
    let r = h.reply();
    eprintln!("track format {ms:.0} ms reply {:02X?}", r);
    assert_eq!(r[0], 0, "{:02X?}", r);
    let d = &h.g.drives[0].image;
    assert!(d[36 * PAGE..36 * PAGE + 8].iter().all(|&b| b == 0xFF) && d[36 * PAGE + 8..37 * PAGE].iter().all(|&b| b == 0xE5));
    assert_eq!(d[45 * PAGE], (45 * 3) as u8, "head 1 of that cylinder untouched");
    h.request(4, 40, 512, 0);
    let p = h.reply();
    assert_eq!(p.len(), 512);
    assert_eq!(p[8], 0xE5);

    let ms = h.request(0x11, 0, 0, 0);
    let r = h.reply();
    eprintln!("disk format {:.1} s reply {:02X?}", ms / 1000.0, r);
    assert_eq!(r[0], 0, "{:02X?}", r);
    let d = &h.g.drives[0].image;
    for p in 0..720 {
        assert!(d[p * PAGE + 8..(p + 1) * PAGE].iter().all(|&b| b == 0xE5), "page {p}");
    }
    for page in [0u32, 13, 719] {
        let ms = h.request(4, page, 512, 0);
        let d = h.reply();
        eprintln!("read page {page} after format: {ms:.1} ms");
        assert_eq!(d.len(), 512);
    }
}
