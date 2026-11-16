//! Host side of the Compass's sound and serial port: audio output through cpal, and the
//! 8274's channel A to a TCP socket or a real serial port, and the internal modem's phone line.

use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Audio output. Samples go into a shared queue that the audio callback drains.
pub struct Audio {
    _stream: cpal::Stream,
    queue: Arc<Mutex<VecDeque<f32>>>,
    pub rate: u32,
    max_queue: usize,
}

impl Audio {
    pub fn open() -> Option<Audio> {
        let host = cpal::default_host();
        let dev = host.default_output_device()?;
        let cfg = dev.default_output_config().ok()?;
        let rate = cfg.sample_rate().0;
        let channels = cfg.channels() as usize;
        let queue: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
        let q = queue.clone();
        let err = |e| eprintln!("audio: {e}");
        let sc: cpal::StreamConfig = cfg.clone().into();
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => dev
                .build_output_stream(
                    &sc,
                    move |data: &mut [f32], _| fill(data, channels, &q, |v| v),
                    err,
                    None,
                )
                .ok()?,
            cpal::SampleFormat::I16 => dev
                .build_output_stream(
                    &sc,
                    move |data: &mut [i16], _| fill(data, channels, &q, |v| (v * 32000.0) as i16),
                    err,
                    None,
                )
                .ok()?,
            cpal::SampleFormat::U16 => dev
                .build_output_stream(
                    &sc,
                    move |data: &mut [u16], _| fill(data, channels, &q, |v| (v * 32000.0 + 32768.0) as u16),
                    err,
                    None,
                )
                .ok()?,
            _ => return None,
        };
        stream.play().ok()?;
        Some(Audio { _stream: stream, queue, rate, max_queue: rate as usize / 6 })
    }

    /// Queue samples; keeps at most ~170 ms queued (drops the oldest, e.g. in turbo).
    pub fn push(&self, s: impl Iterator<Item = f32>) {
        let mut q = self.queue.lock().unwrap();
        q.extend(s);
        while q.len() > self.max_queue {
            q.pop_front();
        }
    }
}

fn fill<T: Copy>(data: &mut [T], channels: usize, q: &Arc<Mutex<VecDeque<f32>>>, conv: impl Fn(f32) -> T) {
    let mut q = q.lock().unwrap();
    for frame in data.chunks_mut(channels.max(1)) {
        let v = q.pop_front().unwrap_or(0.0).clamp(-1.0, 1.0) * 0.8;
        for o in frame.iter_mut() {
            *o = conv(v);
        }
    }
}

/// Where the Compass's serial port goes.
pub enum SerialLink {
    /// Listen on localhost:port; one client at a time (a terminal program, or a virtual
    /// null-modem bridge).
    Tcp { listener: TcpListener, client: Option<TcpStream> },
    /// Connect to another emulator (or any TCP server) at host:port, and keep trying while it
    /// is not there: two Compass-Emus linked like a null-modem cable.
    Connect { addr: String, stream: Option<TcpStream>, next_try: std::time::Instant },
    /// A real serial port (COM3, /dev/ttyUSB0 ...); the bit rate follows the Compass.
    Port { port: Box<dyn serialport::SerialPort>, rate: u32 },
}

impl SerialLink {
    /// `spec` is `tcp:PORT` (listen on this PC), `tcp:HOST:PORT` (listen on that address,
    /// e.g. 0.0.0.0 for other PCs), `connect:PORT` / `connect:HOST:PORT` (connect to another
    /// emulator) or a serial port name.
    pub fn open(spec: &str) -> Result<SerialLink, String> {
        if let Some(p) = spec.strip_prefix("connect:") {
            let addr = if p.contains(':') { p.to_string() } else { format!("127.0.0.1:{p}") };
            let port = addr.rsplit(':').next().unwrap_or("");
            port.parse::<u16>().map_err(|_| format!("bad TCP port {port}"))?;
            return Ok(SerialLink::Connect { addr, stream: None, next_try: std::time::Instant::now() });
        }
        if let Some(p) = spec.strip_prefix("tcp:") {
            let (host, p) = match p.rsplit_once(':') {
                Some((h, p)) => (h, p),
                None => ("127.0.0.1", p),
            };
            let port: u16 = p.parse().map_err(|_| format!("bad TCP port {p}"))?;
            let l = TcpListener::bind((host, port)).map_err(|e| e.to_string())?;
            l.set_nonblocking(true).map_err(|e| e.to_string())?;
            Ok(SerialLink::Tcp { listener: l, client: None })
        }
        else {
            let port = serialport::new(spec, 9600)
                .timeout(std::time::Duration::from_millis(0))
                .open()
                .map_err(|e| e.to_string())?;
            Ok(SerialLink::Port { port, rate: 9600 })
        }
    }

    pub fn connected(&self) -> bool {
        match self {
            SerialLink::Tcp { client, .. } => client.is_some(),
            SerialLink::Connect { stream, .. } => stream.is_some(),
            SerialLink::Port { .. } => true,
        }
    }

    /// Move bytes both ways. `bit_rate` is what the Compass programmed (for a real port).
    pub fn pump(&mut self, out: &[u8], into: &mut VecDeque<u8>, bit_rate: f64) {
        match self {
            SerialLink::Tcp { listener, client } => {
                if client.is_none() {
                    if let Ok((s, _)) = listener.accept() {
                        let _ = s.set_nonblocking(true);
                        let _ = s.set_nodelay(true);
                        *client = Some(s);
                    }
                }
                let mut drop = false;
                if let Some(s) = client {
                    if !out.is_empty() && s.write_all(out).is_err() {
                        drop = true;
                    }
                    let mut buf = [0u8; 512];
                    match s.read(&mut buf) {
                        Ok(0) => drop = true,
                        Ok(n) => into.extend(&buf[..n]),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(_) => drop = true,
                    }
                }
                if drop {
                    *client = None;
                }
            }
            SerialLink::Connect { addr, stream, next_try } => {
                if stream.is_none() && std::time::Instant::now() >= *next_try {
                    *next_try = std::time::Instant::now() + std::time::Duration::from_secs(1);
                    use std::net::ToSocketAddrs;
                    if let Some(sa) = addr.to_socket_addrs().ok().and_then(|mut a| a.next()) {
                        if let Ok(s) = TcpStream::connect_timeout(&sa, std::time::Duration::from_millis(20)) {
                            let _ = s.set_nonblocking(true);
                            let _ = s.set_nodelay(true);
                            *stream = Some(s);
                        }
                    }
                }
                let mut drop = false;
                if let Some(s) = stream {
                    if !out.is_empty() && s.write_all(out).is_err() {
                        drop = true;
                    }
                    let mut buf = [0u8; 512];
                    match s.read(&mut buf) {
                        Ok(0) => drop = true,
                        Ok(n) => into.extend(&buf[..n]),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(_) => drop = true,
                    }
                }
                if drop {
                    *stream = None;
                }
            }
            SerialLink::Port { port, rate } => {
                // Standard rates only; pick the nearest.
                const RATES: [u32; 10] = [110, 300, 600, 1200, 2400, 4800, 9600, 19200, 38400, 57600];
                let want = RATES
                    .iter()
                    .copied()
                    .min_by(|a, b| ((*a as f64 - bit_rate).abs()).partial_cmp(&(*b as f64 - bit_rate).abs()).unwrap())
                    .unwrap_or(9600);
                if want != *rate && port.set_baud_rate(want).is_ok() {
                    *rate = want;
                }
                if !out.is_empty() {
                    let _ = port.write_all(out);
                }
                let mut buf = [0u8; 512];
                if let Ok(n) = port.read(&mut buf) {
                    into.extend(&buf[..n]);
                }
            }
        }
    }
}

/// GRiDLink cable: the 8274's channel A in SDLC mode, carried as whole frames over TCP.
///
/// Wire format (both directions; see docs/gridlink.md): a stream of messages, each
/// `length` (u16, little-endian: the bytes that follow it), `type` (u8), payload.
///   type 46h 'F'  one SDLC frame that passed its CRC: address, control, then the rest
///                 (for GRiDLink I and XID frames a 16-bit LE length and the data); no flags,
///                 no CRC, no bit stuffing.
///   type 48h 'H'  hello, sent once by each side on connecting: ASCII "GRiDLink-TCP 1".
/// Messages of any other type are skipped, so later versions can add line-state messages.
pub struct GridLink {
    link: SerialLink,
    inbuf: VecDeque<u8>,
    up: bool,
    pub frames_out: u64,
    pub frames_in: u64,
}

pub const GRIDLINK_HELLO: &[u8] = b"GRiDLink-TCP 1";

impl GridLink {
    /// `spec`: tcp:PORT, tcp:HOST:PORT (wait for a connection) or connect:[HOST:]PORT.
    pub fn open(spec: &str) -> Result<GridLink, String> {
        if !spec.starts_with("tcp:") && !spec.starts_with("connect:") {
            return Err("GRiDLink needs tcp:PORT, tcp:HOST:PORT or connect:[HOST:]PORT (a PC serial port can't do SDLC)".into());
        }
        Ok(GridLink { link: SerialLink::open(spec)?, inbuf: VecDeque::new(), up: false, frames_out: 0, frames_in: 0 })
    }

    pub fn connected(&self) -> bool {
        self.link.connected()
    }

    fn message(out: &mut Vec<u8>, kind: u8, payload: &[u8]) {
        out.extend(((payload.len() + 1) as u16).to_le_bytes());
        out.push(kind);
        out.extend(payload);
    }

    /// Send the frames the Compass finished; take in the frames the other side sent.
    pub fn pump(&mut self, frames: impl Iterator<Item = Vec<u8>>, into: &mut VecDeque<Vec<u8>>) {
        let mut out = Vec::new();
        if self.up {
            for f in frames {
                Self::message(&mut out, b'F', &f);
                self.frames_out += 1;
            }
        }
        else {
            frames.for_each(drop);
        }
        let mut raw = VecDeque::new();
        self.link.pump(&out, &mut raw, 0.0);
        let now_up = self.link.connected();
        if now_up != self.up {
            self.up = now_up;
            self.inbuf.clear();
            if now_up {
                let mut hello = Vec::new();
                Self::message(&mut hello, b'H', GRIDLINK_HELLO);
                self.link.pump(&hello, &mut raw, 0.0);
            }
        }
        self.inbuf.extend(raw);
        loop {
            if self.inbuf.len() < 2 {
                break;
            }
            let n = self.inbuf[0] as usize | (self.inbuf[1] as usize) << 8;
            if self.inbuf.len() < 2 + n {
                break;
            }
            let msg: Vec<u8> = self.inbuf.drain(..2 + n).skip(2).collect();
            if msg.first() == Some(&b'F') && msg.len() >= 3 {
                into.push_back(msg[1..].to_vec());
                self.frames_in += 1;
            }
        }
    }
}

/// The internal modem's phone line: the far ends are TCP connections carrying the raw data
/// bytes (see docs/modem.md). Dialing connects to the number's address from the phone book;
/// an incoming connection on the listen address rings the Compass. Connecting = the far end
/// answered, closing = it hung up.
pub struct ModemLine {
    listener: Option<TcpListener>,
    book: Vec<(String, String)>,
    stream: Option<TcpStream>,
    /// An incoming caller waiting for the Compass to answer.
    caller: Option<TcpStream>,
    connecting: Option<std::sync::mpsc::Receiver<Option<TcpStream>>>,
    /// What happened last (for the status line).
    pub last: String,
}

impl ModemLine {
    /// `listen`: "" or tcp:PORT / tcp:HOST:PORT. `book`: NUMBER=HOST:PORT entries separated by
    /// commas or semicolons; NUMBER * matches any number.
    pub fn open(listen: &str, book: &str) -> Result<ModemLine, String> {
        let listener = match listen.trim() {
            "" => None,
            l => {
                let p = l.strip_prefix("tcp:").ok_or_else(|| format!("listen address {l}: use tcp:PORT or tcp:HOST:PORT"))?;
                let (host, p) = p.rsplit_once(':').unwrap_or(("127.0.0.1", p));
                let port: u16 = p.parse().map_err(|_| format!("bad TCP port {p}"))?;
                let l = TcpListener::bind((host, port)).map_err(|e| format!("cannot listen on {host}:{port}: {e}"))?;
                l.set_nonblocking(true).map_err(|e| e.to_string())?;
                Some(l)
            }
        };
        let mut entries = vec![];
        for e in book.split([',', ';']).map(str::trim).filter(|e| !e.is_empty()) {
            let (n, a) = e.split_once('=').ok_or_else(|| format!("phone book entry {e}: use NUMBER=HOST:PORT"))?;
            let a = a.trim();
            if !a.contains(':') {
                return Err(format!("phone book entry {e}: the address needs a port (HOST:PORT)"));
            }
            entries.push((normalize_number(n), a.to_string()));
        }
        Ok(ModemLine { listener, book: entries, stream: None, caller: None, connecting: None, last: String::new() })
    }

    /// The address a dialed number reaches.
    pub fn lookup(&self, number: &str) -> Option<String> {
        let n = normalize_number(number);
        if let Some((_, a)) = self.book.iter().find(|(k, _)| *k == n) {
            return Some(a.clone());
        }
        // 127*0*0*1*2323 = 127.0.0.1:2323
        let parts: Vec<&str> = n.split('*').collect();
        if parts.len() == 5 && parts.iter().all(|p| !p.is_empty() && p.parse::<u16>().is_ok()) {
            return Some(format!("{}.{}.{}.{}:{}", parts[0], parts[1], parts[2], parts[3], parts[4]));
        }
        self.book.iter().find(|(k, _)| k == "*").map(|(_, a)| a.clone())
    }

    /// Follow the Compass's line events and move data both ways. `now` = modem time (CPU clocks).
    pub fn pump(&mut self, line: &mut grid_core::modem::Line, now: f64) {
        use grid_core::modem::LineEvent;
        while let Some(ev) = line.events.pop_front() {
            match ev {
                LineEvent::Dial(number) => match self.lookup(&number) {
                    Some(addr) => {
                        self.last = format!("dialing {number} ({addr})");
                        let (tx, rx) = std::sync::mpsc::channel();
                        std::thread::spawn(move || {
                            use std::net::ToSocketAddrs;
                            let s = addr.to_socket_addrs().ok().and_then(|mut a| a.next()).and_then(|sa| {
                                TcpStream::connect_timeout(&sa, std::time::Duration::from_secs(5)).ok()
                            });
                            let _ = tx.send(s);
                        });
                        self.connecting = Some(rx);
                    }
                    None => {
                        self.last = format!("{number}: not in the phone book");
                        line.call_failed();
                    }
                },
                LineEvent::Answer => {
                    if let Some(s) = self.caller.take() {
                        self.stream = Some(s);
                        self.last = "answered".into();
                    }
                }
                LineEvent::HangUp => {
                    self.stream = None;
                    self.caller = None;
                    self.connecting = None;
                    self.last = "hung up".into();
                }
            }
        }
        if let Some(rx) = &self.connecting {
            if let Ok(r) = rx.try_recv() {
                self.connecting = None;
                match r {
                    Some(s) => {
                        let _ = s.set_nonblocking(true);
                        let _ = s.set_nodelay(true);
                        self.stream = Some(s);
                        line.answered(now);
                        self.last = "connected".into();
                    }
                    None => {
                        self.last = "no answer".into();
                        line.call_failed();
                    }
                }
            }
        }
        if let Some(l) = &self.listener {
            if let Ok((s, peer)) = l.accept() {
                if self.stream.is_none() && self.caller.is_none() && self.connecting.is_none() && line.incoming_call(now) {
                    let _ = s.set_nonblocking(true);
                    let _ = s.set_nodelay(true);
                    self.caller = Some(s);
                    self.last = format!("ringing ({peer})");
                }
                // else: busy, the connection is closed
            }
        }
        if let Some(c) = &self.caller {
            let mut b = [0u8; 1];
            let gone = match c.peek(&mut b) {
                Ok(0) => true,
                Ok(_) => false,
                Err(e) => e.kind() != std::io::ErrorKind::WouldBlock,
            };
            if gone {
                self.caller = None;
                line.remote_hangup();
                self.last = "caller hung up".into();
            }
        }
        let mut drop = false;
        if let Some(s) = &mut self.stream {
            if !line.tx_out.is_empty() {
                let out: Vec<u8> = line.tx_out.drain(..).collect();
                if s.write_all(&out).is_err() {
                    drop = true;
                }
            }
            let mut buf = [0u8; 512];
            match s.read(&mut buf) {
                Ok(0) => drop = true,
                Ok(n) => line.rx_in.extend(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => drop = true,
            }
        }
        else {
            line.tx_out.clear();
        }
        if drop {
            self.stream = None;
            line.remote_hangup();
            self.last = "far end hung up".into();
        }
    }
}

/// A dialed number as the phone book compares it: digits, * and # only.
fn normalize_number(n: &str) -> String {
    n.chars().filter(|c| c.is_ascii_digit() || *c == '*' || *c == '#').collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modem_phone_book() {
        let ml = ModemLine::open("", "5551234=10.0.0.1:2400, *=127.0.0.1:2323").unwrap();
        assert_eq!(ml.lookup("555-1234").as_deref(), Some("10.0.0.1:2400"));
        assert_eq!(ml.lookup("911").as_deref(), Some("127.0.0.1:2323"));
        assert_eq!(ml.lookup("192*168*1*20*2400").as_deref(), Some("192.168.1.20:2400"));
        assert!(ModemLine::open("", "555=nohost").is_err());
    }

    #[test]
    fn modem_line_calls_and_rings() {
        use grid_core::modem::{CallState, Line, LineEvent};
        let wait = |f: &mut dyn FnMut() -> bool| {
            for _ in 0..500 {
                if f() {
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            false
        };
        // the Compass dials a server
        let server = TcpListener::bind("127.0.0.1:23991").unwrap();
        let mut ml = ModemLine::open("", "555=127.0.0.1:23991").unwrap();
        let mut line = Line::default();
        line.call = CallState::Calling;
        line.events.push_back(LineEvent::Dial("555".into()));
        assert!(wait(&mut || {
            ml.pump(&mut line, 0.0);
            line.connected()
        }));
        let (mut peer, _) = server.accept().unwrap();
        line.tx_out.extend(b"ATZ");
        ml.pump(&mut line, 0.0);
        let mut buf = [0u8; 3];
        peer.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"ATZ");
        peer.write_all(b"OK").unwrap();
        assert!(wait(&mut || {
            ml.pump(&mut line, 0.0);
            line.rx_in.len() == 2
        }));
        drop(peer);
        assert!(wait(&mut || {
            ml.pump(&mut line, 0.0);
            !line.connected()
        }));
        // a server calls the Compass
        let mut ml = ModemLine::open("tcp:23992", "").unwrap();
        let mut line = Line::default();
        let _caller = TcpStream::connect("127.0.0.1:23992").unwrap();
        assert!(wait(&mut || {
            ml.pump(&mut line, 0.0);
            matches!(line.call, CallState::Ringing { .. })
        }));
    }

    #[test]
    fn two_emulators_link() {
        let mut a = SerialLink::open("tcp:23987").unwrap();
        let mut b = SerialLink::open("connect:23987").unwrap();
        let (mut ina, mut inb) = (VecDeque::new(), VecDeque::new());
        for _ in 0..50 {
            b.pump(b"", &mut inb, 9600.0);
            a.pump(b"", &mut ina, 9600.0);
            if a.connected() && b.connected() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(a.connected() && b.connected());
        a.pump(b"hello", &mut ina, 9600.0);
        b.pump(b"world", &mut inb, 9600.0);
        for _ in 0..50 {
            a.pump(b"", &mut ina, 9600.0);
            b.pump(b"", &mut inb, 9600.0);
            if ina.len() == 5 && inb.len() == 5 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(inb.iter().copied().collect::<Vec<u8>>(), b"hello");
        assert_eq!(ina.iter().copied().collect::<Vec<u8>>(), b"world");
    }

    #[test]
    fn gridlink_frames_over_tcp() {
        let mut a = GridLink::open("tcp:23988").unwrap();
        let mut b = GridLink::open("connect:23988").unwrap();
        let (mut ina, mut inb) = (VecDeque::new(), VecDeque::new());
        for _ in 0..50 {
            b.pump(std::iter::empty(), &mut inb);
            a.pump(std::iter::empty(), &mut ina);
            if a.connected() && b.connected() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(a.connected() && b.connected());
        let big: Vec<u8> = [0x82u8, 0x10, 0x08, 0x02].into_iter().chain((0..520).map(|i| i as u8)).collect();
        a.pump(vec![vec![0xC0, 0xBF], big.clone()].into_iter(), &mut ina);
        for _ in 0..50 {
            a.pump(std::iter::empty(), &mut ina);
            b.pump(std::iter::empty(), &mut inb);
            if inb.len() == 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(inb, VecDeque::from(vec![vec![0xC0, 0xBF], big]));
        assert_eq!((a.frames_out, b.frames_in), (2, 2));
    }
}
