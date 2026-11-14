//! Per-model board hardware: timer chip, interrupt controller decode, modem/sound.

use grid_core::{GridMachine, GridModel, TimerChip};
use marty_core::bus::external::ExternalBoard;

fn machine(m: GridModel) -> GridMachine {
    let prom = vec![0xF4u8; 8192];
    GridMachine::new(m, &prom, &prom)
}

#[test]
fn chips_per_model() {
    use GridModel::*;
    for m in [Compass1101, Compass1109, Compass1121, Compass1129] {
        assert_eq!(m.timer_chip(), TimerChip::I80130, "{m:?}");
        assert!(!m.board_113x() && m.has_modem());
    }
    for m in [Compass1131, Compass1137, Compass1139] {
        assert_eq!(m.timer_chip(), TimerChip::I8254, "{m:?}");
        assert!(m.board_113x() && m.wide_screen() && m.split_board());
    }
    assert!(!Compass1137.has_modem() && Compass1139.has_modem());
    assert_eq!(Compass1137.ram_size(), 0x80000);
}

/// Read a counter through a counter latch command.
fn latched(b: &mut grid_core::GridBoard, r: u8) -> u16 {
    b.io_write_u8(0x0E, r << 6, 0);
    let lo = b.io_read_u8(0x08 + 2 * r as u16, 0) as u16;
    let hi = b.io_read_u8(0x08 + 2 * r as u16, 0) as u16;
    hi << 8 | lo
}

fn load(b: &mut grid_core::GridBoard, r: u8, mode: u8, n: u16) {
    b.io_write_u8(0x0E, r << 6 | 0x30 | mode << 1, 0);
    b.io_write_u8(0x08 + 2 * r as u16, n as u8, 0);
    b.io_write_u8(0x08 + 2 * r as u16, (n >> 8) as u8, 0);
}

#[test]
fn timers_80130() {
    for model in [GridModel::Compass1101, GridModel::Compass1121] {
        let mut m = machine(model);
        let b = m.board_mut();
        load(b, 0, 2, 1000);
        load(b, 1, 2, 500);
        load(b, 2, 3, 33);
        b.tick(1000);
        // timer 0 at CPU clock / 2
        assert_eq!(latched(b, 0), 500, "{model:?}");
        // timer 1 counts timer 0's pulses: one every 1000 timer 0 clocks = 2000 CPU clocks
        assert_eq!(latched(b, 1), 500);
        b.tick(2000 * 10);
        assert_eq!(latched(b, 1), 490);
        // the 80130 has no read-back: the command is ignored, reads give the live count
        b.io_write_u8(0x0E, 0xE2, 0);
        let v = latched(b, 0);
        assert!(v <= 1000);
        // control port reads FF, 4/6 not decoded
        assert_eq!(b.io_read_u8(0x0E, 0), 0xFF);
        assert_eq!(b.io_read_u8(0x04, 0), 0x04);
        // baud timer 2 drives the 8274
        assert_eq!(b.serial.baud_count, 33);
    }
}

#[test]
fn timers_80130_run_from_power_on() {
    let mut m = machine(GridModel::Compass1101);
    let b = m.board_mut();
    b.tick(20_000);
    assert_eq!(latched(b, 0), (65536u32 - 10_000) as u16);
}

#[test]
fn timers_8254() {
    for model in [GridModel::Compass1137, GridModel::Compass1139] {
        let mut m = machine(model);
        let b = m.board_mut();
        // nothing written yet: the counters stand at FFFF and status reads 00
        b.tick(5000);
        assert_eq!(latched(b, 0), 0xFFFF, "{model:?}");
        b.io_write_u8(0x0E, 0xE2, 0);
        assert_eq!(b.io_read_u8(0x08, 0), 0x00);
        // timer 1 loaded first stays frozen until timer 0 runs
        load(b, 1, 2, 1000);
        b.tick(300);
        assert_eq!(latched(b, 1), 1000);
        load(b, 0, 2, 1000);
        b.tick(300);
        // both at the 5 MHz CPU clock
        assert_eq!(latched(b, 0), 700);
        assert_eq!(latched(b, 1), 700);
        // read-back status of timer 0 (as GRiDProbe reads it on a real 1137: B4)
        b.io_write_u8(0x0E, 0xE2, 0);
        assert_eq!(b.io_read_u8(0x08, 0), 0xB4);
        // status + count
        b.io_write_u8(0x0E, 0xC2, 0);
        assert_eq!(b.io_read_u8(0x08, 0), 0xB4);
        assert_eq!(b.io_read_u8(0x08, 0), (700 % 256) as u8);
        assert_eq!(b.io_read_u8(0x08, 0), (700 / 256) as u8);
        // mode 3 status B6
        load(b, 2, 3, 300);
        b.io_write_u8(0x0E, 0xE8, 0);
        assert_eq!(b.io_read_u8(0x0C, 0), 0xB6);
        // 4/6 answer as the PIC, 0E reads the bus
        assert_eq!(b.io_read_u8(0x0E, 0), 0x0E);
        assert_eq!(b.io_read_u8(0x06, 0), b.io_read_u8(0x02, 0));
    }
}

#[test]
fn modem_8255_only_with_modem() {
    // 1139: the modem board's 8255 holds port B
    let mut m = machine(GridModel::Compass1139);
    let b = m.board_mut();
    b.mem_write_u8(0xDFEC2, 0x5D, 0);
    assert_eq!(b.mem_read_u8(0xDFEC2, 0), 0x5D);
    // 1137: no 8255, reads FF like other empty I/O addresses; DTMF bits still reach the MK5089 (assumed latch)
    let mut m = machine(GridModel::Compass1137);
    let b = m.board_mut();
    b.mem_write_u8(0xDFEC6, 0xC0, 0);
    b.mem_write_u8(0xDFEC2, 0xDD, 0);
    assert_eq!(b.mem_read_u8(0xDFEC2, 0), 0xFF);
    assert!(b.sound.dtmf().is_some());
    b.mem_write_u8(0xDFE42, 0x80, 0);
    assert_eq!(b.sound.dac, 0x80);
}
