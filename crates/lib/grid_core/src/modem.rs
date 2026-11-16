//! Compass internal modem, emulated at the chip level: the 8749 MCU runs GRiD's own firmware
//! (16020-007, 2 KB) behind the modem board's 8255 at DFEC0, and a model of the phone line
//! drives the MCU's pins. No audio is modelled: the analog parts (TMS99532 300 baud FSK modem,
//! the 1200 baud DPSK hybrids, ring and call-progress detectors, hook relay) are replaced by
//! what they put on the MCU's pins.
//!
//! Wiring (MCU ROM disassembly, modem-study/rom/MODEM-ROM-NOTES.md):
//!   DB0-7  8255 port A in mode 2: MOVX A,@R0 = RD = ACK (takes the host's byte, OBF# high),
//!          MOVX @R0,A = WR = STB (puts a byte for the host, IBF high)
//!   P1.0   8255 PC2 (the host's "this byte is a command" flag)
//!   P1.1   IRQ to the host (IR2 = GRiD interrupt 4, intModem), 1 = asserted
//!   P1.2   transmit data (1 = mark)          P1.3/P1.4  relays (commands 8/7), unknown use
//!   P1.5   1200 transmitter off (1)           P1.6  originate (1) / answer (0)
//!   P1.7   1200 (1) / 300 (0) select
//!   P2.0   8255 OBF# (0 = host byte waiting)  P2.1  1200 receive clock (sampled on fall)
//!   P2.2   1200 transmit clock (sent on rise) P2.3  call progress tone (dial tone 400 Hz)
//!   P2.4   1200 carrier, active low           P2.5  300 carrier (or answer tone), active low
//!   P2.6   ring detect, active low            P2.7  hook relay, 1 = on-hook
//!   INT    receive data (1 = mark), polled with JNI
//!   8255 PC1 = MCU reset (low = held in reset), PC0 = voice path, port B = MK5089 DTMF.
//! MCU clock 10.7386 MHz, one machine cycle = 15 clocks = 440/63 CPU clocks at 5 MHz.
//!
//! The 8749 is a software UART. At 300 baud it times bits itself; at 1200 baud it sends and
//! samples one bit per edge of the clocks on P2.1/P2.2 and scrambles in software (Bell 212A /
//! V.22 polynomial, taps 14 and 17). The line model is the far modem: it makes the same clocks,
//! scrambles what it sends and descrambles what the Compass sends, and frames bytes 8N1 for the
//! far computer (a Compass set to 7 bits + parity reaches it as 8-bit bytes, as with a real
//! terminal on 8N1).
//!
//! The far end of the line is a byte stream (a TCP socket in the frontend; see `Line`): the
//! Compass dials (DTMF on port B or pulses on the hook relay) and starts originate (command
//! 3), and the frontend connects to the number; an incoming connection rings the Compass,
//! and going off-hook answers it.

use std::collections::VecDeque;

use crate::upi41::{Upi41, UpiPorts};

const CPU_HZ: f64 = crate::screen::CPU_HZ;
/// CPU clocks per MCU machine cycle = CYC_NUM / CYC_DEN.
const CYC_NUM: u64 = 440;
const CYC_DEN: u64 = 63;

/// Seconds to CPU clocks.
fn s(sec: f64) -> f64 {
    sec * CPU_HZ
}

/// The modem board's 8255 (port A mode 2, port B and port C lower half outputs).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Ppi8255 {
    /// Port A output latch (host to MCU) and OBF (a byte waiting for the MCU).
    pub a_out: u8,
    pub obf: bool,
    /// Port A input latch (MCU to host) and IBF.
    pub a_in: u8,
    pub ibf: bool,
    pub b: u8,
    /// Port C as written (PC0-PC2 are outputs; PC4/PC6 hold INTE2/INTE1).
    pub c: u8,
}

impl Default for Ppi8255 {
    fn default() -> Self {
        Ppi8255 { a_out: 0, obf: false, a_in: 0, ibf: false, b: 0, c: 0 }
    }
}

impl Ppi8255 {
    pub fn read(&mut self, reg: usize) -> u8 {
        match reg {
            0 => {
                self.ibf = false;
                self.a_in
            }
            _ => self.peek(reg),
        }
    }

    pub fn peek(&self, reg: usize) -> u8 {
        match reg {
            0 => self.a_in,
            1 => self.b,
            2 => {
                let inte1 = self.c & 0x40 != 0;
                let inte2 = self.c & 0x10 != 0;
                let intr = (inte1 && !self.obf) || (inte2 && self.ibf);
                (if self.obf { 0 } else { 0x80 })
                    | (self.c & 0x50)
                    | if self.ibf { 0x20 } else { 0 }
                    | if intr { 0x08 } else { 0 }
                    | (self.c & 0x07)
            }
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, reg: usize, v: u8) {
        match reg {
            0 => {
                self.a_out = v;
                self.obf = true;
            }
            1 => self.b = v,
            2 => self.c = (self.c & !0x07) | (v & 0x07),
            _ => {
                if v & 0x80 != 0 {
                    // mode set: outputs cleared, handshakes reset
                    self.obf = false;
                    self.ibf = false;
                    self.b = 0;
                    self.c = 0;
                }
                else {
                    let bit = (v >> 1) & 7;
                    if v & 1 != 0 {
                        self.c |= 1 << bit;
                    }
                    else {
                        self.c &= !(1 << bit);
                    }
                }
            }
        }
    }
}

/// What the Compass did on the line, for the frontend.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LineEvent {
    /// Originate started after these digits were dialed (may be empty).
    Dial(String),
    /// The Compass went off-hook while the line was ringing.
    Answer,
    /// The Compass hung up (on-hook for more than 150 ms during a call).
    HangUp,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CallState {
    Idle,
    /// An incoming call rings the Compass (2 s on, 4 s off, 20 Hz while on).
    Ringing { since: f64 },
    /// The Compass dialed; waiting for the frontend to connect.
    Calling,
    /// The number didn't answer: silence until the Compass hangs up.
    Dead,
    /// Connected. `outgoing` = the Compass called.
    Up { since: f64, outgoing: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Far {
    /// Nothing from the far modem.
    Silent,
    /// The far modem answers with its 2225 Hz answer tone (the Compass originated).
    AnswerTone { since: f64 },
    /// The calling far modem hears the Compass's answer tone (and sends nothing yet).
    HeardTone { since: f64 },
    /// Carrier at 300 or 1200 baud.
    Carrier { baud1200: bool, since: f64 },
}

/// The phone line and the far modem.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Line {
    pub call: CallState,
    pub far: Far,
    /// Speed the far modem calls with when the Compass answers (true = 1200).
    pub answer_1200: bool,
    /// Events for the frontend.
    pub events: VecDeque<LineEvent>,
    /// Bytes the Compass sent (for the far end).
    pub tx_out: VecDeque<u8>,
    /// Bytes from the far end, sent to the Compass once the link is up.
    pub rx_in: VecDeque<u8>,
    /// Digits dialed since going off-hook.
    pub digits: String,
    // hook and pulse dialing
    hook_on: bool,
    hook_t: f64,
    hung: bool,
    pulses: u32,
    last_make: f64,
    dtmf_on: bool,
    // MCU pin state
    p1: u8,
    p2: u8,
    // far 300 baud transmitter (to INT): bits left in `tx300`, next bit change time
    tx300: u16,
    tx300_bits: u8,
    tx300_next: f64,
    level300: bool,
    // far 300 baud receiver (from P1.2)
    rx300_next: f64,
    rx300_bit: i8,
    rx300_data: u16,
    // 1200: clock edges counted from 0, scrambler and descrambler histories
    edge: u64,
    rx1200: bool,
    scr: u32,
    scr_ones: u32,
    tx1200: u16,
    tx1200_bits: u8,
    dscr: u32,
    rx1200_bit: i8,
    rx1200_data: u16,
}

impl Default for Line {
    fn default() -> Self {
        Line {
            call: CallState::Idle,
            far: Far::Silent,
            answer_1200: true,
            events: VecDeque::new(),
            tx_out: VecDeque::new(),
            rx_in: VecDeque::new(),
            digits: String::new(),
            hook_on: true,
            hook_t: 0.0,
            hung: true,
            pulses: 0,
            last_make: 0.0,
            dtmf_on: false,
            p1: 0xFF,
            p2: 0xFF,
            tx300: 0,
            tx300_bits: 0,
            tx300_next: 0.0,
            level300: true,
            rx300_next: 0.0,
            rx300_bit: -1,
            rx300_data: 0,
            edge: 0,
            rx1200: true,
            scr: 0,
            scr_ones: 0,
            tx1200: 0,
            tx1200_bits: 0,
            dscr: 0,
            rx1200_bit: -1,
            rx1200_data: 0,
        }
    }
}

const BIT300: f64 = CPU_HZ / 300.0;
const BIT1200: f64 = CPU_HZ / 1200.0;
/// Dial tone half period (400 Hz).
const DIAL_HALF: f64 = CPU_HZ / 800.0;

impl Line {
    // ---- frontend side ----

    /// A call comes in: ring if the line is free. Returns false when busy.
    pub fn incoming_call(&mut self, now: f64) -> bool {
        if self.call == CallState::Idle && self.hook_on {
            self.call = CallState::Ringing { since: now };
            true
        }
        else {
            false
        }
    }

    /// The number the Compass dialed answered.
    pub fn answered(&mut self, now: f64) {
        if self.call == CallState::Calling {
            self.call = CallState::Up { since: now, outgoing: true };
            self.far = Far::Silent;
        }
    }

    /// The number could not be reached.
    pub fn call_failed(&mut self) {
        if self.call == CallState::Calling {
            self.call = CallState::Dead;
        }
    }

    /// The far end hung up (or a ringing caller gave up).
    pub fn remote_hangup(&mut self) {
        if !matches!(self.call, CallState::Idle) {
            self.call = if self.hook_on { CallState::Idle } else { CallState::Dead };
            self.far = Far::Silent;
            self.rx_in.clear();
        }
    }

    pub fn connected(&self) -> bool {
        matches!(self.call, CallState::Up { .. })
    }

    pub fn off_hook(&self) -> bool {
        !self.hook_on
    }

    /// Text for status displays.
    pub fn status(&self) -> String {
        match (self.call, self.far) {
            (CallState::Idle, _) => if self.hook_on { "on-hook".into() } else { format!("off-hook {}", self.digits) },
            (CallState::Ringing { .. }, _) => "ringing".into(),
            (CallState::Calling, _) => format!("calling {}", self.digits),
            (CallState::Dead, _) => "no answer".into(),
            (CallState::Up { .. }, Far::Carrier { baud1200, .. }) => format!("connected {}", if baud1200 { 1200 } else { 300 }),
            (CallState::Up { .. }, _) => "connected (handshake)".into(),
        }
    }

    // ---- MCU pins ----

    fn ring_low(&self, now: f64) -> bool {
        if let CallState::Ringing { since } = self.call {
            let ph = (now - since).max(0.0) % s(6.0);
            ph < s(2.0) && ((ph / s(0.025)) as u64) % 2 == 0
        }
        else {
            false
        }
    }

    fn carrier(&self) -> Option<bool> {
        match self.far {
            Far::Silent | Far::HeardTone { .. } => None,
            Far::AnswerTone { .. } => Some(false),
            Far::Carrier { baud1200, .. } => Some(baud1200),
        }
    }

    /// Port 2 inputs (bit 0 comes from the 8255; bit 7 is the hook output).
    fn p2_inputs(&self, now: f64) -> u8 {
        let mut v = 0xFFu8;
        // 1200 clocks (both receive and transmit) run all the time
        let ph = now % BIT1200;
        if ph >= BIT1200 / 2.0 {
            v &= !0x06;
        }
        if !self.hook_on && matches!(self.call, CallState::Idle) && ((now / DIAL_HALF) as u64) % 2 == 1 {
            v &= !0x08;
        }
        match self.carrier() {
            Some(true) => v &= !0x10,
            Some(false) => v &= !0x20,
            None => {}
        }
        if self.ring_low(now) {
            v &= !0x40;
        }
        v
    }

    fn int_pin(&self) -> bool {
        match self.far {
            Far::Silent | Far::HeardTone { .. } | Far::AnswerTone { .. } => true,
            Far::Carrier { baud1200: false, .. } => self.level300,
            Far::Carrier { baud1200: true, .. } => self.rx1200,
        }
    }

    fn set_p1(&mut self, v: u8, now: f64) {
        let fell = self.p1 & 0x04 != 0 && v & 0x04 == 0;
        self.p1 = v;
        if fell && self.rx300_bit < 0 && matches!(self.far, Far::Carrier { baud1200: false, .. }) {
            // start bit: sample its middle, then each bit
            self.rx300_bit = 0;
            self.rx300_data = 0;
            self.rx300_next = now + BIT300 * 0.5;
        }
    }

    fn set_p2(&mut self, v: u8, now: f64) {
        self.p2 = v;
        let on = v & 0x80 != 0;
        if on != self.hook_on {
            if on {
                self.hook_on = true;
                self.hook_t = now;
            }
            else {
                // make: a short break was a dial pulse
                if !self.hung && now - self.hook_t < s(0.15) {
                    self.pulses += 1;
                    self.last_make = now;
                }
                self.hook_on = false;
                self.hook_t = now;
                if self.hung {
                    self.hung = false;
                    self.digits.clear();
                    self.pulses = 0;
                }
                if let CallState::Ringing { .. } = self.call {
                    self.call = CallState::Up { since: now, outgoing: false };
                    self.far = Far::Silent;
                    self.events.push_back(LineEvent::Answer);
                }
            }
        }
    }

    /// Port B write (MK5089 DTMF: bit 7 on, bits 0-2 column, 3-6 row, low = selected).
    pub fn dtmf(&mut self, b: u8) {
        let cols = !b & 7;
        let rows = (!b >> 3) & 15;
        let on = b & 0x80 != 0 && cols.count_ones() == 1 && rows.count_ones() == 1;
        if on && !self.dtmf_on && !self.hook_on {
            const KEYS: [[char; 3]; 4] = [['1', '2', '3'], ['4', '5', '6'], ['7', '8', '9'], ['*', '0', '#']];
            self.digits.push(KEYS[rows.trailing_zeros() as usize][cols.trailing_zeros() as usize]);
        }
        self.dtmf_on = on;
    }

    /// A command byte the host sent the MCU (seen on the 8255 with PC2 set).
    pub fn command(&mut self, cmd: u8) {
        if cmd == 3 && matches!(self.call, CallState::Idle) {
            self.call = CallState::Calling;
            self.events.push_back(LineEvent::Dial(self.digits.clone()));
        }
    }

    /// Next byte for the far transmitter, if the link is ready to carry data.
    fn next_far_byte(&mut self, now: f64) -> Option<u8> {
        match self.far {
            Far::Carrier { since, .. } if now - since > s(2.0) => self.rx_in.pop_front(),
            _ => None,
        }
    }

    /// Bring the line up to CPU clock `now` (called after every MCU instruction).
    fn advance(&mut self, now: f64) {
        // hook: hang-up and pulse digits
        if self.hook_on && !self.hung && now - self.hook_t > s(0.15) {
            self.hung = true;
            self.pulses = 0;
            self.digits.clear();
            match self.call {
                CallState::Up { .. } | CallState::Calling | CallState::Dead => {
                    self.call = CallState::Idle;
                    self.far = Far::Silent;
                    self.rx_in.clear();
                    self.events.push_back(LineEvent::HangUp);
                }
                _ => {}
            }
        }
        if !self.hook_on && self.pulses > 0 && now - self.last_make > s(0.3) {
            self.digits.push(char::from(b'0' + (self.pulses % 10) as u8));
            self.pulses = 0;
        }
        // far modem handshake
        if let CallState::Up { since, outgoing } = self.call {
            let (p1, off) = (self.p1, !self.hook_on);
            match self.far {
                Far::Silent if outgoing && off && now - since > s(1.0) => self.far = Far::AnswerTone { since: now },
                Far::Silent if !outgoing && off && p1 & 0xE0 == 0x20 => {
                    // the Compass's answer tone (answer mode, 300 path, 1200 TX off)
                    self.far = Far::HeardTone { since: now };
                }
                Far::AnswerTone { .. } => {
                    if p1 & 0x80 == 0 {
                        self.start_carrier(false, now);
                    }
                    else if p1 & 0x20 == 0 {
                        self.start_carrier(true, now);
                    }
                }
                Far::HeardTone { since: t0 } => {
                    if now - t0 > s(0.5) {
                        self.start_carrier(self.answer_1200, now);
                    }
                }
                _ => {}
            }
        }
        // far 300 transmitter
        if let Far::Carrier { baud1200: false, .. } = self.far {
            while now >= self.tx300_next {
                if self.tx300_bits == 0 {
                    match self.next_far_byte(now) {
                        Some(b) => {
                            self.tx300 = (b as u16) << 1 | 0x200;
                            self.tx300_bits = 10;
                        }
                        None => {
                            self.level300 = true;
                            self.tx300_next = now + BIT300 / 4.0;
                            break;
                        }
                    }
                }
                self.level300 = self.tx300 & 1 != 0;
                self.tx300 >>= 1;
                self.tx300_bits -= 1;
                self.tx300_next += BIT300;
            }
            // far 300 receiver
            while self.rx300_bit >= 0 && now >= self.rx300_next {
                let bit = self.p1 & 0x04 != 0;
                match self.rx300_bit {
                    0 => {
                        if bit {
                            self.rx300_bit = -1; // glitch, not a start bit
                            break;
                        }
                    }
                    1..=8 => self.rx300_data |= (bit as u16) << (self.rx300_bit - 1),
                    _ => {
                        if bit {
                            self.tx_out.push_back(self.rx300_data as u8);
                        }
                        self.rx300_bit = -1;
                        break;
                    }
                }
                self.rx300_bit += 1;
                self.rx300_next += BIT300;
            }
        }
        else {
            self.level300 = true;
            self.rx300_bit = -1;
        }
        // 1200 clock edges: rise at k * BIT1200, fall half a bit later
        loop {
            let half = (self.edge + 1) as f64 * BIT1200 / 2.0;
            if half > now {
                break;
            }
            self.edge += 1;
            let carrier1200 = matches!(self.far, Far::Carrier { baud1200: true, .. });
            if self.edge % 2 == 0 {
                // rising edge: the far modem puts its next bit on INT
                if carrier1200 {
                    let d = self.next_bit1200(half);
                    self.rx1200 = self.scramble(d);
                }
            }
            else if carrier1200 {
                // falling edge: take the Compass's bit
                let x = self.p1 & 0x04 != 0;
                let d = x ^ (self.dscr >> 13 & 1 != 0) ^ (self.dscr >> 16 & 1 != 0);
                self.dscr = (self.dscr << 1) | x as u32;
                self.deframe1200(d, half);
            }
        }
    }

    fn start_carrier(&mut self, baud1200: bool, now: f64) {
        self.far = Far::Carrier { baud1200, since: now };
        self.tx300_bits = 0;
        self.tx300_next = now;
        self.tx1200_bits = 0;
        self.rx1200_bit = -1;
        // Until now INT showed marks, so the Compass's descrambler history is all ones: start
        // the scrambler from the same history, so its first bits already descramble right.
        self.scr = !0;
        self.scr_ones = 0;
        self.rx1200 = true;
        self.rx300_bit = -1;
        self.level300 = true;
    }

    fn next_bit1200(&mut self, now: f64) -> bool {
        if self.tx1200_bits == 0 {
            match self.next_far_byte(now) {
                Some(b) => {
                    self.tx1200 = (b as u16) << 1 | 0x200;
                    self.tx1200_bits = 10;
                }
                None => return true,
            }
        }
        let bit = self.tx1200 & 1 != 0;
        self.tx1200 >>= 1;
        self.tx1200_bits -= 1;
        bit
    }

    /// The 8749's own scrambler (ROM 4F4): out = d ^ y[-14] ^ y[-17], and after 64 ones in a
    /// row the next one goes out inverted.
    fn scramble(&mut self, d: bool) -> bool {
        let mut out = d ^ (self.scr >> 13 & 1 != 0) ^ (self.scr >> 16 & 1 != 0);
        if out {
            if self.scr_ones >= 64 {
                out = false;
                self.scr_ones = 0;
            }
            else {
                self.scr_ones += 1;
            }
        }
        else {
            self.scr_ones = 0;
        }
        self.scr = (self.scr << 1) | out as u32;
        out
    }

    fn deframe1200(&mut self, d: bool, now: f64) {
        match self.rx1200_bit {
            -1 => {
                if !d {
                    self.rx1200_bit = 0;
                    self.rx1200_data = 0;
                }
            }
            0..=7 => {
                self.rx1200_data |= (d as u16) << self.rx1200_bit;
                self.rx1200_bit += 1;
            }
            _ => {
                // nothing counts until the descrambler has synchronised to the Compass's
                // scrambler (its first 17 bits are noise)
                let trained = matches!(self.far, Far::Carrier { since, .. } if now - since > s(1.0));
                if d && trained {
                    self.tx_out.push_back(self.rx1200_data as u8);
                }
                self.rx1200_bit = -1;
            }
        }
    }
}

/// Everything around the MCU: the 8255 handshake and the line.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ModemIo {
    pub ppi: Ppi8255,
    pub line: Line,
    /// CPU clock of the instruction being run (for the line model).
    now: f64,
}

impl UpiPorts for ModemIo {
    fn p1_in(&mut self, _p2: u8) -> u8 {
        0xFE | (self.ppi.c >> 2 & 1)
    }
    fn p2_in(&mut self) -> u8 {
        let v = self.line.p2_inputs(self.now);
        (v & !1) | if self.ppi.obf { 0 } else { 1 }
    }
    fn p1_out(&mut self, v: u8) {
        self.line.set_p1(v, self.now);
    }
    fn p2_out(&mut self, v: u8) {
        self.line.set_p2(v, self.now);
    }
    fn t0(&mut self) -> bool {
        true
    }
    fn t1(&mut self) -> bool {
        true
    }
    fn int_pin(&mut self) -> bool {
        self.line.int_pin()
    }
    fn bus_read(&mut self, _addr: u8) -> u8 {
        self.ppi.obf = false;
        self.ppi.a_out
    }
    fn bus_write(&mut self, _addr: u8, v: u8) {
        self.ppi.a_in = v;
        self.ppi.ibf = true;
    }
}

/// The modem board.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Modem {
    pub mcu: Upi41,
    pub io: ModemIo,
    /// MCU time in 1/63 CPU clocks.
    time63: u64,
    in_reset: bool,
}

impl Modem {
    pub fn new(rom: Vec<u8>) -> Modem {
        Modem {
            mcu: Upi41::new_8048(rom),
            io: ModemIo { ppi: Ppi8255::default(), line: Line::default(), now: 0.0 },
            time63: 0,
            in_reset: true,
        }
    }

    /// CPU clock the MCU has run up to.
    pub fn time(&self) -> u64 {
        self.time63 / CYC_DEN
    }

    /// Run the MCU and the line up to CPU clock `t`.
    pub fn run_to(&mut self, t: u64) {
        let end = t * CYC_DEN;
        // PC1 low holds the MCU in reset
        let reset = self.io.ppi.c & 0x02 == 0;
        if reset {
            if !self.in_reset {
                self.mcu.reset();
                self.io.line.set_p1(0xFF, self.time63 as f64 / CYC_DEN as f64);
                self.io.line.set_p2(0xFF, self.time63 as f64 / CYC_DEN as f64);
                self.in_reset = true;
            }
            self.time63 = self.time63.max(end);
            self.io.now = t as f64;
            self.io.line.advance(t as f64);
            return;
        }
        self.in_reset = false;
        while self.time63 + CYC_NUM <= end {
            self.io.now = self.time63 as f64 / CYC_DEN as f64;
            let c = self.mcu.step(&mut self.io);
            self.time63 += c as u64 * CYC_NUM;
            self.io.line.advance(self.time63 as f64 / CYC_DEN as f64);
        }
    }

    /// The IRQ line (P1.1).
    pub fn irq(&self) -> bool {
        !self.in_reset && self.mcu.p1 & 0x02 != 0
    }

    pub fn line(&mut self) -> &mut Line {
        &mut self.io.line
    }

    /// Host access to the 8255 (`reg` 0-3).
    pub fn read(&mut self, reg: usize) -> u8 {
        self.io.ppi.read(reg)
    }

    pub fn peek(&self, reg: usize) -> u8 {
        self.io.ppi.peek(reg)
    }

    pub fn write(&mut self, reg: usize, v: u8) {
        if reg == 0 && self.io.ppi.c & 0x04 != 0 {
            self.io.line.command(v);
        }
        self.io.ppi.write(reg, v);
        if reg == 1 || (reg == 3 && v & 0x80 != 0) {
            let b = self.io.ppi.b;
            self.io.line.dtmf(b);
        }
    }
}
