/*
    External board hook for non-PC machines (GRiD Compass fork).

    An ExternalBoard owns a set of memory-mapped ranges and IO ports, can add wait states to
    plain RAM accesses (display contention) and can supply interrupt vectors. The machine
    code outside marty_core implements it and installs it with BusInterface::install_external().
*/

use std::any::Any;

pub trait ExternalBoard: Any {
    /// (address, size) ranges this board claims. Size must be a multiple of 8K (MMIO_MAP_SIZE).
    fn mem_ranges(&self) -> Vec<(usize, usize)>;
    /// IO ports this board claims.
    fn io_ports(&self) -> Vec<u16>;

    fn mem_read_u8(&mut self, address: usize, cycles: u32) -> u8;
    fn mem_peek_u8(&self, address: usize) -> u8;
    fn mem_write_u8(&mut self, address: usize, data: u8, cycles: u32);
    /// Extra wait states (CPU cycles) for an access to a claimed range.
    fn mem_wait(&mut self, _address: usize, _cycles: u32, _write: bool) -> u32 {
        0
    }
    /// Extra wait states (CPU cycles) for an access to plain RAM (not a claimed range).
    /// `now` is the absolute CPU clock count at T2 of the bus cycle.
    fn ram_wait(&mut self, _address: usize, _now: u64, _write: bool) -> u32 {
        0
    }

    fn io_read_u8(&mut self, port: u16, cycles: u32) -> u8;
    fn io_write_u8(&mut self, port: u16, data: u8, cycles: u32);
    /// Wait states for an IO access.
    fn io_wait(&mut self, _port: u16, _cycles: u32, _write: bool) -> u32 {
        0
    }

    /// Is the INTR line asserted?
    fn intr(&self) -> bool;
    /// Interrupt acknowledge: return the vector.
    fn inta(&mut self) -> Option<u8>;

    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
