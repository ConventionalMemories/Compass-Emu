//! GRiD Compass machine on top of MartyPC's 8086 core.
//!
//! Models 1101/1109 (8086 @ 5 MHz, 256K/512K RAM, 320x240 screen at 0400h, 80130, 8741
//! keyboard, TMS9914 GPIB, 7220 bubble, 8274 serial, MM58174 RTC, modem board with sound
//! DACs) and the split board Compass II 1121/1129/1131/1139 (113x: 512x256 screen).
//! Memory map follows MAME's gridcomp.cpp and GRiD's own CP sources.

pub mod board;
pub mod bubble;
pub mod serial;
pub mod sound;
pub mod fpu;
pub mod gpib;
pub mod keyboard;
pub mod upi41;
pub mod rtc;
pub mod machine;
pub mod screen;
pub mod roms;
pub mod session;
pub mod fragment;

pub use board::GridBoard;
pub use machine::{GridMachine, GridModel};
