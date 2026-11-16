//! Internal modem in the full machine: boots a hard disk whose Programs has Modem~Device~
//! (GRiD-OS activates it at start) and checks that the driver set the modem up through the
//! 8749 firmware (it hung there while the MCU was missing).
//!
//! With KEYS="step|step|..." it then types each step (Session key text, {ret} etc.), runs
//! STEP_S seconds (default 4) and saves OUT/step-N.png, while a far end inside the test
//! answers any number the Compass dials and sends back what it gets in upper case.
//!
//! HD_IMG=disk.img ROMS=dir (with 16020-007.bin) OUT=dir [MODEL=1139] [KEYS=...] [STEP_S=4]
//!   cargo test --release -p grid_core --test modem_boot -- --ignored --nocapture
use grid_core::gpib::{Drive, DriveKind};
use grid_core::session::{png_bytes, Session};

#[test]
#[ignore]
fn modem_device_activates() {
    let img = std::fs::read(std::env::var("HD_IMG").unwrap()).unwrap();
    let roms = std::path::PathBuf::from(std::env::var("ROMS").unwrap());
    let out = std::env::var("OUT").unwrap();
    let model = grid_core::GridModel::parse(&std::env::var("MODEL").unwrap_or("1139".into())).unwrap();
    let mut m = grid_core::roms::machine(model, &roms).unwrap();
    assert!(m.board().modem.is_some(), "no 16020-007.bin in ROMS");
    m.board_mut().gpib.drives.push(Drive::new(DriveKind::Hdd2101, img));
    m.board_mut().gpib.add_2101_floppy_slot();
    m.board_mut().sound.enabled = false;
    m.board_mut().log_unhandled = false;
    m.set_fast(std::env::var("EXACT").is_err());
    let mut s = Session::new(m);
    let shot = |s: &Session, name: &str| {
        let (w, h, px) = s.screen();
        std::fs::write(format!("{out}/{name}.png"), png_bytes(w, h, &px)).unwrap();
    };
    // BOOT_S: run that long instead of waiting for a => prompt (the windowing Executive)
    let ok = match std::env::var("BOOT_S").ok().and_then(|v| v.parse::<f64>().ok()) {
        Some(b) => {
            s.run_seconds(b);
            true
        }
        None => s.wait_prompt(120.0),
    };
    shot(&s, "modem-boot");
    let md = s.m.board().modem.as_ref().unwrap();
    let r = |i| md.mcu.ram_at(i);
    println!(
        "{:.1} s: modem regs 1={:02X} 2={:02X} 6={:02X} 7={:02X} 8={:02X} status {:02X} events {:02X}, line {}",
        s.seconds(),
        r(0x21),
        r(0x22),
        r(0x26),
        r(0x27),
        r(0x28),
        r(0x30),
        r(0x31),
        md.io.line.status()
    );
    assert!(ok, "no prompt: {:?}", s.stop);
    let step_s: f64 = std::env::var("STEP_S").ok().and_then(|v| v.parse().ok()).unwrap_or(4.0);
    let mut far_got = Vec::new();
    for (i, step) in std::env::var("KEYS").unwrap_or_default().split('|').filter(|k| !k.is_empty()).enumerate() {
        s.type_keys(step).unwrap();
        let end = s.seconds() + step_s;
        while s.seconds() < end {
            s.run(25_000);
            let md = s.m.board_mut().modem.as_mut().unwrap();
            let now = md.time() as f64;
            let line = md.line();
            while let Some(ev) = line.events.pop_front() {
                println!("{:.2} s line event {ev:?}", now / 5e6);
                if let grid_core::modem::LineEvent::Dial(_) = ev {
                    line.answered(now);
                }
            }
            let got: Vec<u8> = line.tx_out.drain(..).collect();
            for &b in &got {
                line.rx_in.push_back(b.to_ascii_uppercase());
            }
            far_got.extend(got);
        }
        let md = s.m.board().modem.as_ref().unwrap();
        println!("step {i} {step:?}: line {}, regs 1={:02X} 6={:02X} 7={:02X} status {:02X}, far got {}", md.io.line.status(), md.mcu.ram_at(0x21), md.mcu.ram_at(0x26), md.mcu.ram_at(0x27), md.mcu.ram_at(0x30), far_got.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" "));
        shot(&s, &format!("step-{i}"));
    }
}
