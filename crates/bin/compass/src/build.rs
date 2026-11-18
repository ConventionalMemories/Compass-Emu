//! Build a GRiD program from the GUI (Project > Build), the same way gridbuild.py does but
//! inside compass.exe: the old GRiD Pascal compiler, ASM86 and linker run on a hidden Compass
//! (fast core) booted from a copy of a development disk, on its own thread, while the
//! machine on screen keeps running.
//!
//! A project is a folder NAME holding NAME.pas (the main program), any other .pas files
//! (Pascal modules) and .asm files (assembler modules). The results land in the folder:
//! NAME.run, NAME.lst (and MODULE.lst / MODULE.asm.lst), screen.png (the build machine's
//! screen at the end), and for a debug build the .obj files and NAME.map for the debugger.
//!
//! The development disk must hold the GRiD tools (Programs: Pascal, Asm, Link,
//! DevelopmentExecutive; libs: CompactSystemCalls, CompactCpCalls, 8087, dqlarge), like the
//! merged dev disk. The build runs on a 1129 for a disk set up for the 11xx (the disk the
//! 1101 .bat makes) and on a 1139 for a 113x disk. The first build of a disk boots it and
//! keeps a saved state in the states folder next to compass.exe; later builds start there.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{channel, Receiver, Sender},
};

use grid_core::{
    gpib::{Drive, DriveKind},
    hostdir::{self, Volume, PAGE},
    session::{png_bytes, Session},
    GridModel,
};

/// What to build and with what.
#[derive(Clone)]
pub struct BuildJob {
    /// The project folder.
    pub project: PathBuf,
    /// The development disk image (only read; the build works on a copy).
    pub disk: PathBuf,
    /// The ROM folder for the hidden build machine.
    pub roms: PathBuf,
    /// Compile with the DEBUG option and keep the symbols (.obj, .map).
    pub debug: bool,
    /// Cycle-exact core (much slower, same results).
    pub exact: bool,
}

/// Messages from the build thread.
pub enum BuildMsg {
    Line(String),
    Done(BuildResult),
}

pub struct BuildResult {
    pub ok: bool,
    /// NAME.run when it built.
    pub run: Option<PathBuf>,
    /// The program's GRiD name (NAME).
    pub name: String,
    pub errors: Vec<String>,
}

/// Start a build on its own thread.
pub fn spawn(job: BuildJob) -> Receiver<BuildMsg> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("grid build".into())
        .spawn(move || {
            let name = job.project.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let res = match build(&job, &tx) {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(BuildMsg::Line(e.clone()));
                    BuildResult { ok: false, run: None, name, errors: vec![e] }
                }
            };
            let _ = tx.send(BuildMsg::Done(res));
        })
        .expect("build thread");
    rx
}

fn say(tx: &Sender<BuildMsg>, s: impl Into<String>) {
    let _ = tx.send(BuildMsg::Line(s.into()));
}

const HD: &str = "`'Hard Disk X'`";

fn valid_name(n: &str) -> bool {
    let mut c = n.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic()) && n.len() <= 31 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The project's main program name and its modules (Pascal, assembler).
pub fn project_parts(proj: &Path) -> Result<(String, Vec<String>, Vec<String>), String> {
    let folder = proj.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut pas = vec![];
    let mut asm = vec![];
    for e in fs::read_dir(proj).map_err(|e| format!("{}: {e}", proj.display()))?.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        match p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).as_deref() {
            Some("pas") => pas.push(stem),
            Some("asm") => asm.push(stem),
            _ => {}
        }
    }
    pas.sort();
    asm.sort();
    let main = pas
        .iter()
        .find(|p| p.eq_ignore_ascii_case(&folder))
        .cloned()
        .or_else(|| (pas.len() == 1).then(|| pas[0].clone()))
        .ok_or_else(|| format!("{} needs {folder}.pas (the main program, named after the folder)", proj.display()))?;
    for n in pas.iter().chain(asm.iter()) {
        if !valid_name(n) {
            return Err(format!("{n}: name source files with letters, digits and _ only (up to 31)"));
        }
    }
    let mods = pas.into_iter().filter(|p| *p != main).collect();
    Ok((main, mods, asm))
}

/// The build script typed on the Development Executive (no test run: the program runs on the
/// machine on screen instead).
fn script(n: &str, pmods: &[String], amods: &[String], debug: bool) -> Vec<String> {
    let dbg = if debug { " debug" } else { "" };
    let map = if debug { format!(" print({HD}Projects`{n}~MP1~)") } else { String::new() };
    let mut s = vec![format!("Prefix {HD}Projects")];
    if pmods.is_empty() && amods.is_empty() {
        s.push(format!("{HD}Programs`Pascal {HD}Projects`{n}.pas~Text~{dbg}"));
        s.push(format!(
            "{HD}Programs`Link {HD}Projects`{n}.pas~obj~, {HD}libs`CompactSystemCalls~lib~, {HD}libs`dqlarge~lnk~ To {HD}Projects`{n}~Run~ bind ss(stack(+1500)) purge{map} fl pu"
        ));
        return s;
    }
    for m in amods {
        s.push(format!("{HD}Programs`Asm {HD}Projects`{m}.asm~Text~{dbg}"));
    }
    for m in pmods {
        s.push(format!("{HD}Programs`Pascal {HD}Projects`{m}.pas~Text~{dbg}"));
    }
    s.push(format!("{HD}Programs`Pascal {HD}Projects`{n}.pas~Text~{dbg}"));
    // Pascal modules by their short names (the prefix is Projects) to keep the line short
    let objs: String = pmods.iter().map(|m| format!("{m}.pas~obj~, ")).chain(amods.iter().map(|m| format!("{HD}Projects`{m}.asm~obj~, "))).collect();
    s.push(format!(
        "{HD}Programs`Link {HD}Projects`{n}.pas~obj~, {objs}{HD}libs`CompactCpCalls~lib~, {HD}libs`CompactSystemCalls~lib~, {HD}libs`8087~lib~, {HD}libs`dqlarge~lnk~ To {HD}Projects`{n}~Run~ bind ss(stack(+1500)) purge{map} fl pu"
    ));
    s
}

/// Work on the first GRiD volume of a hard disk image.
fn with_volume<T>(img: &mut [u8], f: impl FnOnce(&mut Volume) -> Result<T, String>) -> Result<T, String> {
    let (st, n) = *hostdir::volumes(img).first().ok_or("the development disk has no GRiD volume")?;
    let (a, b) = (st as usize * PAGE, (st + n) as usize * PAGE);
    let mut v = Volume::open_any(img[a..b].to_vec()).map_err(|e| e.0)?;
    let r = f(&mut v)?;
    img[a..b].copy_from_slice(&v.img);
    Ok(r)
}

fn find(v: &Volume, subject: &str, name: &str) -> Option<(u32, u32)> {
    let dir = v.subject_dir(subject).ok()?;
    v.entries(dir).ok()?.into_iter().find(|e| e.1.eq_ignore_ascii_case(name)).map(|e| (dir, e.0))
}

fn get(v: &Volume, subject: &str, name: &str) -> Option<Vec<u8>> {
    find(v, subject, name).and_then(|(_, fid)| v.read(fid).ok())
}

fn names(v: &Volume, subject: &str) -> Vec<String> {
    v.subject_dir(subject).ok().and_then(|d| v.entries(d).ok()).map(|e| e.into_iter().map(|x| x.1).collect()).unwrap_or_default()
}

/// The model that runs the build: 1129 for a disk set up for the 11xx, 1139 for a 113x one.
fn build_model(img: &mut [u8]) -> Result<GridModel, String> {
    with_volume(img, |v| {
        let cur = get(v, "Programs", "CCOS~System~").ok_or("the development disk has no Programs:CCOS~System~")?;
        if get(v, "Programs", "Pascal~Run~").is_none() || get(v, "Programs", "DevelopmentExecutive~Run~").is_none() {
            return Err("the development disk has no GRiD Pascal compiler or Development Executive (use the merged dev disk)".into());
        }
        let wide = get(v, "Programs", "CCOS 113x 3.1.5~System~").is_some_and(|d| d == cur);
        Ok(if wide { GridModel::Compass1139 } else { GridModel::Compass1129 })
    })
}

fn hash(data: &[u8], extra: &str) -> u64 {
    // FNV-1a over the disk and the emulator version
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data.iter().chain(extra.as_bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn new_machine(model: GridModel, roms: &Path, img: Vec<u8>, exact: bool) -> Result<grid_core::GridMachine, String> {
    let mut m = grid_core::roms::machine(model, roms)?;
    m.board_mut().gpib.drives.push(Drive::new(DriveKind::Hdd2101, img));
    m.board_mut().gpib.add_2101_floppy_slot();
    m.board_mut().bubble = Some(grid_core::bubble::Bubble::new(vec![0u8; 3 * grid_core::bubble::MBM_SIZE]));
    m.board_mut().sound.enabled = false;
    m.board_mut().log_unhandled = false;
    m.set_fast(!exact);
    Ok(m)
}

fn build(job: &BuildJob, tx: &Sender<BuildMsg>) -> Result<BuildResult, String> {
    let proj = &job.project;
    let (n, pmods, amods) = project_parts(proj)?;
    let mut disk = fs::read(&job.disk).map_err(|e| format!("cannot read the development disk {}: {e}", job.disk.display()))?;
    let model = build_model(&mut disk)?;
    let free = with_volume(&mut disk, |v| Ok(v.free_pages()))?;
    if free < 2000 {
        say(tx, format!("Note: the development disk has only {} KB free; the compiler needs room for its work files", free / 2));
    }
    let max_s = 10800.0;

    // A saved state at the command line prompt, per disk, model and emulator version.
    let states = crate::data_dir().join("states");
    // (the boot PROM is part of the key: a stand-in PROM boots a different machine)
    let found = grid_core::roms::find(model, &job.roms);
    let prom: Vec<u8> = [&found.prom_even, &found.prom_odd].iter().flat_map(|p| fs::read(p).unwrap_or_default()).collect();
    let key = format!("gui-{}-{:016x}", model.name(), hash(&disk, &format!("{} {} {:016x}", model.name(), crate::VERSION, hash(&prom, ""))));
    let (state_file, base_file) = (states.join(format!("{key}.state")), states.join(format!("{key}.img")));
    let resumed = match (fs::read(&state_file), fs::read(&base_file)) {
        (Ok(st), Ok(base)) => grid_core::GridMachine::load_state(&st).ok().map(|m| (m, base)),
        _ => None,
    };
    let (mut s, mut img) = match resumed {
        Some((mut m, base)) => {
            say(tx, format!("Starting the build Compass {} from its saved state", model.name()));
            m.set_fast(!job.exact);
            m.board_mut().sound.enabled = false;
            (Session::new(m), base)
        }
        None => {
            // boot straight to the command line instead of the Executive
            let mut base = disk.clone();
            with_volume(&mut base, |v| {
                let dev = get(v, "Programs", "DevelopmentExecutive~Run~").ok_or("no DevelopmentExecutive~Run~")?;
                v.put_file("Programs", "Executive~Run~", &dev).map_err(|e| e.0)
            })?;
            say(tx, format!("Booting the build Compass {} (first build of this disk, about a minute)", model.name()));
            let m = new_machine(model, &job.roms, base.clone(), job.exact)?;
            let mut s = Session::new(m);
            s.run_seconds(20.0);
            if !s.wait_prompt(600.0) {
                return Err("the build Compass did not reach the command line prompt (is it a development disk?)".into());
            }
            let _ = fs::create_dir_all(&states);
            // drop older GUI states of this model
            if let Ok(rd) = fs::read_dir(&states) {
                for e in rd.flatten() {
                    let f = e.file_name().to_string_lossy().to_string();
                    if f.starts_with(&format!("gui-{}-", model.name())) && !f.starts_with(&key) {
                        let _ = fs::remove_file(e.path());
                    }
                }
            }
            let _ = fs::write(&state_file, s.m.save_state());
            let _ = fs::write(&base_file, &base);
            (s, base)
        }
    };

    // Sources in, old outputs out, the build script in.
    let mut outputs = vec![format!("{n}.pas~Obj~"), format!("{n}.pas~Lst~"), format!("{n}~Run~"), format!("{n}~MP1~")];
    outputs.extend(pmods.iter().flat_map(|m| [format!("{m}.pas~Obj~"), format!("{m}.pas~Lst~")]));
    outputs.extend(amods.iter().flat_map(|m| [format!("{m}.asm~Obj~"), format!("{m}.asm~Lst~")]));
    let text = |p: PathBuf| fs::read(&p).map(|d| hostdir::to_crlf(&d)).map_err(|e| format!("{}: {e}", p.display()));
    let mut put: Vec<(String, Vec<u8>)> = vec![(format!("{n}.pas~Text~"), text(proj.join(format!("{n}.pas")))?)];
    for m in &pmods {
        put.push((format!("{m}.pas~Text~"), text(proj.join(format!("{m}.pas")))?));
    }
    for m in &amods {
        put.push((format!("{m}.asm~Text~"), text(proj.join(format!("{m}.asm")))?));
    }
    let lines = script(&n, &pmods, &amods, job.debug);
    let before = with_volume(&mut img, |v| {
        if v.subject_dir("Projects").is_err() {
            v.add_subject("Projects").map_err(|e| e.0)?;
        }
        for o in &outputs {
            if let Some((dir, fid)) = find(v, "Projects", o) {
                v.delete(dir, fid).map_err(|e| e.0)?;
            }
        }
        for (name, data) in &put {
            v.put_file("Projects", name, data).map_err(|e| format!("{name}: {}", e.0))?;
        }
        v.put_file("Programs", "build~Com~", &hostdir::to_crlf((lines.join("\n") + "\n").as_bytes())).map_err(|e| e.0)?;
        Ok(names(v, "Projects").into_iter().map(|x| x.to_ascii_uppercase()).collect::<Vec<_>>())
    })?;
    match s.m.board_mut().gpib.drives.iter_mut().find(|d| d.kind == DriveKind::Hdd2101) {
        Some(d) => d.image = img,
        None => return Err("the build machine has no hard disk".into()),
    }
    let files: Vec<String> = put.iter().map(|p| p.0.replace("~Text~", "")).collect();
    say(tx, format!("Compiling {} ...", files.join(", ")));
    let wall = std::time::Instant::now();
    s.type_keys("Do build{ret}")?;
    let done = s.wait_prompt_back(max_s);
    say(tx, format!("({:.0} emulated seconds in {:.1} s)", s.seconds(), wall.elapsed().as_secs_f64()));
    let (w, h, px) = s.screen();
    let _ = fs::write(proj.join("screen.png"), png_bytes(w, h, &px));
    let mut img = s.m.board().gpib.drives.iter().find(|d| d.kind == DriveKind::Hdd2101).map(|d| d.image.clone()).unwrap_or_default();

    // Results back to the project folder.
    let mut errors = vec![];
    let mut failed = vec![];
    let run = proj.join(format!("{n}.run"));
    let _ = fs::remove_file(&run);
    let built = with_volume(&mut img, |v| {
        let after = names(v, "Projects");
        let has = |f: &str| after.iter().any(|a| a.eq_ignore_ascii_case(f));
        let mut listing = |grid: &str, host: String, asm: bool, label: &str| -> bool {
            let Some(d) = get(v, "Projects", grid) else { return false };
            let _ = fs::write(proj.join(&host), &d);
            let t = String::from_utf8_lossy(&d);
            let mut bad = false;
            for l in t.lines() {
                if (!asm && l.starts_with("***")) || (asm && l.contains("***")) {
                    errors.push(format!("{label}: {}", l.trim_end()));
                    bad = true;
                }
                if l.contains("Errors Detected") || l.to_ascii_uppercase().contains("ASSEMBLY COMPLETE") {
                    errors.push(format!("{label}: {}", l.trim()));
                    if asm && !l.to_ascii_uppercase().contains("NO ERRORS") {
                        bad = true;
                    }
                }
            }
            !bad
        };
        for m in &amods {
            if !listing(&format!("{m}.asm~Lst~"), format!("{m}.asm.lst"), true, &format!("{m}.asm")) {
                failed.push(format!("{m}.asm"));
            }
        }
        for m in &pmods {
            if !listing(&format!("{m}.pas~Lst~"), format!("{m}.lst"), false, &format!("{m}.pas")) {
                failed.push(format!("{m}.pas"));
            }
        }
        if !listing(&format!("{n}.pas~Lst~"), format!("{n}.lst"), false, &format!("{n}.pas")) {
            if !has(&format!("{n}.pas~Lst~")) {
                errors.push(format!("{n}.pas: the compiler made no listing (see screen.png)"));
            }
            failed.push(format!("{n}.pas"));
        }
        let built = get(v, "Projects", &format!("{n}~Run~"));
        if job.debug {
            for f in &after {
                if f.to_ascii_lowercase().ends_with("~obj~") {
                    if let Some(d) = get(v, "Projects", f) {
                        let host = format!("{}.obj", &f[..f.len() - 5]).to_ascii_lowercase();
                        let _ = fs::write(proj.join(host), d);
                    }
                }
                else if f.eq_ignore_ascii_case(&format!("{n}~MP1~")) {
                    if let Some(d) = get(v, "Projects", f) {
                        let _ = fs::write(proj.join(format!("{n}.map")), d);
                    }
                }
            }
        }
        // files the tools or the program wrote that aren't known outputs
        let known: Vec<String> = outputs.iter().chain(put.iter().map(|p| &p.0)).map(|o| o.to_ascii_uppercase()).collect();
        for f in after.iter().filter(|f| !before.contains(&f.to_ascii_uppercase()) && !known.contains(&f.to_ascii_uppercase())) {
            if let Some(d) = get(v, "Projects", f) {
                let out = proj.join("output");
                let _ = fs::create_dir_all(&out);
                let _ = fs::write(out.join(hostdir::host_name(f)), d);
            }
        }
        Ok(built)
    })?;
    let ok = failed.is_empty() && built.is_some();
    if let Some(d) = &built {
        if ok {
            fs::write(&run, d).map_err(|e| format!("{}: {e}", run.display()))?;
        }
    }
    for e in &errors {
        say(tx, e.clone());
    }
    if errors.iter().any(|e| e.contains("ERROR 9010") || e.contains("ERROR 9011")) {
        say(tx, "Error 9010/9011 (I/O error on compiler work file) means the development disk is too full: erase some files from it.");
    }
    if done.is_none() {
        say(tx, "The build did not finish in time (see screen.png).");
    }
    if ok {
        say(tx, format!("BUILD OK: {}", run.display()));
        if job.debug {
            say(tx, "Symbols saved (.obj, .map): Debug > Load program symbols and pick the project folder.");
        }
    }
    else if !failed.is_empty() {
        say(tx, format!("BUILD FAILED in {} (errors in the .lst files next to the source)", failed.join(", ")));
    }
    else {
        say(tx, format!("BUILD FAILED: no {n}~Run~ (linker messages are in screen.png)"));
    }
    Ok(BuildResult { ok, run: ok.then_some(run), name: n, errors })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_match_gridbuild() {
        let s = script("hello", &[], &[], false);
        assert_eq!(s[0], "Prefix `'Hard Disk X'`Projects");
        assert_eq!(s[1], "`'Hard Disk X'`Programs`Pascal `'Hard Disk X'`Projects`hello.pas~Text~");
        assert!(s[2].ends_with("To `'Hard Disk X'`Projects`hello~Run~ bind ss(stack(+1500)) purge fl pu"));
        let s = script("game", &["gmod".into()], &["gasm".into()], true);
        assert_eq!(s[1], "`'Hard Disk X'`Programs`Asm `'Hard Disk X'`Projects`gasm.asm~Text~ debug");
        assert!(s[4].contains("game.pas~obj~, gmod.pas~obj~, `'Hard Disk X'`Projects`gasm.asm~obj~, `'Hard Disk X'`libs`CompactCpCalls~lib~"));
        assert!(s[4].contains("purge print(`'Hard Disk X'`Projects`game~MP1~) fl pu"));
    }

    #[test]
    fn names() {
        assert!(valid_name("hello_2"));
        assert!(!valid_name("2hello"));
        assert!(!valid_name("he-llo"));
    }
}
