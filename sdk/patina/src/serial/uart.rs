//! [SerialIO](crate::serial::SerialIO) UART implementations.
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

/// A null (stub) device that does nothing.
#[derive(Debug)]
pub struct UartNull {}

impl super::SerialIO for UartNull {
    fn init(&mut self) {}

    fn write(&mut self, _buffer: &[u8]) {}

    fn read(&mut self) -> u8 {
        // PANIC: Would loop forever, better to panic.
        panic!();
    }

    fn try_read(&mut self) -> Option<u8> {
        None
    }
}

cfg_if::cfg_if! {
    if #[cfg(all(target_arch = "x86_64", any(target_os = "uefi", feature = "doc")))] {

        use uart_16550::MmioSerialPort;
        use uart_16550::SerialPort as IoSerialPort;

        /// An interface for writing to a Uart16550 device.
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

        impl super::SerialIO for Uart16550 {
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
    }
}

cfg_if::cfg_if! {
    if #[cfg(any(feature = "doc", all(target_os = "uefi", target_arch = "aarch64")))] {
        use core::ptr::NonNull;
        use crate::mmio::{field, fields::{ReadPure, ReadWrite}, UniqueMmioPointer};

        /// PL011 flag register bit: UART busy.
        const FR_BUSY: u8 = 1 << 3;
        /// PL011 flag register bit: receive FIFO empty.
        const FR_RXFE: u8 = 1 << 4;
        /// PL011 flag register bit: transmit FIFO full.
        const FR_TXFF: u8 = 1 << 5;

        /// PL011 MMIO register block.
        ///
        /// Models the Data Register (DR) at offset 0x00 and the Flag Register (FR) at offset 0x18.
        /// Intermediate registers are represented as reserved padding.
        #[repr(C)]
        struct Pl011Registers {
            /// Data Register: reading pops from receive FIFO (side-effect), writing pushes to
            /// transmit FIFO.
            dr: ReadWrite<u8>,
            /// Reserved registers between DR (0x00) and FR (0x18).
            _reserved: [u8; 0x17],
            /// Flag Register: reading has no side-effects (pure status bits).
            fr: ReadPure<u8>,
        }

        /// An interface for writing to a UartPl011 device.
        #[derive(Debug)]
        pub struct UartPl011 {
            /// Owned pointer to the PL011 MMIO register block.
            regs: UniqueMmioPointer<'static, Pl011Registers>,
        }

        impl UartPl011 {
            /// Constructs a new instance of the UART driver for a PL011 device at the
            /// given base address.
            ///
            /// # Safety
            ///
            /// The given base address must point to the MMIO control registers of a
            /// PL011 device, which must be mapped into the address space of the process
            /// as device memory and not have any other aliases for the lifetime of the
            /// returned instance.
            pub const unsafe fn new(base_address: usize) -> Self {
                // SAFETY: The caller guarantees `base_address` points to an exclusively-owned PL011
                // register block mapped as device memory, satisfying `UniqueMmioPointer::new`.
                let regs = unsafe {
                    UniqueMmioPointer::new(NonNull::new(base_address as *mut Pl011Registers).unwrap())
                };
                Self { regs }
            }

            /// Writes a single byte to the UART.
            pub fn write_byte(&mut self, byte: u8) {
                let mut regs = self.regs.reborrow();

                // Wait until there is room in the TX buffer.
                while field!(regs, fr).read() & FR_TXFF != 0 {}

                // Write to the TX buffer.
                field!(regs, dr).write(byte);

                // Wait until the UART is no longer busy.
                while field!(regs, fr).read() & FR_BUSY != 0 {}
            }

            /// Reads a single byte from the UART.
            pub fn read_byte(&mut self) -> Option<u8> {
                let mut regs = self.regs.reborrow();

                // Check if the RX buffer is empty.
                if field!(regs, fr).read() & FR_RXFE != 0 {
                    return None;
                }

                // Read from the RX buffer.
                Some(field!(regs, dr).read())
            }
        }

        impl super::SerialIO for UartPl011 {
            fn init(&mut self) {}

            fn write(&mut self, buffer: &[u8]) {
                for byte in buffer {
                    self.write_byte(*byte);
                }
            }

            fn read(&mut self) -> u8 {
                loop {
                    if let Some(byte) = self.read_byte() {
                        return byte;
                    }
                }
            }

            fn try_read(&mut self) -> Option<u8> {
                self.read_byte()
            }
        }
    }
}
