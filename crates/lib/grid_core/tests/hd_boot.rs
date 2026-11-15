//! Boot from the hard disk in a 2101 and log when the screen changes. Low-level when
//! ROMS holds the 2101 firmware for the image (300205-05.bin for 20 MB), else (or with
//! GRID_2101=protocol) the protocol-level model.
//! HD_IMG=img ROMS=dir OUT=dir cargo test --release -p grid_core --test hd_boot -- --ignored --nocapture
use grid_core::gpib::{Drive, DriveKind};
use grid_core::session::{png_bytes, Session};

#[test]
#[ignore]
fn boot_from_2101() {
    let img = std::fs::read(std::env::var("HD_IMG").unwrap()).unwrap();
    let roms = std::path::PathBuf::from(std::env::var("ROMS").unwrap());
    let out = std::env::var("OUT").unwrap();
    let secs: f64 = std::env::var("SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let model = grid_core::GridModel::parse(&std::env::var("MODEL").unwrap_or("1101".into())).unwrap();
    let mut m = grid_core::roms::machine(model, &roms).unwrap();
    println!("2101 ROMs: {:?}", m.board().gpib.rom_2101.iter().map(|r| r.is_some()).collect::<Vec<_>>());
    m.board_mut().gpib.drives.push(Drive::new(DriveKind::Hdd2101, img));
    m.board_mut().gpib.add_2101_floppy_slot();
    m.board_mut().sound.enabled = false;
    m.board_mut().log_unhandled = false;
    let mut s = Session::new(m);
    let mut last = Vec::new();
    let mut n = 0;
    let wall = std::time::Instant::now();
    while s.seconds() < secs {
        s.run_seconds(0.05);
        let (w, h, px) = s.screen();
        if px != last {
            let lit = px.iter().filter(|&&p| p != 0).count();
            println!("{:6.2} s  lit {lit}", s.seconds());
            if n < 60 {
                std::fs::write(format!("{out}/s{n:02}_{:.2}.png", s.seconds()), png_bytes(w, h, &px)).unwrap();
            }
            n += 1;
            last = px;
        }
    }
    let d = &s.m.board().gpib.drives[0];
    println!("low-level: {} display {:?} log {:?}", d.lle2101.is_some(), d.display(), d.log);
    println!("wall {:.1} s for {secs} s", wall.elapsed().as_secs_f64());
}

/// Boot from a floppy in a 2101's floppy drive (blank hard disk unless HD_IMG is set).
/// FLOP_IMG=img ROMS=dir OUT=dir cargo test --release -p grid_core --test hd_boot boot_from_2101_floppy -- --ignored --nocapture
#[test]
#[ignore]
fn boot_from_2101_floppy() {
    let fd = std::fs::read(std::env::var("FLOP_IMG").unwrap()).unwrap();
    let hd = match std::env::var("HD_IMG") {
        Ok(p) => std::fs::read(p).unwrap(),
        Err(_) => vec![0u8; 41616 * 512],
    };
    let roms = std::path::PathBuf::from(std::env::var("ROMS").unwrap());
    let out = std::env::var("OUT").unwrap();
    let secs: f64 = std::env::var("SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let model = grid_core::GridModel::parse(&std::env::var("MODEL").unwrap_or("1101".into())).unwrap();
    let mut m = grid_core::roms::machine(model, &roms).unwrap();
    m.board_mut().gpib.drives.push(Drive::new(DriveKind::Hdd2101, hd));
    m.board_mut().gpib.drives.push(Drive::new(DriveKind::Floppy2101, fd));
    m.board_mut().sound.enabled = false;
    m.board_mut().log_unhandled = false;
    let mut s = Session::new(m);
    let mut last = Vec::new();
    let mut n = 0;
    while s.seconds() < secs {
        s.run_seconds(0.05);
        let (w, h, px) = s.screen();
        if px != last {
            let lit = px.iter().filter(|&&p| p != 0).count();
            println!("{:6.2} s  lit {lit}", s.seconds());
            if n < 60 {
                std::fs::write(format!("{out}/f{n:02}_{:.2}.png", s.seconds()), png_bytes(w, h, &px)).unwrap();
            }
            n += 1;
            last = px;
        }
    }
    let d = &s.m.board().gpib.drives[0];
    println!("low-level: {} display {:?} log {:?}", d.lle2101.is_some(), d.display(), d.log);
}
