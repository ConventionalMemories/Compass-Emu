//! Low-level internal modem: the real 8749 firmware driven like GRiD-OS's Modem~Device~
//! (read reg = C0h+n, write reg = 80h+r then the value, commands with PC2 set), calling and
//! answering through the line model at 300 and 1200 baud.
//! Needs the modem ROM: MODEM_ROM=path (16020-007); skipped without it.

use grid_core::modem::{LineEvent, Modem};

const HZ: u64 = 5_000_000;

struct Host {
    m: Modem,
    t: u64,
}

impl Host {
    fn tick(&mut self, n: u64) {
        self.t += n;
        self.m.run_to(self.t);
    }

    fn secs(&mut self, s: f64) {
        let end = self.t + (s * HZ as f64) as u64;
        while self.t < end {
            self.tick(500);
        }
    }

    fn wait_c(&mut self, mask: u8, set: bool, what: &str) {
        let start = self.t;
        while (self.m.read(2) & mask != 0) != set {
            self.tick(50);
            assert!(self.t - start < HZ * 4, "timeout waiting for {what} at {:.3} s", self.t as f64 / HZ as f64);
        }
    }

    fn send(&mut self, b: u8, cmd: bool) {
        self.wait_c(0x80, true, "OBF");
        if cmd {
            self.m.write(3, 0x05);
        }
        self.m.write(0, b);
        self.wait_c(0x80, true, "OBF after byte");
        if cmd {
            self.m.write(3, 0x04);
        }
    }

    fn reg(&mut self, n: u8) -> u8 {
        self.send(0xC0 + n, true);
        self.wait_c(0x20, true, "IBF");
        self.m.read(0)
    }

    fn set(&mut self, r: u8, v: u8) {
        self.send(0x80 + r, true);
        self.send(v, false);
    }

    fn init(&mut self) {
        self.m.write(3, 0xC0);
        self.m.write(1, 0x7F);
        self.m.write(3, 0x03);
        self.m.write(3, 0x00);
        self.secs(0.05);
        self.set(6, 8);
        self.set(7, 0);
        self.set(8, 3);
    }

    fn wait_status(&mut self, bit: u8, secs: f64, what: &str) {
        let end = self.t + (secs * HZ as f64) as u64;
        loop {
            let st = self.reg(1);
            if st & bit != 0 {
                return;
            }
            assert!(self.t < end, "no {what}: status {st:02X}, line {}", self.m.io.line.status());
            self.secs(0.02);
        }
    }

    /// Send bytes through register 0 (waiting for TX empty, event bit 1), collect what the
    /// far end gets.
    fn transmit(&mut self, data: &[u8]) -> Vec<u8> {
        for &b in data {
            let start = self.t;
            while self.reg(2) & 0x02 == 0 {
                self.secs(0.002);
                assert!(self.t - start < HZ, "TX never empty");
            }
            self.set(0, b);
        }
        self.secs(0.3);
        self.m.io.line.tx_out.drain(..).collect()
    }

    /// Far end sends bytes; read them through register 0 when event bit 2 says one is there.
    fn receive(&mut self, data: &[u8]) -> Vec<u8> {
        self.m.io.line.rx_in.extend(data);
        let mut got = vec![];
        let end = self.t + HZ * 3;
        while got.len() < data.len() && self.t < end {
            if self.reg(2) & 0x04 != 0 {
                got.push(self.reg(0));
            }
            self.secs(0.001);
        }
        got
    }
}

fn host() -> Option<Host> {
    let p = std::env::var("MODEM_ROM").ok()?;
    let rom = std::fs::read(p).expect("modem ROM");
    Some(Host { m: Modem::new(rom), t: 0 })
}

fn dtmf(h: &mut Host, digits: &str) {
    for c in digits.chars() {
        let (r, col) = match c {
            '1'..='9' => {
                let d = c as u8 - b'1';
                (d / 3, d % 3)
            }
            '0' => (3, 1),
            _ => panic!(),
        };
        h.m.write(1, 0x80 | (!(1u8 << (r + 3)) & 0x78) | (!(1u8 << col) & 7));
        h.secs(0.08);
        h.m.write(1, 0x7F);
        h.secs(0.08);
    }
}

fn call(baud1200: bool) {
    let Some(mut h) = host() else {
        eprintln!("MODEM_ROM not set, skipped");
        return;
    };
    h.init();
    h.set(1, if baud1200 { 0xCA } else { 0xCE });
    h.send(1, true); // off-hook
    h.wait_status(0x04, 3.0, "dial tone");
    dtmf(&mut h, "5551234");
    h.send(3, true); // originate
    h.secs(0.01);
    assert_eq!(h.m.io.line.events.pop_front(), Some(LineEvent::Dial("5551234".into())));
    let now = h.t as f64;
    h.m.io.line.answered(now);
    h.wait_status(0x10, 10.0, "carrier");
    eprintln!("online at {:.2} s: {} P1 {:02X} P2 {:02X} cyc {}", h.t as f64 / HZ as f64, h.m.io.line.status(), h.m.mcu.p1, h.m.mcu.p2, h.m.mcu.cycles);
    assert!(h.m.io.line.status().contains(if baud1200 { "1200" } else { "300" }));
    h.secs(2.5);
    let msg = b"Hello, GRiD Server!\x00\xFF\x55";
    let sent = h.transmit(msg);
    assert_eq!(sent, msg, "Compass -> far end");
    let got = h.receive(msg);
    assert_eq!(got, msg, "far end -> Compass");
    // far end hangs up: carrier lost (status bit 7)
    h.m.io.line.remote_hangup();
    h.wait_status(0x80, 5.0, "carrier loss");
    h.send(0, true);
    h.secs(0.5);
    assert_eq!(h.m.io.line.events.pop_front(), Some(LineEvent::HangUp));
}

#[test]
fn originate_300() {
    call(false);
}

#[test]
fn originate_1200() {
    call(true);
}

fn answer(baud1200: bool) {
    let Some(mut h) = host() else {
        eprintln!("MODEM_ROM not set, skipped");
        return;
    };
    h.init();
    h.set(1, 0xCA);
    h.m.io.line.answer_1200 = baud1200;
    let now = h.t as f64;
    assert!(h.m.io.line.incoming_call(now));
    h.wait_status(0x08, 3.0, "ring");
    h.send(1, true); // off-hook answers
    h.send(4, true); // answer handshake
    h.secs(0.01);
    assert_eq!(h.m.io.line.events.pop_front(), Some(LineEvent::Answer));
    h.wait_status(0x10, 10.0, "carrier");
    eprintln!("online at {:.2} s: {} P1 {:02X} P2 {:02X} cyc {}", h.t as f64 / HZ as f64, h.m.io.line.status(), h.m.mcu.p1, h.m.mcu.p2, h.m.mcu.cycles);
    h.secs(2.5);
    let msg = b"answer test 123";
    assert_eq!(h.transmit(msg), msg);
    assert_eq!(h.receive(msg), msg);
}

#[test]
fn answer_300() {
    answer(false);
}

#[test]
fn answer_1200() {
    answer(true);
}
