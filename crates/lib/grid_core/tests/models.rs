use grid_core::{GridMachine, GridModel};
use marty_core::bus::external::ExternalBoard;

fn machine(m: GridModel) -> GridMachine {
    let prom = vec![0xF4u8; 8192];
    GridMachine::new(m, &prom, &prom)
}

#[test]
fn model_table() {
    assert_eq!(GridModel::parse("1139"), Some(GridModel::Compass1139));
    assert_eq!(GridModel::parse("grid1121"), Some(GridModel::Compass1121));
    assert_eq!(GridModel::parse("1137"), None);
    for m in GridModel::ALL {
        let ram = if matches!(m, GridModel::Compass1101 | GridModel::Compass1121 | GridModel::Compass1131) { 256 } else { 512 };
        assert_eq!(m.ram_size(), ram * 1024, "{m:?}");
    }
    assert!(!GridModel::Compass1109.split_board());
    assert!(GridModel::Compass1129.split_board() && !GridModel::Compass1129.wide_screen());
    assert_eq!(GridModel::Compass1131.screen_size(), (512, 256));
}

#[test]
fn ram_size_and_large_screen() {
    let mut m = machine(GridModel::Compass1131);
    // 256K: nothing above 3FFFF, a read gets the address left on the bus (real 1137 u05-u08)
    let b = m.board_mut();
    assert!(b.mem_ranges().iter().any(|&(s, _)| s == 0x40000));
    assert_eq!(b.mem_read_u8(0x40002, 0), 0x02);
    assert_eq!(b.mem_read_u8(0x40203, 0), 0x02);
    assert_eq!(m.screen_mode(), (320, 240));
    // the 113x OS: video chip on, 0Ah (256K, large screen), video chip off
    let b = m.board_mut();
    b.mem_write_u8(0xDFE00, 1, 0);
    b.mem_write_u8(0xDFE04, 0x0A, 0);
    b.mem_write_u8(0xDFE00, 0, 0);
    assert_eq!(m.screen_mode(), (512, 256));
    // the PROM's 0Eh is compatibility mode
    let b = m.board_mut();
    b.mem_write_u8(0xDFE00, 1, 0);
    b.mem_write_u8(0xDFE04, 0x0E, 0);
    assert_eq!(m.screen_mode(), (320, 240));
    assert_eq!(m.render_now().len(), 512 * 256);

    // a 1129 never switches
    let mut m = machine(GridModel::Compass1129);
    let b = m.board_mut();
    assert!(b.mem_ranges().iter().any(|&(s, _)| s == 0x80000));
    b.mem_write_u8(0xDFE00, 1, 0);
    b.mem_write_u8(0xDFE04, 0x0B, 0);
    assert_eq!(m.screen_mode(), (320, 240));
}
