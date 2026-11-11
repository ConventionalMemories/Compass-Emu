//! `compass mcp`: the Claude Code hook. An MCP server on stdin/stdout (JSON-RPC 2.0, one
//! message per line) with tools to build GRiD programs, drive an emulated Compass (boot,
//! type, run, screenshots, memory) and time code fragments.
//!
//! Claude Code: `claude mcp add compass -- C:\GRiD-MAME\compass-emu\compass.exe mcp`
//! (or an .mcp.json entry with command compass.exe and args ["mcp"]).
//!
//! Environment: GRID_ROOT = the GRiD-MAME folder (default: the folder above compass.exe, when
//! it has tools\gridbuild.py), GRID_MODEL = default model (1129).

use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

use grid_core::{
    gpib::{Drive, DriveKind},
    session::{png_bytes, Session},
    GridModel,
};
use marty_core::cpu_common::Cpu;
use serde_json::{json, Value};

const PROTOCOL: &str = "2024-11-05";

struct Server {
    roms: PathBuf,
    session: Option<Session>,
    hd_path: Option<PathBuf>,
    default_model: GridModel,
}

fn tools() -> Value {
    let model = json!({"type": "string", "description": "1101, 1109, 1121, 1129, 1131 or 1139 (default 1129)"});
    json!([
        {
            "name": "grid_build",
            "description": "Compile, link and test-run a GRiD Pascal (+ ASM-86) program with the GRiD's own tools on an emulated Compass (gridbuild.py build). SOURCE is a .pas file, a project folder or a project name (\"hello\" = projects\\hello\\hello.pas). Returns the build messages (compiler errors, BUILD OK/FAILED) and the GRiD screen at the end. NAME.lst, NAME.run, screen.png and output\\ land next to the source.",
            "inputSchema": {"type": "object", "properties": {"source": {"type": "string"}, "model": model}, "required": ["source"]}
        },
        {
            "name": "grid_boot",
            "description": "Start an emulated Compass from a 2101 hard disk image (works on a copy in memory; the file is only changed by grid_save_disk). Runs until the GRiD-OS command line prompt or Executive is steady (or `seconds`), then returns the screen.",
            "inputSchema": {"type": "object", "properties": {
                "hd": {"type": "string", "description": "hard disk image file"},
                "model": model,
                "seconds": {"type": "number", "description": "run exactly this many emulated seconds instead of waiting for a prompt"},
                "max_seconds": {"type": "number", "description": "limit when waiting for the prompt (default 300)"},
                "fast": {"type": "boolean", "description": "fast CPU core (about 5x quicker, approximate timing: fine for builds and file work, not for timing or racing the beam). Default false"}
            }, "required": ["hd"]}
        },
        {
            "name": "grid_type",
            "description": "Type on the Compass keyboard. Plain text plus {ret} {esc} {tab} {bs} {space} {left} {right} {up} {down} {code+x} {ctrl+x} {shift+x}; a newline is Return. With wait_prompt, waits until a new command line prompt (=>) appears (for Development Executive commands), else runs `seconds` (default 2). Returns the screen.",
            "inputSchema": {"type": "object", "properties": {
                "text": {"type": "string"},
                "wait_prompt": {"type": "boolean"},
                "seconds": {"type": "number"},
                "max_seconds": {"type": "number", "description": "limit for wait_prompt (default 600)"}
            }, "required": ["text"]}
        },
        {
            "name": "grid_run",
            "description": "Let the emulated Compass run for some emulated seconds, then return the screen.",
            "inputSchema": {"type": "object", "properties": {"seconds": {"type": "number"}}, "required": ["seconds"]}
        },
        {
            "name": "grid_screen",
            "description": "The Compass screen now (PNG) plus emulated time, CS:IP and screen mode.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "grid_read_memory",
            "description": "Hex dump of Compass memory. address: SSSS:OOOO or a linear hex address (e.g. 0400 or 2000:0010).",
            "inputSchema": {"type": "object", "properties": {"address": {"type": "string"}, "length": {"type": "integer", "description": "bytes, at most 4096 (default 256)"}}, "required": ["address"]}
        },
        {
            "name": "grid_save_disk",
            "description": "Write the running Compass's hard disk (with the changes GRiD-OS made) to a file.",
            "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}
        },
        {
            "name": "grid_debug",
            "description": "Debugger and cycle counter on the running Compass (after grid_boot). command is one of: b ADDR (code breakpoint, SSSS:OOOO or linear hex), bio PORT, bi N (hardware interrupt IR0-7, 3 = vertical sync), bw ADDR [LEN] (memory write watch), bl, bc, s [N] (step N instructions, shows clocks), r, m ADDR [LEN], u [SSSS:OOOO] [N] (disassemble), marks (cycle counter report: programs mark sections with OUT 0F0h,AL / OUT 0F2h,AL and frames with OUT 0F4h,AL; ports a real Compass ignores), marks-clear, g [SECONDS] (run until a breakpoint or SECONDS emulated seconds, default 10; returns the reason and the screen).",
            "inputSchema": {"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}
        },
        {
            "name": "grid_fragment",
            "description": "Time a raw 8086/8087 machine code fragment on an emulated Compass (cycle-accurate, with display RAM contention): loaded at 1000:0000, DS=ES=2000h, SS:SP=3000:FFFE, interrupts off, runs until it reaches a HLT (F4, not executed). Returns CPU clocks (5 MHz), instructions and registers. Assemble with NASM (bits 16, org 0) and pass the bytes as hex, or give a .bin file.",
            "inputSchema": {"type": "object", "properties": {
                "code_hex": {"type": "string"},
                "bin_file": {"type": "string"},
                "model": model,
                "max_clocks": {"type": "integer", "description": "default 100000000 (20 s)"}
            }}
        }
    ])
}

pub fn run(roms: PathBuf) -> i32 {
    let default_model = std::env::var("GRID_MODEL").ok().and_then(|m| GridModel::parse(&m)).unwrap_or(GridModel::Compass1129);
    // ROMs: the roms folder next to compass.exe, else GRID_ROOT\compass-emu\roms
    let roms = if roms.join("300067-01.bin").exists() {
        roms
    }
    else {
        grid_root().map(|r| r.join("compass-emu").join("roms")).filter(|r| r.exists()).unwrap_or(roms)
    };
    let mut srv = Server { roms, session: None, hd_path: None, default_model };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}}));
                let _ = out.flush();
                continue;
            }
        };
        let Some(id) = msg.get("id").cloned() else { continue }; // notification
        let method = msg["method"].as_str().unwrap_or("");
        let reply = match method {
            "initialize" => Ok(json!({
                "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "compass-emu", "version": crate::VERSION},
                "instructions": "Compass-Emu: a cycle-accurate GRiD Compass emulator. grid_build builds GRiD Pascal/ASM-86 programs with the GRiD's own compiler. grid_boot/grid_type/grid_run/grid_screen drive an emulated Compass; grid_fragment times 8086 code in CPU clocks."
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "tools/call" => {
                let name = msg["params"]["name"].as_str().unwrap_or("").to_string();
                let args = msg["params"]["arguments"].clone();
                Ok(match srv.call(&name, &args) {
                    Ok(content) => json!({"content": content}),
                    Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
                })
            }
            _ => Err(json!({"code": -32601, "message": format!("unknown method {method}")})),
        };
        let resp = match reply {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
        };
        let _ = writeln!(out, "{resp}");
        let _ = out.flush();
    }
    0
}

fn text(s: impl Into<String>) -> Value {
    json!({"type": "text", "text": s.into()})
}

fn image(png: &[u8]) -> Value {
    json!({"type": "image", "data": base64::encode(png), "mimeType": "image/png"})
}

fn num(a: &Value, k: &str) -> Option<f64> {
    a.get(k).and_then(|v| v.as_f64())
}

impl Server {
    fn model(&self, a: &Value) -> Result<GridModel, String> {
        match a.get("model").and_then(|v| v.as_str()) {
            Some(m) => GridModel::parse(m).ok_or_else(|| format!("unknown model {m}")),
            None => Ok(self.default_model),
        }
    }

    fn session(&mut self) -> Result<&mut Session, String> {
        self.session.as_mut().ok_or_else(|| "no Compass running: call grid_boot first".to_string())
    }

    fn screen(&mut self, note: &str) -> Result<Vec<Value>, String> {
        let s = self.session()?;
        let (w, h, px) = s.screen();
        let (cs, ip) = s.m.cs_ip();
        let mode = s.m.screen_mode();
        let info = format!(
            "{note}Compass {} at {:.1} emulated s, CS:IP {cs:04X}:{ip:04X}, screen {}x{}",
            s.m.model.name(),
            s.seconds(),
            mode.0,
            mode.1
        );
        Ok(vec![text(info), image(&png_bytes(w, h, &px))])
    }

    fn call(&mut self, name: &str, a: &Value) -> Result<Vec<Value>, String> {
        match name {
            "grid_build" => self.build(a),
            "grid_boot" => {
                let model = self.model(a)?;
                let hd = PathBuf::from(a["hd"].as_str().ok_or("hd is needed")?);
                let img = std::fs::read(&hd).map_err(|e| format!("cannot read {}: {e}", hd.display()))?;
                let mut m = grid_core::roms::machine(model, &self.roms)?;
                m.board_mut().gpib.drives.push(Drive::new(DriveKind::Hdd2101, img));
                m.board_mut().bubble = Some(grid_core::bubble::Bubble::new(vec![0u8; 3 * grid_core::bubble::MBM_SIZE]));
                m.board_mut().sound.enabled = false;
                m.board_mut().log_unhandled = false;
                m.set_fast(a.get("fast").and_then(|v| v.as_bool()).unwrap_or(false));
                let mut s = Session::new(m);
                let note = if let Some(t) = num(a, "seconds") {
                    s.run_seconds(t);
                    String::new()
                }
                else if s.wait_prompt(num(a, "max_seconds").unwrap_or(300.0)) {
                    "Command line prompt is up. ".into()
                }
                else {
                    "No command line prompt (the Executive menu, or still booting). ".into()
                };
                self.session = Some(s);
                self.hd_path = Some(hd);
                self.screen(&note)
            }
            "grid_type" => {
                let t = a["text"].as_str().ok_or("text is needed")?.to_string();
                let s = self.session()?;
                if a["wait_prompt"].as_bool().unwrap_or(false) {
                    // remember the prompt that is up now, then type
                    s.wait_prompt(5.0);
                    s.type_keys(&t)?;
                    let note = match s.wait_prompt_back(num(a, "max_seconds").unwrap_or(600.0)) {
                        Some(sec) => format!("Prompt back after {sec:.0} emulated s. "),
                        None => "The prompt did not come back in time. ".into(),
                    };
                    self.screen(&note)
                }
                else {
                    s.type_keys(&t)?;
                    while s.typing() {
                        s.run_seconds(0.1);
                    }
                    s.run_seconds(num(a, "seconds").unwrap_or(2.0));
                    self.screen("")
                }
            }
            "grid_run" => {
                let t = num(a, "seconds").ok_or("seconds is needed")?;
                self.session()?.run_seconds(t);
                self.screen("")
            }
            "grid_screen" => self.screen(""),
            "grid_read_memory" => {
                let addr = parse_addr(a["address"].as_str().ok_or("address is needed")?)?;
                let len = a.get("length").and_then(|v| v.as_u64()).unwrap_or(256).min(4096) as usize;
                let s = self.session()?;
                let mut dump = String::new();
                for row in (0..len).step_by(16) {
                    let base = addr + row;
                    let bytes: Vec<u8> = (0..16.min(len - row)).map(|i| s.m.cpu.bus().peek_u8((base + i) & 0xFFFFF).unwrap_or(0xFF)).collect();
                    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
                    let asc: String = bytes.iter().map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { '.' }).collect();
                    dump.push_str(&format!("{:05X}  {:<47}  {asc}\n", base & 0xFFFFF, hex.join(" ")));
                }
                Ok(vec![text(dump)])
            }
            "grid_save_disk" => {
                let p = PathBuf::from(a["path"].as_str().ok_or("path is needed")?);
                let s = self.session()?;
                let d = s.m.board().gpib.drives.first().ok_or("no disk")?;
                std::fs::write(&p, &d.image).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
                Ok(vec![text(format!("disk written to {}", p.display()))])
            }
            "grid_debug" => {
                let cmd = a["command"].as_str().ok_or("command is needed")?.trim().to_string();
                let s = self.session()?;
                let mut words = cmd.split_whitespace();
                if matches!(words.next(), Some("g") | Some("go") | Some("c")) {
                    let secs: f64 = words.next().and_then(|t| t.parse().ok()).unwrap_or(10.0);
                    let end = s.m.board().clocks + (secs * 5_000_000.0) as u64;
                    let mut why = None;
                    while why.is_none() && s.m.board().clocks < end {
                        let left = end - s.m.board().clocks;
                        why = s.m.run_debug(left.min(500_000));
                    }
                    let note = match why {
                        Some(w) => {
                            let (cs, ip) = s.m.cs_ip();
                            format!("{w}\n{}\n{}", s.m.reg_dump(), s.m.disassemble(cs, ip, 3))
                        }
                        None => format!("no breakpoint in {secs} emulated seconds\n"),
                    };
                    return self.screen(&note);
                }
                let (out, _) = s.m.debug_command(&cmd, true);
                Ok(vec![text(out)])
            }
            "grid_fragment" => {
                let code = if let Some(h) = a.get("code_hex").and_then(|v| v.as_str()) {
                    let h: String = h.chars().filter(|c| c.is_ascii_hexdigit()).collect();
                    if h.len() % 2 != 0 {
                        return Err("code_hex has an odd number of digits".into());
                    }
                    (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap()).collect()
                }
                else if let Some(f) = a.get("bin_file").and_then(|v| v.as_str()) {
                    std::fs::read(f).map_err(|e| format!("cannot read {f}: {e}"))?
                }
                else {
                    return Err("give code_hex or bin_file".into());
                };
                let model = self.model(a)?;
                let max = a.get("max_clocks").and_then(|v| v.as_u64()).unwrap_or(100_000_000);
                let r = grid_core::fragment::run(model, &code, max);
                Ok(vec![text(format!(
                    "{}clocks {} ({:.2} us at 5 MHz), instructions {}\n{}",
                    if r.timed_out { "NO HLT REACHED (clock limit). " } else { "" },
                    r.clocks,
                    r.clocks as f64 / 5.0,
                    r.instructions,
                    r.registers
                ))])
            }
            _ => Err(format!("unknown tool {name}")),
        }
    }

    fn build(&mut self, a: &Value) -> Result<Vec<Value>, String> {
        let source = a["source"].as_str().ok_or("source is needed")?;
        let model = self.model(a)?;
        let root = grid_root().ok_or("cannot find the GRiD-MAME folder with tools\\gridbuild.py: set GRID_ROOT")?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let start = SystemTime::now();
        let mut out = None;
        for py in ["python", "py", "python3"] {
            let r = std::process::Command::new(py)
                .arg(root.join("tools").join("gridbuild.py"))
                .arg("build")
                .arg(source)
                .current_dir(&root)
                .env("GRID_EMU", &exe)
                .env("GRID_MODEL", model.name())
                .stdin(std::process::Stdio::null())
                .output();
            if let Ok(o) = r {
                out = Some(o);
                break;
            }
        }
        let o = out.ok_or("cannot start Python (python, py or python3)")?;
        let mut msg = String::from_utf8_lossy(&o.stdout).to_string();
        msg.push_str(&String::from_utf8_lossy(&o.stderr));
        let mut content = vec![text(msg.trim().to_string())];
        // the screen at the end, next to the source
        let dir = project_dir(&root, source);
        let shot = dir.join("screen.png");
        if let Ok(meta) = std::fs::metadata(&shot) {
            if meta.modified().map(|t| t >= start).unwrap_or(false) {
                if let Ok(png) = std::fs::read(&shot) {
                    content.push(image(&png));
                }
            }
        }
        Ok(content)
    }
}

/// SSSS:OOOO or a linear hex address.
fn parse_addr(s: &str) -> Result<usize, String> {
    let s = s.trim().trim_end_matches(['h', 'H']);
    let hex = |x: &str| usize::from_str_radix(x.trim().trim_start_matches("0x"), 16).map_err(|_| format!("bad address {s}"));
    match s.split_once(':') {
        Some((seg, off)) => Ok(((hex(seg)? << 4) + hex(off)?) & 0xFFFFF),
        None => Ok(hex(s)? & 0xFFFFF),
    }
}

fn grid_root() -> Option<PathBuf> {
    if let Ok(r) = std::env::var("GRID_ROOT") {
        return Some(PathBuf::from(r));
    }
    let exe = std::env::current_exe().ok()?;
    let up = exe.parent()?.parent()?.to_path_buf();
    up.join("tools").join("gridbuild.py").exists().then_some(up)
}

fn project_dir(root: &Path, source: &str) -> PathBuf {
    let p = PathBuf::from(source.trim_end_matches(['\\', '/']));
    let p = if p.is_relative() { root.join(&p) } else { p };
    if p.is_dir() {
        p
    }
    else if p.is_file() {
        p.parent().map(|d| d.to_path_buf()).unwrap_or(p)
    }
    else {
        root.join("projects").join(source)
    }
}
