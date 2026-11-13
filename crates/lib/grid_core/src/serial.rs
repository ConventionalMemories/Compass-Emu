//! Intel 8274 multi-protocol serial controller, asynchronous mode (what GRiDTerm, PhoneLink,
//! printers and Xmodem use). Channel A is the RS-232 port; channel B is wired up but goes
//! nowhere. Registers: DFF00 data A, DFF02 data B, DFF04 control A, DFF06 control B (mirrored
//! up to DFF1F). The INT output goes to IR0 (non-vectored; programs read RR2B for the cause
//! when "status affects vector" is set).
//!
//! Bit timing: the transmit and receive clocks come from the 80130's baud timer
//! (CPU clock / count), divided by the 8274's clock mode (x1, x16, x32, x64). Each character
//! takes start + data + parity + stop bits. Host side: bytes the Compass sends appear in
//! `tx_out` (with their clock), bytes for the Compass go into `rx_in` and are delivered one
//! character time apart.
//!
//! SDLC (GRiDLink, WR4 = 20h): channel A sends and receives whole frames. A frame the
//! Compass sends is collected byte by byte and appears in `sdlc_out` once the 8274 has
//! appended the CRC and the closing flag (transmit underrun with the EOM latch reset by
//! WR0 = C0h). Frames for the Compass go into `sdlc_in` (address, control, data; no CRC);
//! they are delivered one byte time apart (x1 clock: 8 baud clocks per byte, bit stuffing
//! not counted), with address search against WR6 (and FFh). The receive FIFO gets the frame
//! bytes, the two CRC bytes (End of Frame in RR1 shows when the second one is at the top of
//! the FIFO and stays until Error Reset) and one more byte after them: the GRiDLink driver's
//! ShortRead waits for that fifth byte of a 4-byte frame before it reads RR1, so the chip
//! must leave something there (an inference from the driver, not from a data sheet).
//! Special receive condition interrupts are not generated.

use std::collections::VecDeque;

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Channel {
    pub wr: [u8; 8],
    ptr: usize,
    rx_fifo: VecDeque<u8>,
    rx_overrun: bool,
    /// Transmit buffer (one character) and shift register busy-until.
    tx_buf: Option<u8>,
    tx_buf_time: u64,
    tx_shift_until: u64,
    tx_int_pending: bool,
    rx_int_pending: bool,
    ext_int_pending: bool,
    rx_first: bool,
    /// Modem input lines.
    pub dcd: bool,
    pub cts: bool,
    last_ext: (bool, bool),
    /// Receive FIFO status per byte (SDLC): 80h End of Frame.
    rx_stat: VecDeque<u8>,
    /// SDLC: bytes of the frame being sent.
    tx_frame: Vec<u8>,
    /// SDLC: Tx Underrun/EOM latch (RR0 bit 6); WR0 = C0h resets it.
    eom_latch: bool,
    /// SDLC: End of Frame seen (RR1 bit 7) until Error Reset.
    eof_latch: bool,
    /// SDLC: the receiver is taking the bytes of a frame (address matched).
    rx_in_frame: bool,
}

impl Channel {
    fn reset(&mut self) {
        let (dcd, cts) = (self.dcd, self.cts);
        *self = Channel::default();
        self.dcd = dcd;
        self.cts = cts;
        self.last_ext = (dcd, cts);
        self.eom_latch = true;
    }

    /// SDLC mode: sync modes (WR4 bits 2-3 = 0) with the SDLC/HDLC format (bits 4-5 = 10).
    pub fn sdlc(&self) -> bool {
        self.wr[4] & 0x3C == 0x20
    }

    fn push_rx(&mut self, b: u8, stat: u8) {
        if self.rx_fifo.len() >= 4 {
            // three FIFO bytes plus the one held in the shift register
            self.rx_overrun = true;
            return;
        }
        self.rx_fifo.push_back(b);
        self.rx_stat.push_back(stat);
        let mode = (self.wr[1] >> 3) & 3;
        if mode == 2 || mode == 3 || (mode == 1 && self.rx_first) {
            self.rx_int_pending = true;
            self.rx_first = false;
        }
    }

    fn pop_rx(&mut self) -> u8 {
        let v = self.rx_fifo.pop_front().unwrap_or(0);
        if self.rx_stat.pop_front().unwrap_or(0) & 0x80 != 0 {
            self.eof_latch = true;
        }
        v
    }

    fn data_bits(bits: u8) -> u32 {
        match bits & 3 {
            0 => 5,
            1 => 7,
            2 => 6,
            _ => 8,
        }
    }

    fn rx_enabled(&self) -> bool {
        self.wr[3] & 1 != 0
    }
    fn tx_enabled(&self) -> bool {
        self.wr[5] & 8 != 0
    }
    pub fn dtr(&self) -> bool {
        self.wr[5] & 0x80 != 0
    }
    pub fn rts(&self) -> bool {
        self.wr[5] & 0x02 != 0
    }

    /// Bits per character on the line (start, data, parity, stop).
    fn frame_bits(&self) -> u32 {
        let data = Self::data_bits(self.wr[5] >> 5);
        let parity = (self.wr[4] & 1) as u32;
        let stop = match (self.wr[4] >> 2) & 3 {
            3 => 2,
            _ => 1,
        };
        1 + data + parity + stop
    }

    fn clock_mode(&self) -> u64 {
        match self.wr[4] >> 6 {
            0 => 1,
            1 => 16,
            2 => 32,
            _ => 64,
        }
    }

    fn data_mask(&self) -> u8 {
        let bits = Self::data_bits(self.wr[3] >> 6);
        ((1u16 << bits) - 1) as u8
    }

    fn int_pending(&self) -> bool {
        let rx_mode = (self.wr[1] >> 3) & 3;
        (self.tx_int_pending && self.wr[1] & 2 != 0)
            || (self.rx_int_pending && rx_mode != 0)
            || (self.ext_int_pending && self.wr[1] & 1 != 0)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Serial8274 {
    pub ch: [Channel; 2],
    /// Bytes channel A sent: (CPU clock when the last bit left, byte).
    pub tx_out: VecDeque<(u64, u8)>,
    /// Bytes waiting to be received on channel A.
    pub rx_in: VecDeque<u8>,
    next_rx: u64,
    /// 80130 baud timer count (CPU clocks per baud clock), set by the board.
    pub baud_count: u32,
    int_line: bool,
    pub log: bool,
    /// SDLC frames channel A sent: (CPU clock when the closing flag ended, address +
    /// control + data, without CRC).
    pub sdlc_out: VecDeque<(u64, Vec<u8>)>,
    /// SDLC frames waiting to be received on channel A (address + control + data).
    pub sdlc_in: VecDeque<Vec<u8>>,
    /// SDLC bytes on their way in: (clock, byte, flags 1 = first of frame, 80h = EOF,
    /// 2 = the byte after the CRC).
    rx_line: VecDeque<(u64, u8, u8)>,
    rx_line_free: u64,
    /// Cable loops channel A's RTS back to CTS (GRiDLink point-to-point cable).
    pub cts_loop: bool,
}

impl Default for Serial8274 {
    fn default() -> Self {
        Self::new()
    }
}

impl Serial8274 {
    pub fn new() -> Serial8274 {
        Serial8274 {
            ch: [Channel::default(), Channel::default()],
            tx_out: VecDeque::new(),
            rx_in: VecDeque::new(),
            next_rx: 0,
            baud_count: 33,
            int_line: false,
            log: false,
            sdlc_out: VecDeque::new(),
            sdlc_in: VecDeque::new(),
            rx_line: VecDeque::new(),
            rx_line_free: 0,
            cts_loop: false,
        }
    }

    /// CPU clocks per character on channel `c`.
    pub fn char_clocks(&self, c: usize) -> u64 {
        let ch = &self.ch[c];
        let bits = if ch.sdlc() { 8 } else { ch.frame_bits() as u64 };
        (self.baud_count.max(1) as u64) * ch.clock_mode() * bits
    }

    /// Bits per second on channel A, from the baud timer and clock mode.
    pub fn bit_rate(&self) -> f64 {
        5_000_000.0 / (self.baud_count.max(1) as f64 * self.ch[0].clock_mode() as f64)
    }

    /// Earliest clock at which advance() has something to do.
    pub fn next_event(&self) -> u64 {
        let mut t = u64::MAX;
        for ch in &self.ch {
            if ch.tx_buf.is_some() {
                t = t.min(ch.tx_shift_until.max(ch.tx_buf_time));
            }
        }
        if !self.rx_in.is_empty() {
            t = t.min(self.next_rx);
        }
        let a = &self.ch[0];
        if a.sdlc() && a.tx_buf.is_none() && !a.tx_frame.is_empty() {
            t = t.min(a.tx_shift_until);
        }
        if let Some(&(at, _, _)) = self.rx_line.front() {
            t = t.min(at);
        }
        if !self.sdlc_in.is_empty() {
            t = 0;
        }
        t
    }

    pub fn int_line(&self) -> bool {
        self.int_line
    }

    fn update_int(&mut self) {
        self.int_line = self.ch[0].int_pending() || self.ch[1].int_pending();
    }

    /// Set channel A's DCD and CTS inputs (host connected or not).
    pub fn set_modem_lines(&mut self, dcd: bool, cts: bool) {
        let ch = &mut self.ch[0];
        ch.dcd = dcd;
        ch.cts = cts;
        if (dcd, cts) != ch.last_ext {
            ch.last_ext = (dcd, cts);
            ch.ext_int_pending = true;
        }
        self.update_int();
    }

    /// Run up to CPU clock `now`: finish transmissions, deliver received characters.
    pub fn advance(&mut self, now: u64) {
        for c in 0..2 {
            let cc = self.char_clocks(c);
            let ch = &mut self.ch[c];
            let sdlc = ch.sdlc();
            // Transmit buffer -> shift register when the shifter is free.
            while ch.tx_buf.is_some() && ch.tx_shift_until.max(ch.tx_buf_time) <= now && ch.tx_enabled() {
                if sdlc && !ch.tx_frame.is_empty() && ch.tx_buf_time > ch.tx_shift_until {
                    // the shifter ran dry before this byte came: the frame ended there
                    let at = ch.tx_shift_until;
                    Self::underrun(ch, c, at, cc, &mut self.sdlc_out, self.log);
                    continue;
                }
                let start = ch.tx_shift_until.max(ch.tx_buf_time);
                let b = ch.tx_buf.take().unwrap();
                ch.tx_shift_until = start + cc;
                if sdlc {
                    ch.tx_frame.push(b);
                }
                else if c == 0 {
                    self.tx_out.push_back((ch.tx_shift_until, b & ch_mask_tx(ch)));
                }
                ch.tx_int_pending = true;
            }
            if sdlc && ch.tx_buf.is_none() && !ch.tx_frame.is_empty() && ch.tx_shift_until <= now {
                let at = ch.tx_shift_until;
                Self::underrun(ch, c, at, cc, &mut self.sdlc_out, self.log);
            }
        }
        self.advance_sdlc_rx(now);
        // Receive on channel A, one character time apart.
        let cc = self.char_clocks(0);
        while !self.rx_in.is_empty() && self.next_rx <= now {
            let ch = &mut self.ch[0];
            let b = self.rx_in.pop_front().unwrap();
            if ch.rx_enabled() {
                if ch.rx_fifo.len() >= 3 {
                    ch.rx_overrun = true;
                    ch.rx_fifo.pop_back();
                    ch.rx_stat.pop_back();
                }
                let m = ch.data_mask();
                ch.rx_fifo.push_back(b & m);
                ch.rx_stat.push_back(0);
                let mode = (ch.wr[1] >> 3) & 3;
                if mode == 2 || mode == 3 || (mode == 1 && ch.rx_first) {
                    ch.rx_int_pending = true;
                    ch.rx_first = false;
                }
            }
            self.next_rx = self.next_rx.max(now.saturating_sub(cc)) + cc;
        }
        if self.rx_in.is_empty() && self.next_rx < now {
            self.next_rx = now;
        }
        self.update_int();
    }

    /// Read: `reg` 0 data A, 1 data B, 2 control A, 3 control B.
    pub fn read(&mut self, reg: usize, now: u64) -> u8 {
        self.advance(now);
        let c = reg & 1;
        let v = if reg < 2 {
            let ch = &mut self.ch[c];
            let v = ch.pop_rx();
            if ch.rx_fifo.is_empty() {
                ch.rx_int_pending = false;
            }
            v
        }
        else {
            let p = self.ch[c].ptr;
            self.ch[c].ptr = 0;
            match p {
                0 => {
                    let ch = &self.ch[c];
                    let mut v = 0u8;
                    if !ch.rx_fifo.is_empty() {
                        v |= 0x01;
                    }
                    if c == 0 && self.int_line {
                        v |= 0x02;
                    }
                    if ch.tx_buf.is_none() {
                        v |= 0x04;
                    }
                    if ch.dcd {
                        v |= 0x08;
                    }
                    let cts = if c == 0 && self.cts_loop { ch.rts() } else { ch.cts };
                    if cts {
                        v |= 0x20;
                    }
                    if ch.sdlc() {
                        if ch.rx_enabled() && !ch.rx_in_frame {
                            v |= 0x10; // hunt
                        }
                        if ch.eom_latch {
                            v |= 0x40;
                        }
                    }
                    v
                }
                1 => {
                    let ch = &self.ch[c];
                    let mut v = 0u8;
                    if ch.tx_buf.is_none() && ch.tx_shift_until <= now && ch.tx_frame.is_empty() {
                        v |= 0x01;
                    }
                    if ch.rx_overrun {
                        v |= 0x20;
                    }
                    if ch.sdlc() && (ch.eof_latch || ch.rx_stat.front().is_some_and(|s| s & 0x80 != 0)) {
                        v |= 0x80;
                    }
                    v
                }
                2 if c == 1 => self.vector(),
                _ => 0,
            }
        };
        self.update_int();
        v
    }

    /// RR2B: the vector from WR2B, with the cause in bits 2-4 (8086 mode, bits 0-2) when
    /// "status affects vector" (WR1B bit 2) is on.
    fn vector(&self) -> u8 {
        let base = self.ch[1].wr[2];
        if self.ch[1].wr[1] & 4 == 0 {
            return base;
        }
        // cause: 0 B tx, 1 B ext, 2 B rx, 3 B special, 4 A tx, 5 A ext, 6 A rx, 7 A special;
        // no interrupt pending reads as 7 (B... per data sheet: "no interrupt" = 111 on ch B).
        let a = &self.ch[0];
        let b = &self.ch[1];
        let rx = |c: &Channel| c.rx_int_pending && (c.wr[1] >> 3) & 3 != 0;
        let tx = |c: &Channel| c.tx_int_pending && c.wr[1] & 2 != 0;
        let ex = |c: &Channel| c.ext_int_pending && c.wr[1] & 1 != 0;
        let cause = if rx(a) {
            6
        }
        else if tx(a) {
            4
        }
        else if ex(a) {
            5
        }
        else if rx(b) {
            2
        }
        else if tx(b) {
            0
        }
        else if ex(b) {
            1
        }
        else {
            7
        };
        // WR2A bit 5: vector mode; 8086 mode puts the cause in bits 0-2, 8085 in bits 2-4.
        if self.ch[0].wr[2] & 0x20 != 0 {
            (base & !0x07) | cause
        }
        else {
            (base & !0x1C) | (cause << 2)
        }
    }

    /// Write: `reg` 0 data A, 1 data B, 2 control A, 3 control B.
    pub fn write(&mut self, reg: usize, v: u8, now: u64) {
        self.advance(now);
        let c = reg & 1;
        if reg < 2 {
            // The byte reaches the shift register on a later transmit clock edge, not in the
            // same instant: INT drops for at least that long, so an edge-triggered IR0 sees
            // a new edge even when the shifter was already idle (a late interrupt routine
            // writing the next byte; GRiDSound's sample clock stopped without this gap).
            let gap = (self.baud_count.max(1) as u64).min(64);
            let ch = &mut self.ch[c];
            ch.tx_buf = Some(v);
            ch.tx_buf_time = now + gap;
            ch.tx_int_pending = false;
            if self.log {
                eprintln!("serial {} tx {:02X} at {}", if c == 0 { 'A' } else { 'B' }, v, now);
            }
            self.advance(now);
            return;
        }
        let ch = &mut self.ch[c];
        let p = ch.ptr;
        ch.ptr = 0;
        if p == 0 {
            ch.ptr = (v & 7) as usize;
            if v >> 6 == 3 {
                // reset Tx Underrun/EOM latch: the CRC goes out at the next underrun
                ch.eom_latch = false;
            }
            match (v >> 3) & 7 {
                2 => {
                    ch.ext_int_pending = false;
                }
                3 => {
                    ch.reset();
                }
                4 => {
                    ch.rx_first = true;
                }
                5 => {
                    ch.tx_int_pending = false;
                }
                6 => {
                    ch.rx_overrun = false;
                    ch.eof_latch = false;
                }
                _ => {}
            }
            ch.wr[0] = v;
        }
        else {
            ch.wr[p] = v;
            if p == 1 && (v >> 3) & 3 == 1 {
                ch.rx_first = true;
            }
            if p == 3 && v & 1 == 0 {
                // receiver off: a frame coming in is lost from here on
                ch.rx_in_frame = false;
            }
        }
        self.update_int();
    }
}

impl Serial8274 {
    /// SDLC transmit underrun at clock `at`: with the EOM latch reset and Tx CRC on, the 8274
    /// sends the CRC and the closing flag (the frame is complete 3 byte times later);
    /// otherwise the frame is cut off without CRC and no station takes it.
    fn underrun(ch: &mut Channel, c: usize, at: u64, cc: u64, out: &mut VecDeque<(u64, Vec<u8>)>, log: bool) {
        let frame = std::mem::take(&mut ch.tx_frame);
        if !ch.eom_latch && ch.wr[5] & 1 != 0 {
            ch.eom_latch = true;
            ch.tx_shift_until = at + 3 * cc;
            if log {
                eprintln!("sdlc {} tx frame {:02X?} at {}", if c == 0 { 'A' } else { 'B' }, frame, at);
            }
            if c == 0 {
                out.push_back((ch.tx_shift_until, frame));
            }
        }
        else {
            ch.eom_latch = true;
            if log {
                eprintln!("sdlc tx underrun without CRC: {} bytes dropped", frame.len());
            }
        }
    }

    /// SDLC receive on channel A: frames from `sdlc_in` go onto the line one byte time apart,
    /// then into the FIFO when the receiver takes them.
    fn advance_sdlc_rx(&mut self, now: u64) {
        let cc = self.char_clocks(0);
        if !self.ch[0].sdlc() {
            self.sdlc_in.clear();
            self.rx_line.clear();
            return;
        }
        while let Some(f) = self.sdlc_in.pop_front() {
            if f.is_empty() {
                continue;
            }
            // opening flag, then the bytes; a gap of one flag between frames
            let mut t = self.rx_line_free.max(now) + cc;
            let crc = crc_sdlc(&f);
            let n = f.len();
            for (i, &b) in f.iter().chain(crc.to_le_bytes().iter()).enumerate() {
                t += cc;
                let st = if i == 0 { 1 } else if i == n + 1 { 0x80 } else { 0 };
                self.rx_line.push_back((t, b, st));
            }
            // the closing flag shifts one more byte into the FIFO
            t += cc;
            self.rx_line.push_back((t, 0x7E, 2));
            self.rx_line_free = t;
        }
        while let Some(&(at, b, st)) = self.rx_line.front() {
            if at > now {
                break;
            }
            self.rx_line.pop_front();
            let ch = &mut self.ch[0];
            if st & 1 != 0 {
                let search = ch.wr[3] & 4 != 0;
                ch.rx_in_frame = ch.rx_enabled() && (!search || b == ch.wr[6] || b == 0xFF);
                if self.log {
                    eprintln!("sdlc A rx frame addr {:02X} at {} {}", b, at, if ch.rx_in_frame { "taken" } else { "ignored" });
                }
            }
            if !ch.rx_in_frame {
                continue;
            }
            ch.push_rx(b, st & 0x80);
            if st & 2 != 0 {
                ch.rx_in_frame = false;
            }
        }
    }
}

/// SDLC frame check sequence (CRC-CCITT, reflected, preset to ones, complemented); sent low
/// byte first.
pub fn crc_sdlc(data: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0x8408 } else { crc >> 1 };
        }
    }
    !crc
}

fn ch_mask_tx(ch: &Channel) -> u8 {
    let bits = Channel::data_bits(ch.wr[5] >> 5);
    ((1u16 << bits) - 1) as u8
}
