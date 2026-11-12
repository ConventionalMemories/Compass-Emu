//! Boot a 1101 from a real bubble image (no disks) and log when the screen changes.
//! BUBBLE_IMG=img ROMS=dir OUT=dir cargo test --release -p grid_core --test bubble_boot -- --ignored --nocapture
use grid_core::session::{png_bytes, Session};

#[test]
#[ignore]
fn boot_from_bubble() {
    let img = std::fs::read(std::env::var("BUBBLE_IMG").unwrap()).unwrap();
    let roms = std::path::PathBuf::from(std::env::var("ROMS").unwrap());
    let out = std::env::var("OUT").unwrap();
    let secs: f64 = std::env::var("SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let model = grid_core::GridModel::parse(&std::env::var("MODEL").unwrap_or("1101".into())).unwrap();
    let mut m = grid_core::roms::machine(model, &roms).unwrap();
    let mut b = grid_core::bubble::Bubble::new(img);
    b.log = std::env::var("BLOG").is_ok();
    m.board_mut().bubble = Some(b);
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
                std::fs::write(format!("{out}/s{n:02}_{:.2}.png", s.seconds()), png_bytes(w, h, &px)).unwrap();
            }
            n += 1;
            last = px;
        }
    }
}
