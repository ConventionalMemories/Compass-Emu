//! Windows-only keyboard help. Alt is the Compass CODE key, but Windows sends Alt and
//! Alt+key as "system" keys: releasing Alt opens the window's menu mode (the next keys go to
//! the menu) and Alt+key beeps. We subclass the window to swallow SC_KEYMENU and WM_SYSCHAR,
//! and read the keys straight from GetAsyncKeyState so Alt combinations and quick taps are seen.

use std::sync::atomic::{AtomicIsize, Ordering};

type Hwnd = *mut core::ffi::c_void;

#[link(name = "user32")]
extern "system" {
    fn GetAsyncKeyState(vk: i32) -> i16;
    fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
    fn CallWindowProcW(prev: isize, hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn GetForegroundWindow() -> Hwnd;
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
    (s & 0x8000 != 0, s & 0x0001 != 0)
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
