//! `compass do`: run a command on the Development Executive command line, headless and as
//! fast as the PC allows, then save the screen. Build mode (gridbuild.py) uses this in place
//! of MAME + gridbuild.lua.
//!
//!   compass do [--model M] --hd disk.img [--roms DIR] [--shot screen.png] [--start S]
//!              [--max-seconds N] [--bubble F] "Do build"
//!
//! Boots, waits for a steady "=>" prompt (not before --start emulated seconds, default 20),
//! types the command and Return, waits until the prompt comes back, saves --shot and writes
//! the changed disk back to the image. Prints "GRIDBUILD done after N emulated seconds".
//! Exit code 0 when the prompt came back, 2 at the time limit (default 10800 s), 1 on errors.

use std::{fs, path::PathBuf, time::Instant};

use grid_core::{
    gpib::{Drive, DriveKind},
    session::{png_bytes, Session},
    GridModel,
};

pub fn run(args: Vec<String>, roms_default: PathBuf) -> i32 {
    let mut model = GridModel::Compass1129;
    let mut hd: Option<PathBuf> = None;
    let mut roms = roms_default;
    let mut shot: Option<PathBuf> = None;
    let mut start = 20.0;
    let mut max_s = 10800.0;
    let mut bubble: Option<PathBuf> = None;
    let mut command: Option<String> = None;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().unwrap_or_else(|| fail(&format!("{a} needs a value")));
        match a.as_str() {
            "--model" => model = GridModel::parse(&val()).unwrap_or_else(|| fail("unknown model")),
            "--hd" => hd = Some(val().into()),
            "--roms" => roms = val().into(),
            "--shot" => shot = Some(val().into()),
            "--start" => start = val().parse().unwrap_or(20.0),
            "--max-seconds" => max_s = val().parse().unwrap_or(10800.0),
            "--bubble" => bubble = Some(val().into()),
            _ if a.starts_with("--") => fail(&format!("unknown option {a}")),
            _ => command = Some(a),
        }
    }
    let Some(command) = command else { fail("no command given") };
    let Some(hd) = hd else { fail("--hd disk.img is needed") };
    let mut m = grid_core::roms::machine(model, &roms).unwrap_or_else(|e| fail(&e));
    let img = fs::read(&hd).unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", hd.display())));
    m.board_mut().gpib.drives.push(Drive::new(DriveKind::Hdd2101, img));
    let bub = bubble.as_ref().map(|p| fs::read(p).unwrap_or_else(|_| vec![0u8; 3 * grid_core::bubble::MBM_SIZE]));
    m.board_mut().bubble = Some(grid_core::bubble::Bubble::new(bub.unwrap_or_else(|| vec![0u8; 3 * grid_core::bubble::MBM_SIZE])));
    m.board_mut().sound.enabled = false;
    m.board_mut().log_unhandled = false;
    if std::env::var("GRID_NO_CONTENTION").is_ok() {
        m.board_mut().display.contention = false;
    }
    let mut s = Session::new(m);
    let wall = Instant::now();

    s.run_seconds(start);
    let code = if !s.wait_prompt(max_s) {
        println!("no command line prompt after {:.0} emulated seconds", s.seconds());
        2
    }
    else {
        let typed_at = s.seconds();
        s.type_keys(&format!("{command}{{ret}}")).unwrap_or_else(|e| fail(&e));
        match s.wait_prompt_back(max_s - typed_at) {
            Some(t) => {
                println!("GRIDBUILD done after {t:.0} emulated seconds");
                0
            }
            None => {
                println!("time limit: the prompt did not come back within {max_s:.0} emulated seconds");
                2
            }
        }
    };
    eprintln!(
        "Compass {}: {:.0} emulated seconds in {:.1} s ({:.0}x)",
        model.name(),
        s.seconds(),
        wall.elapsed().as_secs_f64(),
        s.seconds() / wall.elapsed().as_secs_f64().max(0.001)
    );
    if let Some(p) = &shot {
        let (w, h, px) = s.screen();
        if let Err(e) = fs::write(p, png_bytes(w, h, &px)) {
            eprintln!("cannot write {}: {e}", p.display());
        }
    }
    let b = s.m.board();
    if let Some(d) = b.gpib.drives.first() {
        if d.dirty {
            if let Err(e) = fs::write(&hd, &d.image) {
                eprintln!("cannot write {}: {e}", hd.display());
                return 1;
            }
        }
    }
    if let (Some(p), Some(bb)) = (&bubble, &b.bubble) {
        if bb.dirty {
            let _ = fs::write(p, &bb.image);
        }
    }
    code
}

fn fail(msg: &str) -> ! {
    eprintln!("compass do: {msg}");
    std::process::exit(1)
}
