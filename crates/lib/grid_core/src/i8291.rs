//! Intel 8291A GPIB talker/listener, at the level of the bus lines, as on the GRiD 2101's
//! disk controller board.
//!
//! Registers (RS2-0): read 0 data in, 1 interrupt status 1, 2 interrupt status 2, 3 serial
//! poll status, 4 address status, 5 command pass through, 6 address 0, 7 address 1; write 0
//! data out, 1 interrupt enable 1, 2 interrupt enable 2, 3 serial poll mode, 4 address mode,
//! 5 auxiliary mode, 6 address 0/1, 7 end of string.
//!
//! What the 2101 firmware uses is modelled in full: mode 1 addressing with a major (ADR0) and
//! a minor (ADR1) primary address, the acceptor and source handshakes, END on EOI, send EOI
//! (auxiliary 06h), DMA requests for data in and out, DCL/SDC, GET, serial polls and IFC.
//! Not modelled: extended addressing (modes 2 and 3), parallel poll, EOS detection and the
//! holdoff modes of auxiliary register A.

use serde::{Deserialize, Serialize};

use crate::box2102::{HostLines, Lines};

// Interrupt status 1
pub const BI: u8 = 0x01;
pub const BO: u8 = 0x02;
pub const ERR: u8 = 0x04;
pub const DEC: u8 = 0x08;
pub const END: u8 = 0x10;
pub const GET: u8 = 0x20;
pub const APT: u8 = 0x40;
pub const CPT: u8 = 0x80;
// Interrupt status 2
pub const ADSC: u8 = 0x01;
pub const SPAS: u8 = 0x40;
// Interrupt enable 2 DMA bits
const DMAI: u8 = 0x10;
const DMAO: u8 = 0x20;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Ah {
    /// Not taking part: drives neither NRFD nor NDAC.
    #[default]
    Idle,
    /// Ready for a byte: NDAC asserted, NRFD released (once a byte in DI has been read).
    Ready,
    /// Took the byte: NRFD and NDAC asserted for one handshake step.
    Accepting,
    /// Accepted: NRFD asserted, NDAC released until the source drops DAV.
    WaitDavOff,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Sh {
    /// Nothing to send.
    #[default]
    Idle,
    /// A byte is waiting for every acceptor to be ready (NRFD released).
    Wait,
    /// DAV asserted, waiting for every acceptor to take it (NDAC released).
    Dav,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct I8291 {
    pub ie1: u8,
    pub ie2: u8,
    pub isr1: u8,
    pub isr2: u8,
    pub spmr: u8,
    pub admr: u8,
    /// Address 0 and 1 registers: address in bits 4-0, DL bit 5, DT bit 6.
    pub adr: [u8; 2],
    pub eos: u8,
    pub aux_a: u8,
    pub aux_b: u8,
    /// Held in reset by auxiliary command 02h until 00h (immediate execute pon).
    pub reset: bool,
    di: u8,
    /// A byte is in DI and has not been read (data in DMA request).
    di_full: bool,
    /// The chip can take a byte to send (data out DMA request).
    bo_ready: bool,
    out: u8,
    out_eoi: bool,
    send_eoi: bool,
    pub la: bool,
    pub ta: bool,
    /// The last address recognised was the minor one.
    pub minor: bool,
    /// Serial poll mode (SPE received).
    spms: bool,
    ah: Ah,
    sh: Sh,
    /// The source handshake is sending the serial poll status byte.
    spas: bool,
    /// The status byte has gone out in this serial poll (sent once until ATN comes back: GRiD's
    /// driver takes ATN right after reading it, and the host model has no RFD holdoff).
    #[serde(default)]
    sp_sent: bool,
    /// Last command byte passed through (CPT).
    cpt: u8,
    host: HostLines,
}

impl Default for I8291 {
    fn default() -> Self {
        Self::new()
    }
}

impl I8291 {
    pub fn new() -> I8291 {
        I8291 {
            ie1: 0,
            ie2: 0,
            isr1: 0,
            isr2: 0,
            spmr: 0,
            admr: 0,
            adr: [0x60, 0x60],
            eos: 0,
            aux_a: 0,
            aux_b: 0,
            reset: true,
            di: 0,
            di_full: false,
            bo_ready: false,
            out: 0,
            out_eoi: false,
            send_eoi: false,
            la: false,
            ta: false,
            minor: false,
            spms: false,
            ah: Ah::Idle,
            sh: Sh::Idle,
            spas: false,
            sp_sent: false,
            cpt: 0,
            host: HostLines::default(),
        }
    }

    fn chip_reset(&mut self) {
        self.isr1 = 0;
        self.isr2 = 0;
        self.spmr = 0;
        self.di_full = false;
        self.bo_ready = false;
        self.send_eoi = false;
        self.la = false;
        self.ta = false;
        self.spms = false;
        self.spas = false;
        self.ah = Ah::Idle;
        self.sh = Sh::Idle;
        self.reset = true;
    }

    /// The INT pin.
    pub fn int(&self) -> bool {
        (self.isr1 & self.ie1) != 0 || (self.isr2 & self.ie2 & 0x0F) != 0
    }

    /// Data in DMA request (a byte waits in DI).
    pub fn dreq_in(&self) -> bool {
        self.ie2 & DMAI != 0 && self.di_full
    }

    /// Data out DMA request (the chip takes a byte to send).
    pub fn dreq_out(&self) -> bool {
        self.ie2 & DMAO != 0 && self.bo_ready
    }

    fn talker_active(&self) -> bool {
        self.ta && !self.host.atn && !self.reset
    }

    fn listener_active(&self) -> bool {
        !self.reset && (self.host.atn || self.la)
    }

    pub fn read(&mut self, reg: u8) -> u8 {
        match reg & 7 {
            0 => {
                self.di_full = false;
                self.isr1 &= !BI;
                self.di
            }
            1 => std::mem::take(&mut self.isr1),
            2 => {
                let mut v = self.isr2 & 0x0F;
                if self.spas {
                    v |= SPAS;
                }
                if self.int() {
                    v |= 0x80;
                }
                self.isr2 = 0;
                v
            }
            3 => self.spmr,
            4 => {
                (self.minor as u8)
                    | (self.talker_active() as u8) << 1
                    | ((self.la && !self.reset) as u8) << 2
                    | (self.host.eoi as u8) << 7
            }
            5 => self.cpt,
            6 => self.adr[0],
            _ => self.adr[1],
        }
    }

    pub fn write(&mut self, reg: u8, v: u8) {
        match reg & 7 {
            0 => {
                self.out = v;
                self.out_eoi = std::mem::take(&mut self.send_eoi);
                self.bo_ready = false;
                self.isr1 &= !BO;
                if self.talker_active() && !self.spas {
                    self.sh = Sh::Wait;
                }
            }
            1 => self.ie1 = v,
            2 => self.ie2 = v,
            3 => self.spmr = v,
            4 => self.admr = v,
            5 => self.aux(v),
            6 => {
                let i = (v >> 7) as usize;
                self.adr[i] = v & 0x7F;
            }
            _ => self.eos = v,
        }
        self.update();
    }

    fn aux(&mut self, v: u8) {
        match v >> 5 {
            0 => match v & 0x0F {
                0x00 => self.reset = false,
                0x02 => self.chip_reset(),
                0x03 => {
                    // finish handshake (after a holdoff)
                    if self.ah == Ah::Accepting {
                        self.ah = Ah::WaitDavOff;
                    }
                }
                0x06 => self.send_eoi = true,
                _ => {}
            },
            1 => {} // internal counter (clock prescaler)
            4 => self.aux_a = v & 0x1F,
            5 => self.aux_b = v & 0x1F,
            _ => {}
        }
    }

    /// The lines the host (and any other device) drives.
    pub fn set_host(&mut self, h: HostLines) {
        if h.ifc {
            self.la = false;
            self.ta = false;
            self.spms = false;
        }
        self.host = h;
        self.update();
    }

    /// Does `addr` (5 bits) match address register `i` (enabled for listen or talk)?
    fn matches(&self, i: usize, addr: u8, talk: bool) -> bool {
        let r = self.adr[i];
        let disabled = if talk { r & 0x40 != 0 } else { r & 0x20 != 0 };
        // Mode 1: ADR0 is the major address, ADR1 the minor one.
        let used = i == 0 || self.admr & 3 == 1;
        used && !disabled && r & 0x1F == addr
    }

    fn command(&mut self, b: u8) {
        let b = b & 0x7F;
        let before = (self.la, self.ta);
        match b & 0x60 {
            0x20 => {
                let a = b & 0x1F;
                if a == 31 {
                    self.la = false;
                }
                else if let Some(i) = (0..2).find(|&i| self.matches(i, a, false)) {
                    self.la = true;
                    self.minor = i == 1;
                }
            }
            0x40 => {
                let a = b & 0x1F;
                if a == 31 {
                    self.ta = false;
                }
                else if let Some(i) = (0..2).find(|&i| self.matches(i, a, true)) {
                    self.ta = true;
                    self.minor = i == 1;
                }
                else {
                    // another talker addressed
                    self.ta = false;
                }
            }
            0x00 => match b {
                0x14 => self.isr1 |= DEC,
                0x04 if self.la => self.isr1 |= DEC,
                0x08 if self.la => self.isr1 |= GET,
                0x18 => self.spms = true,
                0x19 => self.spms = false,
                _ => {
                    if self.aux_b & 1 != 0 {
                        self.cpt = b;
                        self.isr1 |= CPT;
                    }
                }
            },
            _ => {} // secondary address: not used in mode 1
        }
        if before != (self.la, self.ta) {
            self.isr2 |= ADSC;
        }
    }

    /// Run the handshakes against the current lines.
    pub fn update(&mut self) {
        let h = self.host;
        // Acceptor handshake.
        if !self.listener_active() {
            self.ah = Ah::Idle;
        }
        else {
            if self.ah == Ah::Idle {
                self.ah = Ah::Ready;
            }
            loop {
                match self.ah {
                    Ah::Ready if h.dav && (h.atn || !self.di_full) => {
                        if h.atn {
                            self.command(h.dio);
                        }
                        else {
                            self.di = h.dio;
                            self.di_full = true;
                            self.isr1 |= BI;
                            if h.eoi {
                                self.isr1 |= END;
                            }
                        }
                        self.ah = Ah::Accepting;
                    }
                    Ah::Accepting => self.ah = Ah::WaitDavOff,
                    Ah::WaitDavOff if !h.dav => self.ah = Ah::Ready,
                    _ => break,
                }
                if !self.listener_active() {
                    self.ah = Ah::Idle;
                    break;
                }
            }
        }
        // Source handshake (talker active, ATN released).
        if self.host.atn {
            self.sp_sent = false;
        }
        if !self.talker_active() {
            if self.sh != Sh::Idle && self.host.atn {
                // ATN taken: the byte is kept and goes out when talking resumes.
                self.sh = if self.spas { Sh::Idle } else { Sh::Wait };
            }
            self.spas = false;
            if !self.ta || self.reset {
                self.sh = Sh::Idle;
                self.bo_ready = false;
            }
            return;
        }
        if self.spms && self.sh == Sh::Idle && !self.spas && !self.sp_sent {
            self.spas = true;
            self.sh = Sh::Wait;
        }
        loop {
            match self.sh {
                Sh::Idle => {
                    if !self.bo_ready && !self.spms {
                        self.bo_ready = true;
                        self.isr1 |= BO;
                    }
                    break;
                }
                Sh::Wait if !h.nrfd => {
                    if !h.ndac {
                        // nobody listening
                        self.isr1 |= ERR;
                    }
                    self.sh = Sh::Dav;
                }
                Sh::Dav if !h.ndac => {
                    if self.spas {
                        self.spmr &= !0x40;
                        self.isr2 |= SPAS;
                        self.spas = false;
                        self.sp_sent = true;
                        self.sh = Sh::Idle;
                        break;
                    }
                    self.sh = Sh::Idle;
                }
                _ => break,
            }
        }
    }

    /// The lines the chip drives.
    pub fn lines(&self) -> Lines {
        let mut l = Lines::default();
        if self.reset {
            return l;
        }
        match self.ah {
            Ah::Idle => {}
            Ah::Ready => {
                l.ndac = true;
                l.nrfd = self.di_full && !self.host.atn;
            }
            Ah::Accepting => {
                l.ndac = true;
                l.nrfd = true;
            }
            Ah::WaitDavOff => {
                l.nrfd = true;
            }
        }
        if self.sh == Sh::Dav && self.talker_active() {
            l.dav = true;
            if self.spas {
                l.dio = self.spmr;
                l.eoi = self.aux_b & 2 != 0;
            }
            else {
                l.dio = self.out;
                l.eoi = self.out_eoi;
            }
        }
        l.srq = self.spmr & 0x40 != 0 && !self.spas;
        l
    }
}
