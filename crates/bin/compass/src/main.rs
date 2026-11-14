//! Compass-Emu 0.9.0: a GRiD Compass in a window.
//!
//! compass [options]
//!   --model M                    1101 (default), 1109, 1121, 1129, 1131 or 1139
//!   --prom-even F --prom-odd F   boot PROM chips (default: see default_prom)
//!   --prom F                     boot PROM as one 16K file (even/odd bytes interleaved)
//!   --kbd F                      8741 keyboard firmware (default roms/300067-01.bin; the
//!                                Compass II models use roms/300001-02.bin when it is there)
//!   --hd F                       2101 hard disk image (GPIB 4)
//!   --flop F                     2102 floppy image (GPIB 6)
//!   --flop2101 F                 2101 floppy image (GPIB 5)
//!   --folder D                   a PC folder as the 2101 "Floppy Disk" (GPIB 5): its files and
//!                                sub-folders (= subjects) are copied in at start, and files GRiD
//!                                made, changed or erased are written back when you quit
//!   --rom0 F .. --rom3 F         application ROM chip in socket 0-3 (Compass II models only; a
//!                                32K, 64K or 128K dump, e.g. from Rom Toolbox)
//!   --bubble F                   bubble memory image (384K; a new blank one is made if F is missing)
//!   --serial S                   serial port: tcp:PORT (listen on localhost), tcp:HOST:PORT,
//!                                connect:[HOST:]PORT (link to another emulator) or a port name (COM3)
//!   --gridlink S                 GRiDLink cable on the same port (8274 SDLC frames over TCP, see
//!                                docs/gridlink.md): tcp:PORT, tcp:HOST:PORT or connect:[HOST:]PORT
//!   --gridlink-p2p               GRiDLink point-to-point cable (RTS looped to CTS); default multidrop
//!   --mute                       no sound
//!   --readonly                   never write disk changes back to the image files
//!   --scale N                    window scale 1, 2 or 4 (default 4)
//!   --overlay                    start with the demo tools overlay on (F10)
//!   --fast                       fast CPU core (whole instructions, approximate timing)
//!   --classic                    the plain window of 0.4 (no menus) instead of the GUI
//!   --symbols D                  program symbols for the debugger: the project folder of a
//!                                debug build (gridbuild.py debug saves NAME.pas.obj files there)
//!   --no-catch                   crash catcher off (it stops on GRiD-OS errors, divide error,
//!                                INT 3 and wild jumps)
//! Without machine options the GUI starts at the machine setup screen.
//! Keys: PC keys map to the Compass keys by position; Alt = CODE, Ctrl = CTRL.
//!   Debugger: type commands in the console window (h = help), F8 = pause / go on.
//!   F9 = clear the cycle counter marks, F10 = demo tools overlay (marks, see
//!   grid_core::marks), F11 = save screenshot, F12 = turbo on/off, Esc = Compass ESC. Close the window to quit
//!   (changed disks are written back unless --readonly).

use std::path::PathBuf;

use grid_core::{screen, GridModel};
mod classic;
mod display;
mod gui;
mod headless;
mod host;
mod mcp;
mod runner;
mod session;
#[cfg(windows)]
mod winkeys;

use session::{MachineConfig, Session};

pub const VERSION: &str = "0.9.1";

fn usage() -> ! {
    eprintln!(
        "Compass-Emu {VERSION}\nusage: compass [--model 1101|1109|1121|1129|1131|1139] [--hd disk.img] [--flop floppy.img] [--flop2101 floppy.img] [--folder DIR] [--rom0..3 chip.bin] [--bubble bubble.img]\n       [--serial tcp:PORT|connect:[HOST:]PORT|COMn] [--gridlink tcp:PORT|connect:[HOST:]PORT] [--gridlink-p2p] [--mute] [--prom-even F --prom-odd F | --prom F] [--kbd F] [--readonly] [--scale N] [--overlay] [--fast] [--classic]\n       [--symbols DIR] [--no-catch]\nROM defaults are looked for in a roms folder next to the program.\nWithout machine options the window opens at the machine setup screen.\n       compass do [--model M] --hd disk.img [--shot F] [--max-seconds N] \"Do build\"   (headless, see headless.rs)"
    );
    std::process::exit(1)
}

/// The program's folder (where roms, profiles and the package files live), else ".".
pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_path(name: &str) -> PathBuf {
    for base in [exe_dir(), PathBuf::from(".")] {
        let p = base.join("roms").join(name);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("roms").join(name)
}

/// Options for the window itself (not part of a machine profile).
pub struct WindowOpts {
    pub scale: u32,
    pub overlay: bool,
}

/// After the window closed: the marks report, and the disks written back.
pub fn finish(s: &mut Session) {
    let b = s.m.board();
    if !b.marks.is_empty() {
        let r = b.marks.report(b.frame_clocks(), screen::CPU_HZ);
        print!("{r}");
        match std::fs::write("compass-marks.txt", &r) {
            Ok(_) => println!("saved compass-marks.txt"),
            Err(e) => eprintln!("could not save compass-marks.txt: {e}"),
        }
    }
    for l in s.write_back() {
        println!("{l}");
    }
}

fn main() {
    let all: Vec<String> = std::env::args().skip(1).collect();
    if all.first().map(|a| a == "do").unwrap_or(false) {
        std::process::exit(headless::run(all[1..].to_vec(), default_path("")));
    }
    if all.first().map(|a| a == "mcp").unwrap_or(false) {
        std::process::exit(mcp::run(default_path("")));
    }
    let mut args = all.into_iter();
    let mut cfg = MachineConfig { model: "1101".into(), ..Default::default() };
    let mut given = false;
    let mut classic = false;
    let mut opts = WindowOpts { scale: 4, overlay: false };
    while let Some(a) = args.next() {
        let mut val = || args.next().unwrap_or_else(|| usage());
        let mut machine = true;
        match a.as_str() {
            "--model" => {
                let v = val();
                cfg.model = GridModel::parse(&v)
                    .unwrap_or_else(|| {
                        eprintln!("unknown model {v} (1101, 1109, 1121, 1129, 1131, 1139)");
                        std::process::exit(1)
                    })
                    .name()
                    .to_string()
            }
            "--prom-even" => cfg.prom_even = val(),
            "--prom-odd" => cfg.prom_odd = val(),
            "--prom" => cfg.prom = val(),
            "--kbd" => cfg.kbd = val(),
            "--hd" => cfg.hd = val(),
            "--flop" => cfg.flop = val(),
            "--flop2101" => cfg.flop2101 = val(),
            "--folder" => cfg.folder = val(),
            "--rom0" | "--rom1" | "--rom2" | "--rom3" => cfg.roms[(a.as_bytes()[5] - b'0') as usize] = val(),
            "--readonly" => cfg.readonly = true,
            "--bubble" => cfg.bubble = val(),
            "--serial" => cfg.serial = val(),
            "--gridlink" => cfg.gridlink = val(),
            "--gridlink-p2p" => cfg.gridlink_p2p = true,
            "--mute" => cfg.mute = true,
            "--fast" => cfg.fast = true,
            "--symbols" => cfg.symbols = val(),
            "--no-catch" => cfg.no_catch = true,
            "--overlay" => {
                opts.overlay = true;
                machine = false;
            }
            "--scale" => {
                opts.scale = val().parse().unwrap_or(4);
                machine = false;
            }
            "--classic" => {
                classic = true;
                machine = false;
            }
            _ => usage(),
        }
        given |= machine;
    }
    if classic {
        let s = Session::build(&cfg, &default_path("")).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1)
        });
        classic::run(s, opts.scale, opts.overlay);
        return;
    }
    let start = given.then_some(cfg.clone());
    let gui_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| gui::run(start, &opts)))
        .unwrap_or_else(|_| Err("the window system could not be opened".into()));
    if let Err(e) = gui_result {
        // No OpenGL 2 (an old graphics driver or a virtual machine): fall back to the plain window.
        eprintln!("the GUI could not start ({e}); using the classic window");
        if !given {
            eprintln!("start it with machine options (see compass --help) or from one of the .bat files");
            std::process::exit(1);
        }
        let s = Session::build(&cfg, &default_path("")).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1)
        });
        classic::run(s, opts.scale, opts.overlay);
    }
}
