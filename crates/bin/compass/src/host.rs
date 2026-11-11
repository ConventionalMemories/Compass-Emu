//! Host side of the Compass's sound and serial port: audio output through cpal, and the
//! 8274's channel A to a TCP socket or a real serial port.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
