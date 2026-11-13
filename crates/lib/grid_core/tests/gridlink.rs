//! GRiDLink over the emulated 8274: boots a hard disk, runs a GRiDLink client program
//! (CableTest~Run~ from the GRiD Server "Programs for GRiD Compass" disk, which carries the
//! same secondary-station code as GRiDLink~Device~) and plays the first steps of a primary
//! station against it: address poll XID, address assignment XID, SNRM, RR polls, and an echo
//! of the I-frames the Compass sends. This is a test of the SDLC model, not a server.
//!
//! HD_IMG=disk.img ROMS=dir OUT=dir [MODEL=1129] [CMD=CableTest] [P2P=1]
//!   cargo test --release -p grid_core --test gridlink -- --ignored --nocapture
use grid_core::gpib::{Drive, DriveKind};
use grid_core::session::{png_bytes, Session};

const MS: u64 = 5_000;

#[test]
#[ignore]
fn gridlink_client_signs_on() {
    let img = std::fs::read(std::env::var("HD_IMG").unwrap()).unwrap();
    let roms = std::path::PathBuf::from(std::env::var("ROMS").unwrap());
    let out = std::env::var("OUT").unwrap();
    let model = grid_core::GridModel::parse(&std::env::var("MODEL").unwrap_or("1129".into())).unwrap();
    let cmd = std::env::var("CMD").unwrap_or("CableTest".into());
    let mut m = grid_core::roms::machine(model, &roms).unwrap();
    m.board_mut().gpib.drives.push(Drive::new(DriveKind::Hdd2101, img));
    m.board_mut().gpib.add_2101_floppy_slot();
    m.board_mut().bubble = Some(grid_core::bubble::Bubble::new(vec![0u8; 3 * grid_core::bubble::MBM_SIZE]));
    m.board_mut().sound.enabled = false;
    m.board_mut().log_unhandled = false;
    m.set_fast(std::env::var("EXACT").is_err());
    let p2p = std::env::var("P2P").is_ok();
    {
        let s = &mut m.board_mut().serial;
        s.cts_loop = p2p;
        s.set_modem_lines(true, !p2p);
        s.dcd_auto = !p2p;
        s.log = std::env::var("SLOG").is_ok();
    }
    let mut s = Session::new(m);
    s.keep_output = true;
    let shot = |s: &Session, name: &str| {
        let (w, h, px) = s.screen();
        std::fs::write(format!("{out}/{name}.png"), png_bytes(w, h, &px)).unwrap();
    };
    if !s.wait_prompt(120.0) {
        shot(&s, "0-noprompt");
        panic!("no prompt after {:.1} s: {:?}", s.seconds(), s.stop);
    }
    shot(&s, "0-prompt");
    s.type_keys(&format!("{cmd}\r")).unwrap();
    s.run_seconds(8.0);
    shot(&s, "1-started");
    // CableTest asks for the line speed: Return takes 250K
    s.type_keys("\r").unwrap();
    s.run_seconds(2.0);
    shot(&s, "2-speed");
    println!("serial select latches {:02X?}, WR4 {:02X} WR6 {:02X}", s.m.board().serial_select, s.m.board().serial.ch[0].wr[4], s.m.board().serial.ch[0].wr[6]);

    // primary station. CableTest: after the address, TEST+P polls; the Compass answers with
    // a TEST frame of 520 pattern bytes and wants it back (TEST without F). GRiDLink~Device~
    // (CMD=other): SNRM, then RR polls, echoing the Compass's I-frames.
    let cable_test = cmd.eq_ignore_ascii_case("CableTest");
    let mut addr = 0xC0u8;
    let mut state = 0; // 0 polling C0, 1 address given, 2 SNRM sent, 3 connected / testing
    let mut vr = 0u8; // next N(S) expected from the Compass
    let mut vs = 0u8;
    let mut next_poll = s.clocks();
    let mut echo: Option<Vec<u8>> = None;
    let mut frames_in = 0;
    let mut pending: std::collections::VecDeque<(u64, Vec<u8>)> = Default::default();
    let mut answers = 0; // I-frames or TEST frames the Compass sent after connecting
    let end = s.clocks() + 20_000 * MS;
    while s.clocks() < end {
        s.run(MS / 5);
        let sent: Vec<(u64, Vec<u8>)> = s.m.board_mut().serial.sdlc_out.drain(..).collect();
        for (t, f) in sent {
            frames_in += 1;
            if frames_in < 30 {
                println!("{:9.3} s  Compass -> {:02X?} ({} bytes)", t as f64 / 5e6, &f[..f.len().min(16)], f.len());
            }
            let ctl = f.get(1).copied().unwrap_or(0);
            let reply: Vec<Vec<u8>> = match (state, ctl & 0xEF) {
                // XID reply: type, 8-byte ID, address 0 -> assign 82h
                (0, 0xAF) if f.len() >= 14 => {
                    state = 1;
                    addr = 0x82;
                    let mut x = vec![0xC0, 0xBF, 12, 0];
                    x.extend(&f[4..13]);
                    x.extend([addr, 0, 0]);
                    vec![x]
                }
                (1 | 3, 0xE3) if cable_test && f.len() > 4 => {
                    state = 3;
                    answers += 1;
                    let mut x = f.clone();
                    x[1] = 0xE3;
                    vec![x]
                }
                (2, 0x63) => {
                    state = 3;
                    println!("connected as {addr:02X}");
                    vec![]
                }
                (3, c) if c & 1 == 0 => {
                    // I-frame with F: take it, answer with our I-frame (echo of its data)
                    answers += 1;
                    let ns = (c >> 1) & 7;
                    if ns == vr {
                        vr = (vr + 1) & 7;
                        echo = Some(f[4..].to_vec());
                    }
                    let mut v = vec![];
                    if let Some(d) = echo.take() {
                        let mut x = vec![addr, (vs << 1) | (vr << 5)];
                        x.extend((d.len() as u16).to_le_bytes());
                        x.extend(d);
                        vs = (vs + 1) & 7;
                        v.push(x);
                    }
                    v
                }
                _ => vec![],
            };
            // a real primary needs a moment to turn the line around (the Compass sets up
            // its LongRead after its own frame ends)
            for r in reply {
                pending.push_back((t + 2 * MS, r));
            }
            next_poll = s.clocks() + 20 * MS;
        }
        while pending.front().is_some_and(|p| p.0 <= s.clocks()) {
            let (_, r) = pending.pop_front().unwrap();
            if frames_in < 30 {
                println!("{:9.3} s  Server  -> {:02X?} ({} bytes)", s.clocks() as f64 / 5e6, &r[..r.len().min(16)], r.len());
            }
            s.m.board_mut().serial.sdlc_in.push_back(r);
        }
        if s.clocks() >= next_poll && pending.is_empty() {
            let f = match (state, cable_test) {
                (0, _) => vec![0xC0, 0xBF],
                (_, true) => vec![addr, 0xF3],
                (1 | 2, false) => {
                    state = 2;
                    vec![addr, 0x93]
                }
                _ => vec![addr, 0x11 | (vr << 5)],
            };
            if frames_in < 30 {
                println!("{:9.3} s  Server  -> {:02X?}", s.clocks() as f64 / 5e6, f);
            }
            s.m.board_mut().serial.sdlc_in.push_back(f);
            next_poll = s.clocks() + if state == 0 { 300 * MS } else { 100 * MS };
        }
    }
    shot(&s, "3-end");
    println!("frames from the Compass {frames_in}, exchanges after connecting {answers}, state {state}");
    assert!(state >= 3 && answers >= 10, "the Compass did not connect");
}
