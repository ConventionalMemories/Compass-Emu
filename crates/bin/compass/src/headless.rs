//! `compass do`: run a command on the Development Executive command line, headless and as
//! fast as the PC allows, then save the screen. Build mode (gridbuild.py) uses this in place
//! of MAME + gridbuild.lua.
//!
//!   compass do [--model M] --hd disk.img [--roms DIR] [--shot screen.png] [--start S]
//!              [--max-seconds N] [--bubble F] [--flop F] "Do build"
//!
//! Boots, waits for a steady "=>" prompt (not before --start emulated seconds, default 20),
//! types the command and Return, waits until the prompt comes back, saves --shot and writes
//! the changed disk back to the image. Prints "GRIDBUILD done after N emulated seconds".
//! Exit code 0 when the prompt came back, 2 at the time limit (default 10800 s), 1 on errors.
//!
//! Runs on the fast CPU core (whole instructions, approximate clocks) unless --exact is
//! given; build output does not depend on timing, so builds take the fast path.
//!
//! --flop F: a 2102 portable floppy image (GPIB 6) next to the hard disk, written back when
//! changed. The machine still boots from the hard disk.
//!
//! --folder D: a PC folder as the 2101 "Floppy Disk" (GPIB 5); GRiD's changes are written
//! back to the folder at the end. Not with --state.
//!
//! --state F: if F exists, start from that saved state (taken at the prompt) instead of
//! booting, with the disk image from --hd attached; otherwise boot and save the state at the
//! prompt into F. The disk may have gained files since the state was saved (GRiD-OS reads
//! the volume and bitmap again on every attach), but its system files must be the same.

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
    let mut flop: Option<PathBuf> = None;
    let mut folder: Option<PathBuf> = None;
    let mut state: Option<PathBuf> = None;
    let mut command: Option<String> = None;
    let mut exact = false;
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
            "--flop" => flop = Some(val().into()),
            "--folder" => folder = Some(val().into()),
            "--state" => state = Some(val().into()),
            "--exact" => exact = true,
            "--fast" => exact = false,
            _ if a.starts_with("--") => fail(&format!("unknown option {a}")),
            _ => command = Some(a),
        }
    }
    let Some(command) = command else { fail("no command given") };
    let Some(hd) = hd else { fail("--hd disk.img is needed") };
    let img = fs::read(&hd).unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", hd.display())));
    let bub = bubble.as_ref().map(|p| fs::read(p).unwrap_or_else(|_| vec![0u8; 3 * grid_core::bubble::MBM_SIZE]));
    let wall = Instant::now();
    let resumed = state.as_ref().and_then(|p| fs::read(p).ok()).and_then(|d| match grid_core::GridMachine::load_state(&d) {
        Ok(m) if m.model == model => Some(m),
        Ok(_) => None,
        Err(e) => {
            eprintln!("ignoring saved state: {e}");
            None
        }
    });
    let from_state = resumed.is_some();
    if from_state && folder.is_some() {
        fail("--folder can't be used with a saved state");
    }
    let mut hostdir: Option<grid_core::hostdir::HostDir> = None;
    let mut m = match resumed {
        Some(mut m) => {
            match m.board_mut().gpib.drives.iter_mut().find(|d| d.kind == DriveKind::Hdd2101) {
                Some(d) => d.image = img,
                None => fail("the saved state has no hard disk"),
            }
            if let (Some(b), Some(data)) = (m.board_mut().bubble.as_mut(), bub) {
                b.image = data;
            }
            m
        }
        None => {
            let mut m = grid_core::roms::machine(model, &roms).unwrap_or_else(|e| fail(&e));
            m.board_mut().gpib.drives.push(Drive::new(DriveKind::Hdd2101, img));
            if let Some(f) = &flop {
                let data = fs::read(f).unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", f.display())));
                m.board_mut().gpib.drives.push(Drive::new(DriveKind::Floppy2102, data));
            }
            if let Some(dir) = &folder {
                match grid_core::hostdir::HostDir::import(dir) {
                    Ok((hd, vol)) => {
                        m.board_mut().gpib.drives.push(Drive::new(DriveKind::Floppy2101, vol.img));
                        hostdir = Some(hd);
                    }
                    Err(e) => fail(&format!("cannot use folder: {e}")),
                }
            }
            m.board_mut().gpib.add_2101_floppy_slot();
            m.board_mut().bubble = Some(grid_core::bubble::Bubble::new(bub.unwrap_or_else(|| vec![0u8; 3 * grid_core::bubble::MBM_SIZE])));
            m
        }
    };
    m.board_mut().sound.enabled = false;
    m.board_mut().log_unhandled = false;
    if std::env::var("GRID_NO_CONTENTION").is_ok() {
        m.board_mut().display.contention = false;
    }
    if std::env::var("GRID_CONTENTION").is_ok() {
        m.board_mut().display.contention = true;
    }
    {
        // Display contention overrides (15 MHz states), as in gridfrag.
        let d = &mut m.board_mut().display;
        let env = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<u32>().ok());
        if let Some(v) = env("DISP_FETCH_PERIOD") {
            d.fetch_period = v;
        }
        if let Some(v) = env("DISP_FETCH_LEN") {
            d.fetch_len = v;
        }
        if let Some(v) = env("DISP_CPU_LEN") {
            d.cpu_len = v;
        }
        if let Some(v) = env("DISP_CPU_WRITE_LEN") {
            d.cpu_write_len = v;
        }
    }
    m.set_fast(!exact);
    let mut s = Session::new(m);

    if from_state {
        eprintln!("resumed from {}", state.as_ref().unwrap().display());
    }
    else {
        s.run_seconds(start);
    }
    let up = s.wait_prompt(max_s);
    if up && !from_state {
        if let Some(p) = &state {
            let tmp = p.with_extension("tmp");
            if fs::write(&tmp, s.m.save_state()).and_then(|_| fs::rename(&tmp, p)).is_ok() {
                eprintln!("state saved to {}", p.display());
            }
        }
    }
    let code = if !up {
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
        "Compass {} ({} core): {:.0} emulated seconds in {:.1} s ({:.0}x)",
        model.name(),
        if exact { "exact" } else { "fast" },
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
    if let Some(f) = &flop {
        if let Some(d) = b.gpib.drives.iter().find(|d| d.kind == DriveKind::Floppy2102 && d.dirty) {
            let _ = fs::write(f, &d.image);
        }
    }
    if let Some(hdir) = &hostdir {
        if let Some(d) = b.gpib.drives.iter().find(|d| d.kind == DriveKind::Floppy2101 && d.dirty) {
            match hdir.export(&d.image) {
                Ok(log) => log.iter().for_each(|l| eprintln!("folder: {l}")),
                Err(e) => eprintln!("could not write the folder back: {e}"),
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
