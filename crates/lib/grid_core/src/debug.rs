//! Debugger for demo work: breakpoints on code addresses, I/O ports and hardware interrupts,
//! memory write watches, stepping, registers, memory dumps and disassembly. Used by the
//! window version's console (`compass --debug`) and by the Claude Code hook.

use std::collections::{BTreeSet, HashMap};

use marty_core::{bytequeue::ByteQueue, cpu_808x::Intel808x, cpu_common::Cpu};

use crate::GridMachine;

#[derive(Default)]
pub struct Watch {
    pub addr: u32,
    pub len: u32,
    last: Vec<u8>,
}

#[derive(Default)]
pub struct Debugger {
    /// Code breakpoints (linear addresses).
    pub exec: BTreeSet<u32>,
    /// I/O port breakpoints (IN or OUT).
    pub ports: BTreeSet<u16>,
    /// Hardware interrupt breakpoints: 80130 IR line 0-7 (3 = vertical sync tick).
    pub irqs: BTreeSet<u8>,
    pub watches: Vec<Watch>,
    /// Set by the board when a port or interrupt breakpoint is met during an instruction.
    pub hit: Option<String>,
    /// A code breakpoint just stopped here: don't stop again before the CPU moves on.
    resume_from: Option<u32>,
    /// Profiler running (see Profile).
    pub profile: Option<Profile>,
}

/// Profiler: CPU clocks per instruction address over a number of frames, grouped into
/// routines by the CALL targets seen while it ran.
#[derive(Default)]
pub struct Profile {
    /// (CS << 16 | IP) -> (clocks, times executed)
    pub at: HashMap<u32, (u64, u64)>,
    /// CALL targets seen (CS << 16 | IP).
    pub targets: BTreeSet<u32>,
    pub start: u64,
    pub end: u64,
    pub total: u64,
}

impl Profile {
    pub fn report(&self, top: usize) -> String {
        let total = self.total.max(1) as f64;
        let mut s = format!(
            "Profile: {} CPU clocks ({:.1} frames of the 1101 = {:.2} s)\n",
            self.total,
            self.total as f64 / 83_844.0,
            self.total as f64 / 5e6
        );
        // routines: the nearest CALL target at or below the address in the same segment
        let mut routines: HashMap<u32, (u64, u64)> = HashMap::new();
        for (&k, &(c, n)) in &self.at {
            let cs = k & 0xFFFF_0000;
            let r = self.targets.range(cs..=k).next_back().copied().unwrap_or(cs);
            let e = routines.entry(r).or_default();
            e.0 += c;
            e.1 += n;
        }
        let mut rv: Vec<_> = routines.into_iter().collect();
        rv.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        s += "routine (entry)   clocks       %   instructions\n";
        for (k, (c, n)) in rv.iter().take(top) {
            let tag = if self.targets.contains(k) { "" } else { " (no CALL seen)" };
            s += &format!("{:04X}:{:04X}  {:>12} {:6.1}% {:>12}{tag}\n", k >> 16, k & 0xFFFF, c, *c as f64 * 100.0 / total, n);
        }
        let mut av: Vec<_> = self.at.iter().collect();
        av.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        s += "hottest instructions   clocks       %    times  clk/time\n";
        for (k, (c, n)) in av.iter().take(top) {
            s += &format!(
                "{:04X}:{:04X}        {:>12} {:6.1}% {:>8} {:9.1}\n",
                *k >> 16,
                *k & 0xFFFF,
                c,
                *c as f64 * 100.0 / total,
                n,
                *c as f64 / (*n).max(1) as f64
            );
        }
        s
    }
}

impl Debugger {
    pub fn active(&self) -> bool {
        self.profile.is_some() || !(self.exec.is_empty() && self.ports.is_empty() && self.irqs.is_empty() && self.watches.is_empty())
    }

    pub fn clear(&mut self) {
        let p = self.profile.take();
        *self = Debugger::default();
        self.profile = p;
    }

    pub fn list(&self) -> String {
        let mut s = String::new();
        for a in &self.exec {
            s += &format!("code  {:05X}\n", a);
        }
        for p in &self.ports {
            s += &format!("port  {:02X}h\n", p);
        }
        for i in &self.irqs {
            s += &format!("irq   IR{i}\n");
        }
        for w in &self.watches {
            s += &format!("write {:05X} ({} bytes)\n", w.addr, w.len);
        }
        if s.is_empty() {
            s = "no breakpoints\n".into();
        }
        s
    }
}

/// "SSSS:OOOO" or a linear hex address.
pub fn parse_addr(s: &str) -> Result<u32, String> {
    let s = s.trim().trim_end_matches(['h', 'H']);
    let hex = |t: &str| u32::from_str_radix(t.trim(), 16).map_err(|_| format!("not a hex address: {s}"));
    match s.split_once(':') {
        Some((seg, off)) => Ok(((hex(seg)? << 4) + hex(off)?) & 0xFFFFF),
        None => Ok(hex(s)? & 0xFFFFF),
    }
}

impl GridMachine {
    /// CS and IP, from whichever core runs.
    pub fn cs_ip(&mut self) -> (u16, u16) {
        let (r, ip, _, _) = self.regs();
        (r[9], ip)
    }

    fn pc_linear(&mut self) -> u32 {
        let (cs, ip) = self.cs_ip();
        (((cs as u32) << 4) + ip as u32) & 0xFFFFF
    }

    /// Registers as one line.
    pub fn reg_dump(&mut self) -> String {
        let (r, ip, f, clocks) = self.regs();
        let flag = |b: u16, c: char| if f & (1 << b) != 0 { c } else { '-' };
        format!(
            "AX={:04X} BX={:04X} CX={:04X} DX={:04X} SP={:04X} BP={:04X} SI={:04X} DI={:04X}\nES={:04X} CS={:04X} SS={:04X} DS={:04X} IP={:04X} {}{}{}{}{}{}{}{}{} clock {}",
            r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7], r[8], r[9], r[10], r[11], ip,
            flag(11, 'O'), flag(10, 'D'), flag(9, 'I'), flag(8, 'T'), flag(7, 'S'), flag(6, 'Z'), flag(4, 'A'), flag(2, 'P'), flag(0, 'C'),
            clocks
        )
    }

    /// A byte of memory as the CPU would read it (the board's I/O area reads FFh here, so a
    /// dump has no side effects).
    pub fn peek(&self, addr: u32) -> u8 {
        let a = addr as usize & 0xFFFFF;
        if a >= self.model.ram_size() && !(0xF0000..=0xFFFFF).contains(&a) && !(0x80000..=0x9FFFF).contains(&a) {
            return 0xFF;
        }
        self.cpu.bus().peek_u8(a).unwrap_or(0xFF)
    }

    /// Hex dump of `len` bytes.
    pub fn dump(&self, addr: u32, len: u32) -> String {
        let mut out = String::new();
        for row in (0..len).step_by(16) {
            let base = addr + row;
            let bytes: Vec<u8> = (0..16.min(len - row)).map(|i| self.peek(base + i)).collect();
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
            let asc: String = bytes.iter().map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { '.' }).collect();
            out += &format!("{:05X}  {:<47}  {asc}\n", base & 0xFFFFF, hex.join(" "));
        }
        out
    }

    /// Disassemble `n` instructions from SSSS:OOOO (RAM, PROM and ROM window only).
    pub fn disassemble(&mut self, seg: u16, mut off: u16, n: usize) -> String {
        let mut out = String::new();
        for _ in 0..n {
            let lin = (((seg as usize) << 4) + off as usize) & 0xFFFFF;
            let bytes: Vec<u8> = (0..6).map(|i| self.peek((lin + i) as u32)).collect();
            let bus = self.cpu.bus_mut();
            bus.seek(lin);
            let (text, size) = match Intel808x::decode(bus, true) {
                Ok(i) => (i.to_string(), i.size.max(1) as usize),
                Err(_) => ("??".into(), 1),
            };
            let hex: Vec<String> = bytes[..size.min(6)].iter().map(|b| format!("{b:02X}")).collect();
            out += &format!("{seg:04X}:{off:04X}  {:<14} {text}\n", hex.join(""));
            off = off.wrapping_add(size as u16);
        }
        out
    }

    /// Add a write watch on `len` bytes at `addr`.
    pub fn watch(&mut self, addr: u32, len: u32) {
        let last = (0..len).map(|i| self.peek(addr + i)).collect();
        self.board_mut().debug.watches.push(Watch { addr, len, last });
    }

    /// Check breakpoints before and after one instruction; Some(reason) when one is met.
    fn debug_step(&mut self) -> Option<String> {
        let pc = self.pc_linear();
        let d = &mut self.board_mut().debug;
        if d.exec.contains(&pc) && d.resume_from != Some(pc) {
            d.resume_from = Some(pc);
            let (cs, ip) = self.cs_ip();
            return Some(format!("breakpoint at {cs:04X}:{ip:04X}"));
        }
        d.resume_from = None;
        if self.board().debug.profile.is_some() {
            let (cs, ip) = self.cs_ip();
            let op = self.peek(pc);
            let modrm = self.peek(pc + 1);
            let call = op == 0xE8 || op == 0x9A || (op == 0xFF && matches!((modrm >> 3) & 7, 2 | 3));
            let c = self.step() as u64;
            let now = self.board().clocks;
            let (ncs, nip) = if call { self.cs_ip() } else { (0, 0) };
            let p = self.board_mut().debug.profile.as_mut().unwrap();
            let e = p.at.entry(((cs as u32) << 16) | ip as u32).or_default();
            e.0 += c;
            e.1 += 1;
            p.total += c;
            if call {
                p.targets.insert(((ncs as u32) << 16) | nip as u32);
            }
            if now >= p.end {
                let r = p.report(15);
                self.board_mut().debug.profile = None;
                return Some(format!("profile done\n{r}"));
            }
        }
        else {
            self.step();
        }
        if let Some(h) = self.board_mut().debug.hit.take() {
            return Some(h);
        }
        if !self.board().debug.watches.is_empty() {
            let mut hit = None;
            let mut ws = std::mem::take(&mut self.board_mut().debug.watches);
            for w in ws.iter_mut() {
                let now: Vec<u8> = (0..w.len).map(|i| self.peek(w.addr + i)).collect();
                if now != w.last {
                    let (cs, ip) = self.cs_ip();
                    hit = Some(format!(
                        "write to {:05X}: {:02X?} -> {:02X?} (next instruction {cs:04X}:{ip:04X})",
                        w.addr, w.last, now
                    ));
                    w.last = now;
                }
            }
            self.board_mut().debug.watches = ws;
            if hit.is_some() {
                return hit;
            }
        }
        None
    }

    /// Run `clocks` CPU clocks, stopping at a breakpoint: Some(reason) if one was met.
    pub fn run_debug(&mut self, clocks: u64) -> Option<String> {
        if !self.board().debug.active() {
            self.run_clocks(clocks);
            return None;
        }
        let end = self.board().clocks + clocks;
        while self.board().clocks < end {
            if let Some(r) = self.debug_step() {
                return Some(r);
            }
        }
        None
    }

    /// Execute `n` instructions (stops early at a breakpoint).
    pub fn step_debug(&mut self, n: u64) -> Option<String> {
        // stepping off a code breakpoint must not stop on it again
        let pc = self.pc_linear();
        self.board_mut().debug.resume_from = Some(pc);
        for _ in 0..n {
            if let Some(r) = self.debug_step() {
                return Some(r);
            }
        }
        None
    }

    /// One debugger command (the window console and the Claude Code hook share these).
    /// Returns the text to show and whether the machine should stay paused.
    pub fn debug_command(&mut self, line: &str, paused: bool) -> (String, bool) {
        let mut it = line.split_whitespace();
        let cmd = it.next().unwrap_or("").to_ascii_lowercase();
        let args: Vec<&str> = it.collect();
        let arg = |i: usize| args.get(i).copied();
        let num = |i: usize, d: u32| arg(i).and_then(|s| u32::from_str_radix(s.trim_end_matches(['h', 'H']), 16).ok()).unwrap_or(d);
        let r: Result<(String, bool), String> = (|| match cmd.as_str() {
            "" => Ok((String::new(), paused)),
            "h" | "help" | "?" => Ok((DEBUG_HELP.to_string(), paused)),
            "p" | "pause" => {
                let (cs, ip) = self.cs_ip();
                Ok((format!("paused at {cs:04X}:{ip:04X}\n{}\n", self.reg_dump()), true))
            }
            "g" | "go" | "c" => Ok(("running\n".into(), false)),
            "s" | "step" | "t" => {
                let n = arg(0).and_then(|s| s.parse().ok()).unwrap_or(1);
                let c0 = self.board().clocks;
                let why = self.step_debug(n);
                let (cs, ip) = self.cs_ip();
                let mut s = format!("{} clocks\n{}\n{}", self.board().clocks - c0, self.reg_dump(), self.disassemble(cs, ip, 1));
                if let Some(w) = why {
                    s = format!("{w}\n{s}");
                }
                Ok((s, true))
            }
            "r" | "regs" => Ok((self.reg_dump() + "\n", paused)),
            "m" | "mem" => {
                let a = parse_addr(arg(0).ok_or("m ADDRESS [LENGTH]")?)?;
                Ok((self.dump(a, num(1, 0x80).min(4096)), paused))
            }
            "u" | "dis" => {
                let (seg, off) = match arg(0) {
                    Some(a) => match a.split_once(':') {
                        Some((s, o)) => (
                            u16::from_str_radix(s, 16).map_err(|_| "bad segment")?,
                            u16::from_str_radix(o, 16).map_err(|_| "bad offset")?,
                        ),
                        None => return Err("u SSSS:OOOO [COUNT]".into()),
                    },
                    None => self.cs_ip(),
                };
                let n = arg(1).and_then(|s| s.parse().ok()).unwrap_or(16);
                Ok((self.disassemble(seg, off, n), paused))
            }
            "b" | "bp" => {
                let a = parse_addr(arg(0).ok_or("b ADDRESS")?)?;
                self.board_mut().debug.exec.insert(a);
                Ok((format!("code breakpoint at {a:05X}\n"), paused))
            }
            "bio" | "port" => {
                let p = u16::from_str_radix(arg(0).ok_or("bio PORT")?.trim_end_matches(['h', 'H']), 16).map_err(|_| "bad port")?;
                self.board_mut().debug.ports.insert(p);
                Ok((format!("port breakpoint at {p:02X}h\n"), paused))
            }
            "bi" | "irq" => {
                let n: u8 = arg(0).and_then(|s| s.parse().ok()).filter(|&n| n < 8).ok_or("bi 0-7 (IR line; 3 = vertical sync)")?;
                self.board_mut().debug.irqs.insert(n);
                Ok((format!("interrupt breakpoint on IR{n}\n"), paused))
            }
            "bw" | "watch" => {
                let a = parse_addr(arg(0).ok_or("bw ADDRESS [LENGTH]")?)?;
                let n = num(1, 1).clamp(1, 256);
                self.watch(a, n);
                Ok((format!("write watch on {a:05X} ({n} bytes)\n"), paused))
            }
            "bl" | "list" => Ok((self.board().debug.list(), paused)),
            "bc" | "clear" => {
                self.board_mut().debug.clear();
                Ok(("all breakpoints cleared\n".into(), paused))
            }
            "prof" | "profile" => {
                let frames: u64 = arg(0).and_then(|s| s.parse().ok()).unwrap_or(60);
                let now = self.board().clocks;
                let fc = self.board().frame_clocks();
                self.board_mut().debug.profile = Some(Profile { start: now, end: now + frames * fc, ..Default::default() });
                Ok((format!("profiling the next {frames} frames (the result comes when they have run)\n"), false))
            }
            "marks" => {
                let b = self.board();
                Ok((b.marks.report(b.frame_clocks(), crate::screen::CPU_HZ), paused))
            }
            "marks-clear" => {
                self.board_mut().marks.reset();
                Ok(("marks cleared\n".into(), paused))
            }
            _ => Err(format!("unknown command {cmd} (h for help)")),
        })();
        match r {
            Ok(v) => v,
            Err(e) => (e + "\n", paused),
        }
    }
}

pub const DEBUG_HELP: &str = "\
Debugger commands (numbers are hex; addresses SSSS:OOOO or linear):
  p              pause             g        go on
  s [N]          step N instructions (decimal), shows clocks used
  r              registers         u [SSSS:OOOO] [N]   disassemble
  m ADDR [LEN]   memory dump
  b ADDR         break when the CPU gets to ADDR
  bio PORT       break on IN/OUT to PORT
  bi N           break on hardware interrupt IR N (0-7; 3 = vertical sync tick)
  bw ADDR [LEN]  break when memory there changes
  bl / bc        list / clear breakpoints
  prof [N]       profile the next N frames (decimal, default 60): clocks per routine
                 and per instruction; runs the machine and reports at the end
  marks          cycle counter report      marks-clear   clear it
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoints_and_watch() {
        // MOV AL,1; OUT F0,AL; MOV [0010],AL; NOP; HLT
        let code = [0xB0, 0x01, 0xE6, 0xF0, 0xA2, 0x10, 0x00, 0x90, 0xF4];
        let mut m = crate::fragment::run(crate::GridModel::Compass1101, &[0xF4], 10).machine;
        for (i, b) in code.iter().enumerate() {
            let _ = m.cpu.bus_mut().write_u8(0x10000 + i, *b, 0);
        }
        assert_eq!(parse_addr("1000:0004").unwrap(), 0x10004);
        m.debug_command("b 1000:0004", true);
        m.debug_command("bio F0", true);
        m.debug_command("bw 2000:0010", true);
        let r = m.run_debug(1000).unwrap();
        assert!(r.contains("port F0"), "{r}");
        let r = m.run_debug(1000).unwrap();
        assert!(r.contains("1000:0004"), "{r}");
        let r = m.run_debug(1000).unwrap();
        assert!(r.contains("write to 20010"), "{r}");
        // profile: a CALL to a loop that counts down
        // 0: CALL 0005; 3: JMP 0000; 5: MOV CX,50; L: LOOP L; RET
        let code = [0xE8, 0x02, 0x00, 0xEB, 0xFB, 0xB9, 0x32, 0x00, 0xE2, 0xFE, 0xC3];
        let mut m2 = crate::fragment::run(crate::GridModel::Compass1101, &code, 1).machine;
        m2.debug_command("prof 1", true);
        let r = m2.run_debug(200_000).unwrap();
        assert!(r.contains("profile done") && r.contains("1000:0005"), "{r}");
        println!("{r}");
        let d = m.disassemble(0x1000, 0, 2);
        assert!(d.contains("mov") && d.contains("out"), "{d}");
    }
}
