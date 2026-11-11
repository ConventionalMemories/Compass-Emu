//! Running the machine in real time (or turbo) a frame at a time, shared by the GUI and the
//! classic window: pacing, the keyboard matrix, pause and the debugger console.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use grid_core::{screen, GridMachine};

/// Shortest time a key stays down for the Compass (CPU clocks, 60 ms), so quick taps are
/// seen by the 8741 scan.
const MIN_HOLD: u64 = 300_000;
/// Shortest time between two presses of one key (CPU clocks) so the second is seen.
const KEY_GAP: u64 = 250_000;
/// Shortest time between two queued presses (CPU clocks, 20 ms).
const STAGGER: u64 = 100_000;

pub struct Runner {
    pub paused: bool,
    pub turbo: bool,
    /// Real-time pacing: emulated clock `base_clock` matched wall time `base_wall`.
    base_wall: Instant,
    base_clock: u64,
    /// key position -> (press clock, still held, modifiers it was typed with)
    held: HashMap<(usize, u8), (u64, bool, u8)>,
    /// Clock at which each key was last let go (so a repeated tap gets a gap the scan sees).
    released: HashMap<(usize, u8), u64>,
    /// Taps waiting for the same key's previous press and gap to end ("ll" in "hello").
    pending: Vec<((usize, u8), u8)>,
    /// Clock of the last queued press.
    last_start: u64,
    /// Emulated speed (1.0 = a real Compass), updated about once a second.
    pub speed: f64,
    /// Frames shown per second.
    pub fps: f64,
    speed_wall: Instant,
    speed_clock: u64,
    frames: u32,
}

impl Runner {
    pub fn new(m: &GridMachine) -> Runner {
        Runner {
            paused: false,
            turbo: false,
            base_wall: Instant::now(),
            base_clock: m.board().clocks,
            held: HashMap::new(),
            released: HashMap::new(),
            pending: Vec::new(),
            last_start: 0,
            speed: 0.0,
            fps: 0.0,
            speed_wall: Instant::now(),
            speed_clock: m.board().clocks,
            frames: 0,
        }
    }

    /// Start pacing afresh from now (after a pause, turbo, a reset or a loaded state).
    pub fn resync(&mut self, m: &GridMachine) {
        self.base_wall = Instant::now();
        self.base_clock = m.board().clocks;
        self.speed_wall = Instant::now();
        self.speed_clock = m.board().clocks;
        self.held.clear();
        self.released.clear();
        self.pending.clear();
    }

    pub fn set_turbo(&mut self, m: &GridMachine, on: bool) {
        self.turbo = on;
        self.resync(m);
    }

    pub fn set_paused(&mut self, m: &GridMachine, on: bool) {
        if self.paused && !on {
            self.resync(m);
        }
        self.paused = on;
    }

    /// Keys held now plus keys tapped since the last frame (matrix positions). Taps are
    /// queued and pressed one at a time in the order typed, each held at least MIN_HOLD, at
    /// least STAGGER after the one before (two new keys in one keyboard scan would come out
    /// in matrix order) and KEY_GAP after the same key was let go (so "ll" is two presses).
    pub fn set_keys(&mut self, m: &mut GridMachine, down: &[(usize, u8)], tapped: &[((usize, u8), u8)], mods: u8) {
        let now = m.board().clocks;
        self.pending.extend_from_slice(tapped);
        if let Some(&(p, pm)) = self.pending.first() {
            let gap_over = self.released.get(&p).map(|&t| now.saturating_sub(t) >= KEY_GAP).unwrap_or(true);
            // (the keyboard chip stops scanning with three matrix keys down)
            // (and a key typed with other modifiers waits until the last tap is let go)
            let same_mods = self.held.values().all(|st| st.1 || st.2 == pm);
            if !self.held.contains_key(&p) && self.held.len() < 2 && same_mods && gap_over && now.saturating_sub(self.last_start) >= STAGGER {
                self.pending.remove(0);
                self.held.insert(p, (now, true, pm));
                self.last_start = now;
            }
        }
        // Keys held longer than a tap stay down while they are held (a key still waiting in
        // the queue doesn't count yet).
        for (p, st) in self.held.iter_mut() {
            st.1 = down.contains(p) && !self.pending.iter().any(|q| q.0 == *p);
        }
        let released = &mut self.released;
        self.held.retain(|p, st| {
            let keep = st.1 || now.saturating_sub(st.0) < MIN_HOLD;
            if !keep {
                released.insert(*p, now);
            }
            keep
        });
        // A queued tap is pressed with the Shift/Ctrl/Code it was typed with, even if those
        // are let go by then.
        let mut cols = [0u8; 8];
        let mut mods = mods;
        for ((c, b), st) in self.held.iter() {
            cols[*c] |= *b;
            if !st.1 {
                mods |= st.2;
            }
        }
        if let Some(k) = &mut m.board_mut().keyboard {
            k.set_matrix(cols, mods);
        }
    }

    /// A debugger console command; returns its output.
    pub fn command(&mut self, m: &mut GridMachine, line: &str) -> String {
        let (out, p) = m.debug_command(line, self.paused);
        self.set_paused(m, p);
        out
    }

    /// Run one frame's worth: real time, or as much as fits in ~15 ms in turbo. Returns a
    /// message when a breakpoint stopped the machine (or a profile finished).
    pub fn run_frame(&mut self, m: &mut GridMachine) -> Option<String> {
        self.frames += 1;
        let mut why = None;
        if self.paused {
            // stopped in the debugger: only the window runs
        }
        else if self.turbo {
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(15) {
                if let Some(w) = m.run_debug(50_000) {
                    why = Some(w);
                    break;
                }
            }
        }
        else {
            let target = self.base_clock + (self.base_wall.elapsed().as_secs_f64() * screen::CPU_HZ) as u64;
            let now = m.board().clocks;
            let max_step = (0.1 * screen::CPU_HZ) as u64;
            let behind = target.saturating_sub(now);
            if behind > max_step || now > target + max_step {
                // Too far behind (slow PC, window dragged) or ahead: start pacing afresh.
                self.base_wall = Instant::now();
                self.base_clock = now + behind.min(max_step);
            }
            let behind = behind.min(max_step);
            if behind > 0 {
                why = m.run_debug(behind);
            }
        }
        let t = self.speed_wall.elapsed().as_secs_f64();
        if t >= 1.0 {
            let c = m.board().clocks;
            self.speed = (c - self.speed_clock) as f64 / screen::CPU_HZ / t;
            self.fps = self.frames as f64 / t;
            self.frames = 0;
            self.speed_clock = c;
            self.speed_wall = Instant::now();
        }
        let why = why?;
        if why.starts_with("profile done") {
            return Some(why);
        }
        self.paused = true;
        let (cs, ip) = m.cs_ip();
        Some(format!("{why}\n{}\n{}", m.reg_dump(), m.disassemble(cs, ip, 3)))
    }
}
