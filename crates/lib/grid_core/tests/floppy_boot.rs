//! Boot from a floppy in a 2102 (no hard disk) and log when the screen changes. Low-level when
//! ROMS holds the 2102 firmware (300237-02.bin), else (or with GRID_2102=protocol) the
//! protocol-level model.
//! FLOP_IMG=img ROMS=dir OUT=dir cargo test --release -p grid_core --test floppy_boot -- --ignored --nocapture
use grid_core::gpib::{Drive, DriveKind};
use grid_core::session::{png_bytes, Session};

#[test]
#[ignore]
fn boot_from_2102() {
    let img = std::fs::read(std::env::var("FLOP_IMG").unwrap()).unwrap();
    let roms = std::path::PathBuf::from(std::env::var("ROMS").unwrap());
    let out = std::env::var("OUT").unwrap();
    let secs: f64 = std::env::var("SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let model = grid_core::GridModel::parse(&std::env::var("MODEL").unwrap_or("1129".into())).unwrap();
    let mut m = grid_core::roms::machine(model, &roms).unwrap();
    println!("2102: {}", if m.board().gpib.rom_2102.is_some() { "low-level" } else { "protocol model" });
    m.board_mut().gpib.drives.push(Drive::new(DriveKind::Floppy2102, img));
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
    println!("wall {:.1} s for {secs} s", wall.elapsed().as_secs_f64());
}
