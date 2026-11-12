//! Debugger for demo work: breakpoints on code addresses, I/O ports and hardware interrupts,
//! memory write watches, stepping, registers, memory dumps and disassembly. Used by the
//! window version's console (`compass --debug`) and by the Claude Code hook.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use marty_core::{bytequeue::ByteQueue, cpu_808x::Intel808x, cpu_common::Cpu};

use crate::{symbols::Symbols, GridMachine};

/// What the crash catcher stops on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Catch {
    pub on: bool,
    /// GRiD-OS errors: an error code turned into its message (OsDecodeException), so the
    /// machine stops where a program is about to show an error.
    pub errors: bool,
    /// Divide error (DIV/IDIV overflow or by zero, AAM 0).
    pub divide: bool,
    /// INT 3 that isn't GRiD-OS's own (its loader turns each 8087 WAIT into INT 3).
    pub int3: bool,
    /// Running through empty memory (00 00 or FF FF): a jump, call or return gone wrong.
    pub wild: bool,
}

impl Default for Catch {
    fn default() -> Self {
        Catch { on: false, errors: true, divide: true, int3: true, wild: true }
    }
}

/// Instructions kept for "recent instructions" (CS << 16 | IP).
const HISTORY: usize = 256;
/// OS calls kept in the call log.
const OS_LOG: usize = 400;

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
    /// Program symbols (see symbols.rs).
    pub syms: Symbols,
    /// Code breakpoints by name, armed when the program is found in memory: name -> where.
    pub named: BTreeMap<String, Option<u32>>,
    pub catch: Catch,
    /// Last crash report (for the window).
    pub crash: Option<String>,
    /// Recent instructions (CS << 16 | IP), newest last.
    pub history: VecDeque<u32>,
    /// Instructions in a row executed from empty memory.
    empty_run: u32,
    /// OsDecodeException called: (return address, message buffer, error code).
    decoding: Option<(u32, u32, u16)>,
    /// GRiD-OS call log on, and the calls (newest last).
    pub os_log_on: bool,
    pub os_log: VecDeque<String>,
    /// Entry points of the call vectors 70h, 71h, 73h (+2 = the library's way in), refreshed
    /// now and then.
    entries: [u32; 3],
    /// Clock of the last symbol search while the program wasn't in memory.
    last_locate: u64,
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
    pub fn report(&self, top: usize, syms: &Symbols) -> String {
        let lin = |k: u32| (((k >> 16) << 4) + (k & 0xFFFF)) & 0xFFFFF;
        let total = self.total.max(1) as f64;
        let mut s = format!(
            "Profile: {} CPU clocks ({:.1} frames of the 1101 = {:.2} s)\n",
            self.total,
            self.total as f64 / crate::screen::FRAME_CLOCKS as f64,
            self.total as f64 / 5e6
        );
        // routines: the procedure from the symbols, else the nearest CALL target at or below
        // the address in the same segment
        let mut routines: HashMap<u32, (u64, u64)> = HashMap::new();
        let mut proc_names: HashMap<u32, String> = HashMap::new();
        for (&k, &(c, n)) in &self.at {
            let cs = k & 0xFFFF_0000;
            let r = match syms.name_at(lin(k)) {
                Some(name) => {
                    let base = name.split('+').next().unwrap_or("").to_string();
                    let start = syms.lookup(&base).filter(|&a| a <= lin(k)).map(|a| k - (lin(k) - a)).unwrap_or(k);
                    proc_names.insert(start, base);
                    start
                }
                None => self.targets.range(cs..=k).next_back().copied().unwrap_or(cs),
            };
            let e = routines.entry(r).or_default();
            e.0 += c;
            e.1 += n;
        }
        let mut rv: Vec<_> = routines.into_iter().collect();
        rv.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        s += "routine (entry)   clocks       %   instructions\n";
        for (k, (c, n)) in rv.iter().take(top) {
            let tag = match proc_names.get(k) {
                Some(p) => format!(" {p}"),
                None if self.targets.contains(k) => String::new(),
                None => " (no CALL seen)".into(),
            };
            s += &format!("{:04X}:{:04X}  {:>12} {:6.1}% {:>12}{tag}\n", k >> 16, k & 0xFFFF, c, *c as f64 * 100.0 / total, n);
        }
        let mut av: Vec<_> = self.at.iter().collect();
        av.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        s += "hottest instructions   clocks       %    times  clk/time\n";
        for (k, (c, n)) in av.iter().take(top) {
            let name = syms.name_at(lin(**k)).map(|n| format!(" {n}")).unwrap_or_default();
            let line = syms.line_of(lin(**k)).map(|l| format!("  [{l}]")).unwrap_or_default();
            s += &format!(
                "{:04X}:{:04X}        {:>12} {:6.1}% {:>8} {:9.1}{name}{line}\n",
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
        self.profile.is_some()
            || self.catch.on
            || self.os_log_on
            || !(self.exec.is_empty() && self.ports.is_empty() && self.irqs.is_empty() && self.watches.is_empty() && self.named.is_empty())
    }

    /// Clear the breakpoints (symbols, crash catcher settings and the profiler stay).
    pub fn clear(&mut self) {
        self.exec.clear();
        self.ports.clear();
        self.irqs.clear();
        self.watches.clear();
        self.named.clear();
        self.hit = None;
        self.resume_from = None;
    }

    pub fn list(&self) -> String {
        let mut s = String::new();
        for (n, a) in &self.named {
            match a {
                Some(a) => s += &format!("code  {n} ({:05X})\n", a),
                None => s += &format!("code  {n} (when the program is in memory)\n"),
            }
        }
        for a in &self.exec {
            if self.named.values().any(|x| *x == Some(*a)) {
                continue;
            }
            match self.syms.name_at(*a) {
                Some(n) => s += &format!("code  {:05X} {n}\n", a),
                None => s += &format!("code  {:05X}\n", a),
            }
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

    /// Add a write watch on `len` bytes at `addr`.
    pub fn watch(&mut self, addr: u32, len: u32) {
        let last = (0..len).map(|i| self.peek(addr + i)).collect();
        self.board_mut().debug.watches.push(Watch { addr, len, last });
    }

    /// Disassemble `n` instructions from SSSS:OOOO (RAM, PROM and ROM window only), with
    /// the program's names and source lines when symbols are loaded.
    pub fn disassemble(&mut self, seg: u16, mut off: u16, n: usize) -> String {
        self.locate_symbols();
        let mut out = String::new();
        for _ in 0..n {
            let lin = (((seg as usize) << 4) + off as usize) & 0xFFFFF;
            let syms = &self.board().debug.syms;
            if !syms.is_empty() {
                if let Some(l) = syms.label_at(lin as u32) {
                    out += &format!("{l}:\n");
                }
                if let Some(src) = syms.line_at(lin as u32) {
                    out += &format!("                ; {src}\n");
                }
            }
            let (text, size, note) = self.decode_at(seg, off);
            let bytes: Vec<u8> = (0..size.min(6)).map(|i| self.peek((lin + i) as u32)).collect();
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
            out += &format!("{seg:04X}:{off:04X}  {:<14} {text}{note}\n", hex.join(""));
            off = off.wrapping_add(size as u16);
        }
        out
    }

    /// One instruction: (text, size, "  ; NAME" of a jump or call target when known).
    pub fn decode_at(&mut self, seg: u16, off: u16) -> (String, usize, String) {
        let lin = (((seg as usize) << 4) + off as usize) & 0xFFFFF;
        let bus = self.cpu.bus_mut();
        bus.seek(lin);
        let (text, size) = match Intel808x::decode(bus, true) {
            Ok(i) => (i.to_string(), i.size.max(1) as usize),
            Err(_) => ("??".into(), 1),
        };
        let note = match self.branch_target(seg, off) {
            Some(t) => self.board().debug.syms.name_at(t).map(|n| format!("  ; {n}")).unwrap_or_default(),
            None => String::new(),
        };
        (text, size, note)
    }

    /// Where a direct CALL or JMP at SSSS:OOOO goes (linear).
    fn branch_target(&self, seg: u16, off: u16) -> Option<u32> {
        let lin = |o: u16| (((seg as u32) << 4) + o as u32) & 0xFFFFF;
        let b = |k: u16| self.peek(lin(off.wrapping_add(k)));
        let w = |k: u16| u16::from_le_bytes([b(k), b(k + 1)]);
        match b(0) {
            0xE8 | 0xE9 => Some(lin(off.wrapping_add(3).wrapping_add(w(1)))),
            0xEB | 0x70..=0x7F | 0xE0..=0xE3 => Some(lin(off.wrapping_add(2).wrapping_add(b(1) as i8 as u16))),
            0x9A | 0xEA => Some((((w(3) as u32) << 4) + w(1) as u32) & 0xFFFFF),
            _ => None,
        }
    }

    /// Load program symbols from a folder (or a file in it); returns the report.
    pub fn load_symbols(&mut self, path: &std::path::Path) -> Result<String, String> {
        let s = Symbols::load(path)?;
        self.board_mut().debug.syms = s;
        self.board_mut().debug.last_locate = 0;
        self.locate_symbols();
        Ok(self.board().debug.syms.report())
    }

    /// Find the program's modules in memory (quick when they haven't moved) and arm the
    /// breakpoints set by name.
    pub fn locate_symbols(&mut self) {
        if self.board().debug.syms.is_empty() {
            return;
        }
        let mut syms = std::mem::take(&mut self.board_mut().debug.syms);
        let top = self.model.ram_size();
        {
            let mem = self.cpu.bus_mut().memory_mut();
            let n = top.min(mem.len());
            syms.locate(&mem[..n]);
        }
        let d = &mut self.board_mut().debug;
        d.syms = syms;
        let names: Vec<String> = d.named.keys().cloned().collect();
        for n in names {
            let now = d.syms.lookup(&n);
            let old = d.named[&n];
            if now != old {
                if let Some(o) = old {
                    d.exec.remove(&o);
                }
                if let Some(a) = now {
                    d.exec.insert(a);
                }
                d.named.insert(n, now);
            }
        }
    }

    /// An address typed in a command: a name from the symbols (PROC, MODULE.PROC,
    /// MODULE:LINE), SSSS:OOOO or linear hex.
    pub fn resolve(&mut self, s: &str) -> Result<u32, String> {
        let hexy = |t: &str| t.split(':').all(|p| !p.is_empty() && p.trim_end_matches(['h', 'H']).chars().all(|c| c.is_ascii_hexdigit()));
        if !self.board().debug.syms.is_empty() && !(s.contains(':') && hexy(s)) {
            self.locate_symbols();
            if let Some(a) = self.board().debug.syms.lookup(s) {
                return Ok(a);
            }
        }
        parse_addr(s).map_err(|e| if self.board().debug.syms.is_empty() { e } else { format!("no name {s} in the program in memory, and {e}") })
    }

    /// "1234:0019 GREET+5  [HELLO 23: dummy := i + 1;]"
    pub fn describe(&self, cs: u16, ip: u16) -> String {
        self.describe_at(cs, ip, false)
    }

    /// As describe; `ret`: a return address, whose source line is the CALL's before it.
    fn describe_at(&self, cs: u16, ip: u16, ret: bool) -> String {
        let lin = (((cs as u32) << 4) + ip as u32) & 0xFFFFF;
        let syms = &self.board().debug.syms;
        let mut s = format!("{cs:04X}:{ip:04X}");
        if let Some(n) = syms.name_at(if ret { lin - 1 } else { lin }) {
            s += &format!(" {}", if ret { syms.name_at(lin).unwrap_or(n) } else { n });
        }
        if let Some(l) = syms.line_of(if ret { lin.saturating_sub(1) } else { lin }) {
            s += &format!("  [{l}]");
        }
        s
    }

    fn peek16(&self, addr: u32) -> u16 {
        u16::from_le_bytes([self.peek(addr), self.peek(addr + 1)])
    }

    /// Is there a CALL just before this return address?
    fn call_before(&self, cs: u16, ip: u16) -> bool {
        let lin = |o: u16| (((cs as u32) << 4) + o as u32) & 0xFFFFF;
        let b = |k: u16| self.peek(lin(ip.wrapping_sub(k)));
        if b(3) == 0xE8 || b(5) == 0x9A {
            return true;
        }
        // CALL r/m (FF /2, FF /3): ModR/M with its displacement
        (2..=4).any(|len| {
            let m = b(len - 1);
            let reg = (m >> 3) & 7;
            let size = match m >> 6 {
                0 if m & 7 == 6 => 4,
                0 => 2,
                1 => 3,
                2 => 4,
                _ => 2,
            };
            b(len) == 0xFF && (reg == 2 || reg == 3) && size == len
        })
    }

    /// Call stack from CS:IP: the frames found by following BP (each procedure's PUSH BP;
    /// MOV BP,SP), checking that a CALL comes before each return address. Newest first.
    pub fn backtrace(&mut self, cs: u16, ip: u16, max: usize) -> Vec<(u16, u16)> {
        let (r, _, _, _) = self.regs();
        let (ss, sp, mut bp) = (r[10], r[4], r[5]);
        let st = |o: u16| (((ss as u32) << 4) + o as u32) & 0xFFFFF;
        let mut out = vec![(cs, ip)];
        let mut cur_cs = cs;
        let lin = (((cs as u32) << 4) + ip as u32) & 0xFFFFF;
        // at a procedure's first instructions BP still belongs to the caller: the return
        // address is on top of the stack
        let at_entry = self.peek(lin) == 0x55 || (self.peek(lin) == 0x8B && self.peek(lin + 1) == 0xEC);
        if at_entry {
            let top = if self.peek(lin) == 0x55 { sp } else { sp.wrapping_add(2) };
            let rip = self.peek16(st(top));
            let rcs = self.peek16(st(top.wrapping_add(2)));
            if self.call_before(rcs, rip) && rcs != cs {
                out.push((rcs, rip));
                cur_cs = rcs;
            }
            else if self.call_before(cs, rip) {
                out.push((cs, rip));
            }
        }
        while out.len() < max && bp != 0 {
            let mut found = None;
            for k in [2u16, 4] {
                let rip = self.peek16(st(bp.wrapping_add(k)));
                let rcs = self.peek16(st(bp.wrapping_add(k + 2)));
                if rcs != cur_cs && self.call_before(rcs, rip) && self.peek((((rcs as u32) << 4) + rip.wrapping_sub(5) as u32) & 0xFFFFF) == 0x9A {
                    found = Some((rcs, rip));
                    break;
                }
                if self.call_before(cur_cs, rip) {
                    found = Some((cur_cs, rip));
                    break;
                }
            }
            let Some((fcs, fip)) = found
            else {
                break;
            };
            out.push((fcs, fip));
            cur_cs = fcs;
            let next = self.peek16(st(bp));
            if next <= bp {
                break;
            }
            bp = next;
        }
        out
    }

    /// The call stack as text, one frame a line.
    pub fn backtrace_text(&mut self, cs: u16, ip: u16) -> String {
        self.locate_symbols();
        let frames = self.backtrace(cs, ip, 24);
        let mut s = String::new();
        for (i, (c, o)) in frames.iter().enumerate() {
            s += &format!("  #{i:<2} {}\n", self.describe_at(*c, *o, i > 0));
        }
        s
    }

    /// The last `n` instructions run (crash catcher or any breakpoint on), oldest first.
    pub fn history_text(&mut self, n: usize) -> String {
        self.locate_symbols();
        let h: Vec<u32> = self.board().debug.history.iter().rev().take(n).rev().copied().collect();
        let mut s = String::new();
        for k in h {
            let (cs, ip) = ((k >> 16) as u16, k as u16);
            let (text, _, note) = self.decode_at(cs, ip);
            let lin = (((cs as u32) << 4) + ip as u32) & 0xFFFFF;
            let name = self.board().debug.syms.name_at(lin).map(|n| format!(" {n}")).unwrap_or_default();
            s += &format!("  {cs:04X}:{ip:04X}{name:<18} {text}{note}\n");
        }
        s
    }

    /// Crash report: why, where (with the source line), call stack and recent instructions.
    fn crash_report(&mut self, why: String, cs: u16, ip: u16) -> String {
        self.crash_report_at(why, cs, ip, false)
    }

    /// `ret`: CS:IP is where a call returned to (the source line shown is the call's).
    fn crash_report_at(&mut self, why: String, cs: u16, ip: u16, ret: bool) -> String {
        let at = self.describe_at(cs, ip, ret);
        let mut s = format!("CRASH CATCHER: {why}\n  at {at}\nCall stack:\n");
        s += &self.backtrace_text(cs, ip);
        s += "Recent instructions:\n";
        s += &self.history_text(16);
        self.board_mut().debug.crash = Some(s.clone());
        s
    }

    /// The vector 70h/71h/73h entry points (the library jumps to the vector's target + 2).
    fn refresh_entries(&mut self) {
        let e = [0x70u32, 0x71, 0x73].map(|v| {
            let off = self.peek16(v * 4) as u32;
            let seg = self.peek16(v * 4 + 2) as u32;
            (((seg << 4) + off + 2) & 0xFFFFF) | if seg == 0 && off == 0 { 0x8000_0000 } else { 0 }
        });
        self.board_mut().debug.entries = e;
    }

    /// GRiD-OS call at an entry point: log it, and note OsDecodeException for the catcher.
    fn os_call(&mut self, which: usize, pc: u32) {
        let (r, _, _, _) = self.regs();
        let (ss, sp) = (r[10] as u32, r[4]);
        let st = |o: u16| ((ss << 4) + o as u32) & 0xFFFFF;
        let n = r[0] as u8;
        let vec = [0x70u8, 0x71, 0x73][which];
        let rip = self.peek16(st(sp));
        let rcs = self.peek16(st(sp.wrapping_add(2)));
        let _ = pc;
        if vec == 0x70 && n == 0x27 && self.board().debug.catch.on && self.board().debug.catch.errors {
            // OsDecodeException (code, pMessage): arguments above the return address
            let p_off = self.peek16(st(sp.wrapping_add(4))) as u32;
            let p_seg = self.peek16(st(sp.wrapping_add(6))) as u32;
            let code = self.peek16(st(sp.wrapping_add(8)));
            let ret = (((rcs as u32) << 4) + rip as u32) & 0xFFFFF;
            self.board_mut().debug.decoding = Some((ret, ((p_seg << 4) + p_off) & 0xFFFFF, code));
        }
        if self.board().debug.os_log_on {
            let name = crate::oscalls::name(vec, n).map(|s| s.to_string()).unwrap_or_else(|| format!("{vec:02X}h call {n:02X}h"));
            let from = self.describe(rcs, rip.wrapping_sub(3));
            let d = &mut self.board_mut().debug;
            if d.os_log.len() >= OS_LOG {
                d.os_log.pop_front();
            }
            d.os_log.push_back(format!("{name:<24} from {from}"));
        }
    }

    /// Crash catcher and OS call checks before an instruction: Some(report) to stop.
    fn catch_before(&mut self, pc: u32) -> Option<String> {
        let (cs, ip) = self.cs_ip();
        let d = &self.board().debug;
        let catch = d.catch;
        let entries = d.entries;
        if let Some((ret, buf, code)) = d.decoding {
            if pc == ret {
                self.board_mut().debug.decoding = None;
                let len = (self.peek(buf) as u32).min(80);
                let text: String = (1..=len).map(|i| self.peek(buf + i)).map(|b| if (0x20..0x7F).contains(&b) { b as char } else { ' ' }).collect();
                let text = text.trim();
                let text = text.strip_prefix(&format!("{code}:")).unwrap_or(text).trim();
                let why = format!("GRiD-OS error {code}: {text}");
                return Some(self.crash_report_at(why, cs, ip, true));
            }
        }
        if let Some(w) = entries.iter().position(|&e| e == pc) {
            self.os_call(w, pc);
        }
        if !catch.on {
            return None;
        }
        let op = self.peek(pc);
        if catch.int3 && (op == 0xCC || (op == 0xCD && self.peek(pc + 1) == 3)) {
            // GRiD-OS's loader turns every WAIT into INT 3: before an 8087 instruction, or
            // after one (to wait for its result). The object file knows for sure.
            let esc_at = |m: &Self, a: u32| {
                let b = m.peek(a);
                (0xD8..=0xDF).contains(&b) || (matches!(b, 0x26 | 0x2E | 0x36 | 0x3E) && (0xD8..=0xDF).contains(&m.peek(a + 1)))
            };
            let wait = match self.board().debug.syms.was_wait(pc) {
                Some(w) => w,
                None => {
                    let next = esc_at(self, pc + if op == 0xCC { 1 } else { 2 });
                    let prev = self.board().debug.history.back().map(|&k| esc_at(self, ((((k >> 16) << 4) + (k & 0xFFFF)) & 0xFFFFF) as u32)).unwrap_or(false);
                    op == 0xCC && (next || prev)
                }
            };
            if !wait {
                return Some(self.crash_report("INT 3 (a breakpoint in the program)".into(), cs, ip));
            }
        }
        if catch.wild {
            let w = self.peek16(pc);
            if w == 0 || w == 0xFFFF {
                self.board_mut().debug.empty_run += 1;
                if self.board().debug.empty_run == 4 {
                    // where it came from: the last instruction before the empty memory
                    let hist = &self.board().debug.history;
                    let from = hist.iter().rev().find(|&&k| {
                        let l = ((((k >> 16) << 4) + (k & 0xFFFF)) & 0xFFFFF) as u32;
                        let w = self.peek16(l);
                        w != 0 && w != 0xFFFF
                    });
                    let came = match from {
                        Some(&k) => format!(", came from {}", self.describe((k >> 16) as u16, k as u16)),
                        None => String::new(),
                    };
                    let what = if w == 0 { "00 00" } else { "FF FF" };
                    return Some(self.crash_report(format!("wild jump: running through empty memory ({what}){came}"), cs, ip));
                }
            }
            else {
                self.board_mut().debug.empty_run = 0;
            }
        }
        None
    }

    /// Check breakpoints before and after one instruction; Some(reason) when one is met.
    fn debug_step(&mut self) -> Option<String> {
        let pc = self.pc_linear();
        let d = &mut self.board_mut().debug;
        if d.exec.contains(&pc) && d.resume_from != Some(pc) {
            d.resume_from = Some(pc);
            let (cs, ip) = self.cs_ip();
            let at = self.describe(cs, ip);
            return Some(format!("breakpoint at {at}"));
        }
        let resuming = d.resume_from == Some(pc);
        d.resume_from = None;
        if !resuming {
            if let Some(r) = self.catch_before(pc) {
                self.board_mut().debug.resume_from = Some(pc);
                return Some(r);
            }
        }
        let (cs, ip) = self.cs_ip();
        {
            let d = &mut self.board_mut().debug;
            if d.history.len() >= HISTORY {
                d.history.pop_front();
            }
            d.history.push_back(((cs as u32) << 16) | ip as u32);
        }
        let op = self.peek(pc);
        let modrm = self.peek(pc + 1);
        if self.board().debug.profile.is_some() {
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
                self.locate_symbols();
                let p = self.board_mut().debug.profile.take().unwrap();
                let r = p.report(15, &self.board().debug.syms);
                return Some(format!("profile done\n{r}"));
            }
        }
        else {
            self.step();
        }
        // divide error: the CPU went to the INT 0 handler from a DIV, IDIV or AAM
        let c = self.board().debug.catch;
        if c.on && c.divide && (matches!(op, 0xF6 | 0xF7) && (modrm >> 3) & 7 >= 6 || op == 0xD4) {
            let handler = (((self.peek16(2) as u32) << 4) + self.peek16(0) as u32) & 0xFFFFF;
            if self.pc_linear() == handler {
                return Some(self.crash_report("divide error (division by zero or a result too big)".into(), cs, ip));
            }
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
                        "write to {:05X}: {:02X?} -> {:02X?} (next instruction {})",
                        w.addr,
                        w.last,
                        now,
                        self.describe(cs, ip)
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

    /// Housekeeping between runs: OS call entry points, and looking for the program when
    /// breakpoints wait for it (twice a second at most).
    fn debug_prepare(&mut self) {
        let d = &self.board().debug;
        if d.catch.on || d.os_log_on {
            self.refresh_entries();
        }
        let d = &self.board().debug;
        let waiting = d.named.values().any(|a| a.is_none());
        let now = self.board().clocks;
        let _ = waiting;
        if !d.syms.is_empty() && now.saturating_sub(d.last_locate) > 2_500_000 {
            self.board_mut().debug.last_locate = now;
            self.locate_symbols();
        }
    }

    /// Run `clocks` CPU clocks, stopping at a breakpoint: Some(reason) if one was met.
    pub fn run_debug(&mut self, clocks: u64) -> Option<String> {
        if !self.board().debug.active() {
            self.run_clocks(clocks);
            return None;
        }
        self.debug_prepare();
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
        self.debug_prepare();
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
                let a = self.resolve(arg(0).ok_or("m ADDRESS [LENGTH]")?)?;
                Ok((self.dump(a, num(1, 0x80).min(4096)), paused))
            }
            "u" | "dis" => {
                let (seg, off) = match arg(0) {
                    Some(a) => match a.split_once(':').map(|(s, o)| (u16::from_str_radix(s, 16), u16::from_str_radix(o, 16))) {
                        Some((Ok(s), Ok(o))) => (s, o),
                        _ => {
                            // a name: show it in the program's own code segment
                            let lin = self.resolve(a)?;
                            let (cs, _) = self.cs_ip();
                            let cs_lin = (cs as u32) << 4;
                            if lin >= cs_lin && lin < cs_lin + 0x10000 {
                                (cs, (lin - cs_lin) as u16)
                            }
                            else {
                                ((lin >> 4) as u16, (lin & 0xF) as u16)
                            }
                        }
                    },
                    None => self.cs_ip(),
                };
                let n = arg(1).and_then(|s| s.parse().ok()).unwrap_or(16);
                Ok((self.disassemble(seg, off, n), paused))
            }
            "b" | "bp" => {
                let t = arg(0).ok_or("b ADDRESS or NAME")?;
                let hexy = t.split(':').all(|p| !p.is_empty() && p.trim_end_matches(['h', 'H']).chars().all(|c| c.is_ascii_hexdigit()));
                if self.board().debug.syms.is_empty() || (t.contains(':') && hexy) {
                    let a = parse_addr(t)?;
                    self.board_mut().debug.exec.insert(a);
                    return Ok((format!("code breakpoint at {a:05X}\n"), paused));
                }
                // a name: armed whenever the program is in memory (also after it is started
                // again somewhere else)
                let known = self.board().debug.syms.modules.iter().any(|m| {
                    m.syms.iter().any(|s| s.name.eq_ignore_ascii_case(t.rsplit('.').next().unwrap_or(t))) || m.name.eq_ignore_ascii_case(t.split(':').next().unwrap_or(t))
                });
                if !known && hexy {
                    let a = parse_addr(t)?;
                    self.board_mut().debug.exec.insert(a);
                    return Ok((format!("code breakpoint at {a:05X}\n"), paused));
                }
                if !known && self.resolve(t).is_err() {
                    return Err(format!("no name {t} in the symbols (sym lists the modules)"));
                }
                self.board_mut().debug.named.insert(t.to_ascii_uppercase(), None);
                self.locate_symbols();
                Ok(match self.board().debug.named.get(&t.to_ascii_uppercase()).copied().flatten() {
                    Some(a) => (format!("code breakpoint at {} ({a:05X})\n", t.to_ascii_uppercase()), paused),
                    None => (format!("code breakpoint at {} (set when the program is in memory)\n", t.to_ascii_uppercase()), paused),
                })
            }
            "sym" | "symbols" => match arg(0) {
                None => {
                    self.locate_symbols();
                    let s = &self.board().debug.syms;
                    Ok((if s.is_empty() { "no symbols loaded (sym FOLDER: the project folder of a debug build)\n".into() } else { s.report() }, paused))
                }
                Some("off") => {
                    let d = &mut self.board_mut().debug;
                    d.syms = Symbols::default();
                    for a in d.named.values().flatten() {
                        d.exec.remove(a);
                    }
                    d.named.clear();
                    Ok(("symbols unloaded\n".into(), paused))
                }
                Some(_) => {
                    let path = line.trim_start()[cmd.len()..].trim().trim_matches('"');
                    Ok((self.load_symbols(std::path::Path::new(path))?, paused))
                }
            },
            "names" => {
                self.locate_symbols();
                let f = arg(0).unwrap_or("").to_ascii_uppercase();
                let mut s = String::new();
                for (n, a) in self.board().debug.syms.all() {
                    if n.contains(&f) {
                        s += &format!("{:04X}:{:04X}  {n}\n", a >> 4, a & 0xF);
                    }
                }
                if s.is_empty() {
                    s = "no names (sym loads them; the program must be in memory)\n".into();
                }
                Ok((s, paused))
            }
            "k" | "bt" | "stack" => {
                let (cs, ip) = self.cs_ip();
                Ok((format!("Call stack:\n{}", self.backtrace_text(cs, ip)), paused))
            }
            "hist" | "history" => {
                let n = arg(0).and_then(|s| s.parse().ok()).unwrap_or(24).min(HISTORY);
                if self.board().debug.history.is_empty() {
                    return Ok(("no instructions recorded (they are while the crash catcher or a breakpoint is on)\n".into(), paused));
                }
                Ok((self.history_text(n), paused))
            }
            "catch" => {
                let c = &mut self.board_mut().debug.catch;
                match (arg(0), arg(1)) {
                    (None, _) => {}
                    (Some("on"), _) => c.on = true,
                    (Some("off"), _) => c.on = false,
                    (Some(k), Some(v)) => {
                        let on = v == "on";
                        match k {
                            "errors" | "error" => c.errors = on,
                            "div" | "divide" => c.divide = on,
                            "int3" => c.int3 = on,
                            "wild" => c.wild = on,
                            _ => return Err("catch [on|off] or catch errors|div|int3|wild on|off".into()),
                        }
                        if on {
                            c.on = true;
                        }
                    }
                    _ => return Err("catch [on|off] or catch errors|div|int3|wild on|off".into()),
                }
                let c = *c;
                let f = |b: bool| if b { "on" } else { "off" };
                Ok((
                    format!(
                        "crash catcher {}: GRiD-OS errors {}, divide error {}, INT 3 {}, wild jumps {}\n",
                        f(c.on),
                        f(c.errors),
                        f(c.divide),
                        f(c.int3),
                        f(c.wild)
                    ),
                    paused,
                ))
            }
            "os" | "oscalls" => match arg(0) {
                Some("on") => {
                    self.board_mut().debug.os_log_on = true;
                    Ok(("GRiD-OS call log on (os shows it)\n".into(), paused))
                }
                Some("off") => {
                    self.board_mut().debug.os_log_on = false;
                    Ok(("GRiD-OS call log off\n".into(), paused))
                }
                Some("clear") => {
                    self.board_mut().debug.os_log.clear();
                    Ok(("GRiD-OS call log cleared\n".into(), paused))
                }
                _ => {
                    let d = &self.board().debug;
                    let n = arg(0).and_then(|s| s.parse().ok()).unwrap_or(40);
                    let mut s: String = d.os_log.iter().rev().take(n).rev().map(|l| format!("{l}\n")).collect();
                    if s.is_empty() {
                        s = if d.os_log_on { "no GRiD-OS calls yet\n".into() } else { "GRiD-OS call log is off (os on)\n".into() };
                    }
                    Ok((s, paused))
                }
            },
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
Program symbols (from a debug build's .obj files; names work wherever an address does):
  sym FOLDER     load the symbols of the program built there    sym  status   sym off
  b NAME         break at a procedure or label (PROC, MODULE.PROC, MODULE:LINE); armed
                 whenever the program is in memory
  names [TEXT]   list the program's names        k   call stack
  hist [N]       the last N instructions run (decimal)
  catch [on|off] crash catcher: stop on GRiD-OS errors, divide error, INT 3, wild jumps
  catch errors|div|int3|wild on|off              one kind
  os [on|off|clear|N]   log of GRiD-OS calls (name and caller), the last N
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

    fn poke(m: &mut GridMachine, at: usize, bytes: &[u8]) {
        for (i, b) in bytes.iter().enumerate() {
            let _ = m.cpu.bus_mut().write_u8(at + i, *b, 0);
        }
    }

    /// A machine stopped at 1000:0000 with `code` there.
    fn machine_with(code: &[u8]) -> GridMachine {
        let mut m = crate::fragment::run(crate::GridModel::Compass1101, &[0xF4], 10).machine;
        poke(&mut m, 0x10000, code);
        m
    }

    #[test]
    fn crash_catcher() {
        // divide error: MOV CX,0; MOV AX,1; DIV CX; with INT 0 -> 1000:0100 (HLT)
        let mut m = machine_with(&[0xB9, 0, 0, 0xB8, 1, 0, 0xF7, 0xF1, 0x90]);
        poke(&mut m, 0, &[0x00, 0x01, 0x00, 0x10]);
        poke(&mut m, 0x10100, &[0x90, 0x90, 0xEB, 0xFE]);
        m.debug_command("catch on", true);
        let r = m.run_debug(1000).unwrap();
        assert!(r.contains("divide error") && r.contains("1000:0006"), "{r}");
        // INT 3 before an 8087 instruction is GRiD-OS's WAIT: no stop; a lone INT 3 stops
        let mut m = machine_with(&[0xCC, 0xD9, 0xC0, 0x90, 0xCC, 0x90, 0x90]);
        poke(&mut m, 12, &[0x00, 0x02, 0x00, 0x10]);
        poke(&mut m, 0x10200, &[0xCF]);
        m.debug_command("catch on", true);
        let r = m.run_debug(10_000).unwrap();
        assert!(r.contains("INT 3") && r.contains("1000:0004"), "{r}");
        // wild jump into empty memory: JMP FAR 5000:0000
        let mut m = machine_with(&[0x90, 0xEA, 0x00, 0x00, 0x00, 0x50]);
        m.debug_command("catch on", true);
        let r = m.run_debug(10_000).unwrap();
        assert!(r.contains("wild jump") && r.contains("came from 1000:0001"), "{r}");
        // off: nothing stops
        let mut m = machine_with(&[0x90, 0xEA, 0x00, 0x00, 0x00, 0x50]);
        assert!(m.run_debug(10_000).is_none());
    }

    #[test]
    fn symbols_from_a_debug_build() {
        // crashme (tests/data/crashme): a debug build of a Pascal + ASM-86 program. Its code
        // is put at 2000:0000 the way the .run file has it.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/crashme");
        let run = std::fs::read(dir.join("crashme.run")).unwrap();
        let mut m = machine_with(&[0x90]);
        let mut i = 0;
        while i + 3 <= run.len() {
            let n = run[i + 1] as usize | (run[i + 2] as usize) << 8;
            let b = &run[i + 3..i + 2 + n];
            if run[i] == 0x72 && b[1] == 1 {
                let off = b[2] as usize | (b[3] as usize) << 8;
                poke(&mut m, 0x20000 + off, &b[4..]);
            }
            i += 3 + n;
        }
        let r = m.load_symbols(&dir).unwrap();
        assert!(r.contains("CRASHME") && r.contains("at 2000:0000") && r.contains("CRASHASM"), "{r}");
        let inner = m.resolve("Inner").unwrap();
        assert_eq!(inner, 0x20000);
        assert_eq!(m.resolve("crashme.middle").unwrap(), 0x20019);
        assert!(m.resolve("DOINT3").unwrap() > 0x20100);
        let d = m.disassemble(0x2000, 0, 6);
        assert!(d.contains("INNER:") && d.contains("r := 100 DIV (n - n);"), "{d}");
        // the 8087 WAITs of FloatTest are known as WAITs (the loader makes them INT 3)
        let ft = m.resolve("FLOATTEST").unwrap();
        let syms = &m.board().debug.syms;
        assert!((ft..ft + 0x20).any(|a| syms.was_wait(a) == Some(true)));
        assert_eq!(syms.was_wait(inner), Some(false));
        assert_eq!(syms.name_at(inner + 0xE).as_deref(), Some("INNER+E"));
        // a breakpoint by name, and the call stack from it
        m.debug_command("b CRASHME:30", true);
        let l = m.board().debug.list();
        assert!(l.contains("CRASHME:30 (20"), "{l}");
        m.debug_command("bc", true);
        assert!(m.board().debug.named.is_empty());
    }
}
