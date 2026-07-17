//! [`Uart16550`] — a 16550 UART [`SerialIO`](crate::peripheral::serial::SerialIO) implementation.
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use uart_16550::MmioSerialPort;
use uart_16550::SerialPort as IoSerialPort;

/// An interface for writing to a Uart16550 device.
///
/// Each variant owns the underlying serial port. The owning `&mut` access required by the
/// [`SerialIO`](crate::peripheral::serial::SerialIO) methods guarantees exclusive use of the device, so no
/// interior mutability or per-operation reconstruction is required.
#[derive(Debug)]
pub enum Uart16550 {
    /// The I/O port-mapped interface for the Uart16550 serial port.
    Io(IoSerialPort),
    /// The Memory Mapped I/O interface for the Uart16550 serial port.
    Mmio(MmioSerialPort),
}

impl Uart16550 {
    /// Creates a new I/O port-mapped Uart16550 interface at the given base port.
    ///
    /// # Safety
    ///
    /// The caller must ensure `base` points to a valid Uart16550 I/O port range and that
    /// the caller has the rights to perform the I/O operations on it.
    pub const unsafe fn new_io(base: u16) -> Self {
        // SAFETY: The safety contract is forwarded to the caller of this function.
        Uart16550::Io(unsafe { IoSerialPort::new(base) })
    }

    /// Creates a new memory-mapped Uart16550 interface at the given base address and
    /// register stride.
    ///
    /// # Safety
    ///
    /// The caller must ensure `base` points to a valid, exclusively-owned Uart16550 MMIO
    /// register range with the given `reg_stride` between consecutive registers.
    pub const unsafe fn new_mmio(base: usize, reg_stride: usize) -> Self {
        // SAFETY: The safety contract is forwarded to the caller of this function.
        Uart16550::Mmio(unsafe { MmioSerialPort::new_with_stride(base, reg_stride) })
    }
}

impl crate::peripheral::serial::SerialIO for Uart16550 {
    fn init(&mut self) {
        match self {
            Uart16550::Io(port) => port.init(),
            Uart16550::Mmio(port) => port.init(),
        }
    }

    fn write(&mut self, buffer: &[u8]) {
        match self {
            Uart16550::Io(port) => {
                for b in buffer {
                    port.send(*b);
                }
            }
            Uart16550::Mmio(port) => {
                for b in buffer {
                    port.send(*b);
                }
            }
        }
    }

    fn read(&mut self) -> u8 {
        match self {
            Uart16550::Io(port) => port.receive(),
            Uart16550::Mmio(port) => port.receive(),
        }
    }

    fn try_read(&mut self) -> Option<u8> {
        match self {
            Uart16550::Io(port) => port.try_receive().ok(),
            Uart16550::Mmio(port) => port.try_receive().ok(),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use super::*;
    use crate::peripheral::serial::SerialIO;

    struct FakeMmio {
        regs: [u8; 6],
    }

    impl FakeMmio {
        // Register offsets (stride = 1).
        const DATA: usize = 0;
        const INT_EN: usize = 1;
        const FIFO_CTRL: usize = 2;
        const LINE_CTRL: usize = 3;
        const MODEM_CTRL: usize = 4;
        const LINE_STS: usize = 5;

        // Line status register bits.
        const INPUT_FULL: u8 = 1;
        const OUTPUT_EMPTY: u8 = 1 << 5;

        fn new() -> Self {
            let mut regs = [0u8; 6];
            regs[Self::LINE_STS] = Self::OUTPUT_EMPTY;
            FakeMmio { regs }
        }

        fn set_rx_byte(&mut self, byte: u8) {
            self.regs[Self::DATA] = byte;
            self.regs[Self::LINE_STS] |= Self::INPUT_FULL;
        }

        fn uart(&mut self) -> Uart16550 {
            let base = self.regs.as_mut_ptr() as usize;
            // SAFETY: `base` points to a six-byte register file (stride 1) that outlives the
            // returned interface and is exclusively owned for the duration of the test.
            unsafe { Uart16550::new_mmio(base, 1) }
        }
    }

    #[test]
    fn test_uart_16550_new_io_constructs_io_variant() {
        // SAFETY: constructing the interface does not perform any I/O; no port access occurs.
        let uart = unsafe { Uart16550::new_io(0x3F8) };
        assert!(matches!(uart, Uart16550::Io(_)));
    }

    #[test]
    fn test_uart_16550_new_mmio_constructs_mmio_variant() {
        // SAFETY: constructing the interface does not perform any I/O; no memory access occurs.
        let uart = unsafe { Uart16550::new_mmio(0x1000, 1) };
        assert!(matches!(uart, Uart16550::Mmio(_)));
    }

    #[test]
    fn test_uart_16550_mmio_init_programs_registers() {
        let mut fake = FakeMmio::new();
        fake.uart().init();

        // Values written by the 16550 default configuration (38400/8-N-1).
        assert_eq!(fake.regs[FakeMmio::DATA], 0x03);
        assert_eq!(fake.regs[FakeMmio::INT_EN], 0x01);
        assert_eq!(fake.regs[FakeMmio::FIFO_CTRL], 0xC7);
        assert_eq!(fake.regs[FakeMmio::LINE_CTRL], 0x03);
        assert_eq!(fake.regs[FakeMmio::MODEM_CTRL], 0x0B);
    }

    #[test]
    fn test_uart_16550_mmio_write_sends_byte_to_data_register() {
        let mut fake = FakeMmio::new();
        fake.uart().write(b"A");
        assert_eq!(fake.regs[FakeMmio::DATA], b'A');
    }

    #[test]
    fn test_uart_16550_mmio_write_forwards_entire_buffer() {
        let mut fake = FakeMmio::new();
        // The data register holds a single byte, so only check the final byte of the buffer.
        fake.uart().write(b"abc");
        assert_eq!(fake.regs[FakeMmio::DATA], b'c');
    }

    #[test]
    fn test_uart_16550_mmio_write_empty_buffer_is_noop() {
        let mut fake = FakeMmio::new();
        fake.uart().write(b"");
        assert_eq!(fake.regs[FakeMmio::DATA], 0);
    }

    #[test]
    fn test_uart_16550_mmio_read_returns_available_byte() {
        let mut fake = FakeMmio::new();
        fake.set_rx_byte(b'Z');
        assert_eq!(fake.uart().read(), b'Z');
    }

    #[test]
    fn test_uart_16550_mmio_try_read_returns_none_when_empty() {
        let mut fake = FakeMmio::new();
        assert_eq!(fake.uart().try_read(), None);
    }

    #[test]
    fn test_uart_16550_mmio_try_read_returns_available_byte() {
        let mut fake = FakeMmio::new();
        fake.set_rx_byte(b'!');
        assert_eq!(fake.uart().try_read(), Some(b'!'));
    }
}
