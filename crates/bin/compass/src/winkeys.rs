//! Windows-only keyboard help. Alt is the Compass CODE key, but Windows sends Alt and
//! Alt+key as "system" keys: releasing Alt opens the window's menu mode (the next keys go to
//! the menu) and Alt+key beeps. We subclass the window to swallow SC_KEYMENU and WM_SYSCHAR,
//! and read the keys straight from GetAsyncKeyState so Alt combinations and quick taps are seen.
//! A low-level keyboard hook keeps Windows from acting on Alt+Esc (send window back) and
//! Alt+Space (window menu) while the emulator has focus; those keys then go to the Compass
//! as CODE-Esc and CODE-Space. Alt+Tab and Alt+F4 still work.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

type Hwnd = *mut core::ffi::c_void;

#[link(name = "user32")]
extern "system" {
    fn GetAsyncKeyState(vk: i32) -> i16;
    fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
    fn CallWindowProcW(prev: isize, hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn GetForegroundWindow() -> Hwnd;
    fn SetWindowsHookExW(id: i32, proc_: usize, hmod: Hwnd, thread: u32) -> Hwnd;
    fn CallNextHookEx(hook: Hwnd, code: i32, wparam: usize, lparam: isize) -> isize;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Hwnd;
}

const WH_KEYBOARD_LL: i32 = 13;
const LLKHF_ALTDOWN: u32 = 0x20;
const VK_ESCAPE: u32 = 0x1B;
const VK_SPACE: u32 = 0x20;

#[repr(C)]
struct KbdLl {
    vk: u32,
    scan: u32,
    flags: u32,
    time: u32,
    extra: usize,
}

/// Esc and Space as the hook saw them while it kept them from Windows: (down, pressed since
/// last poll).
static ESC: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static SPACE: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

unsafe extern "system" fn kbd_hook(code: i32, wparam: usize, lparam: isize) -> isize {
    if code == 0 && lparam != 0 {
        let k = &*(lparam as *const KbdLl);
        let st = match k.vk {
            VK_ESCAPE => Some(&ESC),
            VK_SPACE => Some(&SPACE),
            _ => None,
        };
        if let Some(st) = st {
            let up = wparam == 0x101 || wparam == 0x105;
            let held_here = st[0].load(Ordering::Relaxed);
            // swallow Alt+key while we have focus, and its key-up whatever Alt does by then
            if (focused() && k.flags & LLKHF_ALTDOWN != 0 && !up) || (held_here && up) {
                st[0].store(!up, Ordering::Relaxed);
                if !up {
                    st[1].store(true, Ordering::Relaxed);
                }
                return 1;
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

const GWLP_WNDPROC: i32 = -4;
const WM_SYSCOMMAND: u32 = 0x0112;
const WM_SYSCHAR: u32 = 0x0106;
const SC_KEYMENU: usize = 0xF100;

static OLD_PROC: AtomicIsize = AtomicIsize::new(0);
static mut HWND_MAIN: usize = 0;

unsafe extern "system" fn wnd_proc(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize {
    if msg == WM_SYSCOMMAND && (wparam & 0xFFF0) == SC_KEYMENU {
        return 0;
    }
    if msg == WM_SYSCHAR {
        return 0;
    }
    CallWindowProcW(OLD_PROC.load(Ordering::Relaxed), hwnd, msg, wparam, lparam)
}

pub fn install(hwnd: *mut core::ffi::c_void) {
    unsafe {
        HWND_MAIN = hwnd as usize;
        let old = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, wnd_proc as usize as isize);
        OLD_PROC.store(old, Ordering::Relaxed);
        // (runs on this thread's message loop, which minifb pumps every frame)
        SetWindowsHookExW(WH_KEYBOARD_LL, kbd_hook as usize, GetModuleHandleW(std::ptr::null()), 0);
    }
}

fn focused() -> bool {
    unsafe { GetForegroundWindow() as usize == HWND_MAIN }
}

/// (down now, pressed since last poll) for a virtual key, only while our window has focus.
pub fn vk(vk: i32) -> (bool, bool) {
    if !focused() {
        return (false, false);
    }
    let s = unsafe { GetAsyncKeyState(vk) } as u16;
    let (mut d, mut t) = (s & 0x8000 != 0, s & 0x0001 != 0);
    let hooked = match vk as u32 {
        VK_ESCAPE => Some(&ESC),
        VK_SPACE => Some(&SPACE),
        _ => None,
    };
    if let Some(h) = hooked {
        d |= h[0].load(Ordering::Relaxed);
        t |= h[1].swap(false, Ordering::Relaxed);
    }
    (d, t)
}

/// Compass characters (as used by keyboard::key_for_char) and their Windows virtual keys.
pub const KEYS: &[(char, i32)] = &[
    ('a', 0x41), ('b', 0x42), ('c', 0x43), ('d', 0x44), ('e', 0x45), ('f', 0x46), ('g', 0x47),
    ('h', 0x48), ('i', 0x49), ('j', 0x4A), ('k', 0x4B), ('l', 0x4C), ('m', 0x4D), ('n', 0x4E),
    ('o', 0x4F), ('p', 0x50), ('q', 0x51), ('r', 0x52), ('s', 0x53), ('t', 0x54), ('u', 0x55),
    ('v', 0x56), ('w', 0x57), ('x', 0x58), ('y', 0x59), ('z', 0x5A),
    ('0', 0x30), ('1', 0x31), ('2', 0x32), ('3', 0x33), ('4', 0x34), ('5', 0x35), ('6', 0x36),
    ('7', 0x37), ('8', 0x38), ('9', 0x39),
    (' ', 0x20), ('\r', 0x0D), ('\t', 0x09), ('\x1b', 0x1B), ('\x08', 0x08),
    ('-', 0xBD), ('=', 0xBB), (',', 0xBC), ('.', 0xBE), ('/', 0xBF), (';', 0xBA), ('\'', 0xDE),
    ('\u{e000}', 0x25), ('\u{e003}', 0x26), ('\u{e002}', 0x27), ('\u{e001}', 0x28),
];
pub const VK_SHIFT: i32 = 0x10;
pub const VK_CONTROL: i32 = 0x11;
pub const VK_MENU: i32 = 0x12;
