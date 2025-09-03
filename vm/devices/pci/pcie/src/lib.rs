// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! PCI Express definitions and emulators.

use chipset_device::ChipsetDevice;
use chipset_device::io::IoError;
use chipset_device::io::IoResult;
use chipset_device::mmio::ControlMmioIntercept;
use chipset_device::mmio::MmioIntercept;
use chipset_device::mmio::RegisterMmioIntercept;
use inspect::Inspect;
use inspect::InspectMut;
use pci_bus::GenericPciBusDevice;
use std::collections::HashMap;
use vmcore::device_state::ChangeDeviceState;
use zerocopy::IntoBytes;

/// A generic PCI Express root complex emulator.
#[derive(InspectMut)]
pub struct GenericPcieRootComplex {
    /// The segment number on which the root complex resides.
    _segment: u16,
    /// The lowest valid bus number under the root complex.
    start_bus: u8,
    /// The highest valid bus number under the root complex.
    end_bus: u8,
    /// Intercept control for the ECAM MMIO region.
    ecam: Box<dyn ControlMmioIntercept>,
    /// Map of root ports attached to the root complex, indexed by combined device and function numbers.
    #[inspect(iter_by_key)]
    ports: HashMap<u8, GenericPcieRootPort>,
}

impl GenericPcieRootComplex {
    /// Constructs a new `GenericPcieRootComplex` emulator.
    pub fn new(
        register_mmio: &mut dyn RegisterMmioIntercept,
        segment: u16,
        start_bus: u8,
        end_bus: u8,
        ecam_base: u64,
        ports: HashMap<u8, GenericPcieRootPort>

    ) -> Self {
        let bus_count = (end_bus as u16) - (start_bus as u16) + 1;
        let ecam_size =  (bus_count as u64) * 256 * 4096;
        let mut ecam = register_mmio.new_io_region("ecam", ecam_size);
        ecam.map(ecam_base);

        Self {
            _segment: segment,
            start_bus,
            end_bus,
            ecam,
            ports,
        }
    }

    fn decode_ecam_access(&self, addr: u64) -> (u8, u8, u16) {
        let ecam_offset = self.ecam.offset_of(addr).unwrap();
        let cfg_offset = (ecam_offset % 4096) as u16;
        let bdf = (ecam_offset / 4096) & 0xFFFF;
        let bus = ((bdf & 0xFF00) >> 8) as u8;
        let device_function = (bdf & 0xFF) as u8;

        (bus, device_function, cfg_offset)
    }
}

fn shift_read_value(cfg_offset: u16, len: usize, value: u32) -> u32 {
    let shift = (cfg_offset & 0x3) * 8;
    match len {
        4 => value,
        2 => value >> shift & 0xFFFF,
        1 => value >> shift & 0xFF,
        _ => unreachable!(),
    }
}

fn combine_old_new_values(cfg_offset: u16, old_value: u32, new_value: u32, len: usize) -> u32 {
    let shift = (cfg_offset & 0x3) * 8;
    let mask = (1 << (len * 8)) - 1;
    (old_value & !(mask << shift)) | (new_value << shift)
}

impl ChangeDeviceState for GenericPcieRootComplex {
    fn start(&mut self) {}

    async fn stop(&mut self) {}

    async fn reset(&mut self) {}
}

impl ChipsetDevice for GenericPcieRootComplex {
    fn supports_mmio(&mut self) -> Option<&mut dyn MmioIntercept> {
        Some(self)
    }
}

impl MmioIntercept for GenericPcieRootComplex {
    fn mmio_read(&mut self, addr: u64, data: &mut [u8]) -> IoResult {
        if !matches!(data.len(), 1 | 2 | 4) {
            return IoResult::Err(IoError::InvalidAccessSize);
        }

        if !(((data.len() == 4) && (addr & 3 == 0))
            || ((data.len() == 2) && (addr & 1 == 0))
            || (data.len() == 1))
        {
            return IoResult::Err(IoError::UnalignedAccess);
        }

        let (bus, device_function, cfg_offset) = self.decode_ecam_access(addr);
        tracing::debug!("ecam read addr 0x{:016x} decodes to offset 0x{:04x} on {:02x}:{:02x}",
            addr, cfg_offset, bus, device_function);

        let mut value = !0;
        if bus == self.start_bus {
            // Access on the internal "root bus" of the root complex.
            if let Some(port) = self.ports.get(&device_function) {
                let _ = port.pci_cfg_read(cfg_offset & !3, &mut value);
            }
        } else if bus > self.start_bus && bus <= self.end_bus {
            // Accessing a different bus number that's valid within this root complex, forward down
            // the port based on bus number assignments.
        } else {
            tracing::error!("unexpected intercept")
        }

        let value = shift_read_value(cfg_offset, data.len(), value);
        data.copy_from_slice(&value.as_bytes()[..data.len()]);
        IoResult::Ok
    }

    fn mmio_write(&mut self, addr: u64, data: &[u8]) -> IoResult {
        if !matches!(data.len(), 1 | 2 | 4) {
            return IoResult::Err(IoError::InvalidAccessSize);
        }

        if !(((data.len() == 4) && (addr & 3 == 0))
            || ((data.len() == 2) && (addr & 1 == 0))
            || (data.len() == 1))
        {
            return IoResult::Err(IoError::UnalignedAccess);
        }

        let (bus, device_function, cfg_offset) = self.decode_ecam_access(addr);
        tracing::debug!("ecam write addr 0x{:016x} decodes to offset 0x{:04x} on {:02x}:{:02x}",
            addr, cfg_offset, bus, device_function);

        let write_value = {
            let mut temp: u32 = 0;
            temp.as_mut_bytes()[..data.len()].copy_from_slice(data);
            temp
        };

        if bus == self.start_bus {
            // Access on the internal "root bus" of the root complex.
            if let Some(port) = self.ports.get_mut(&device_function) {
                let rounded_offset = cfg_offset & !3;
                let merged_value = if data.len() == 4 {
                    write_value
                } else {
                    let mut temp: u32 = 0;
                    let _ = port.pci_cfg_read(rounded_offset, &mut temp);
                    combine_old_new_values(cfg_offset, temp, write_value, data.len())
                };

                let _ = port.pci_cfg_write(rounded_offset, merged_value);
            } else {
                tracing::trace!("invalid root bus access device_function 0x{:02x}", device_function);
            }
        } else if bus > self.start_bus && bus <= self.end_bus {
            // Accessing a different bus number that's valid within this root complex.
            //tracing::trace!("forwarding config space down ports is not implemented yet")
        } else {
            tracing::error!("unexpected intercept")
        }

        IoResult::Ok
    }
}

#[derive(Inspect)]
/// A generic PCI Express root port emulator, to be used
/// in conjunction with the generic root complex.
pub struct GenericPcieRootPort {
    // Minimal type 1 configuration space emulation for
    // Linux and Windows to enumerate the port. This should
    // be refactored into a dedicated type 1 emulator.
    command_status_register: u32,
    bus_number_registers: u32,
    memory_limit_registers: u32,
    prefetch_limit_registers: u32,
    prefetch_base_upper_register: u32,
    prefetch_limit_upper_register: u32,

    #[inspect(skip)]
    _link: Option<Box<dyn GenericPciBusDevice>>,
}

impl GenericPcieRootPort {
    /// Constructs a new `GenericPcieRootPort` emulator.
    pub fn new() -> Self {
        Self {
            command_status_register: 0,
            bus_number_registers: 0,
            memory_limit_registers: 0,
            prefetch_limit_registers: 0,
            prefetch_base_upper_register: 0,
            prefetch_limit_upper_register: 0,
            _link: None,
        }
    }

    fn pci_cfg_read(&self, offset: u16, value: &mut u32) -> IoResult {
        *value = match offset {
            0x00 => 0xF111_1414, // Device and Vendor IDs
            0x04 => self.command_status_register | 0x0010_0000,
            0x08 => 0x0604_0000, // Class code and revision
            0x0C => 0x0001_0000, // Header type 1
            0x10 => 0x0000_0000, // BAR0
            0x14 => 0x0000_0000, // BAR1
            0x18 => self.bus_number_registers,
            0x1C => 0x0000_0000, // Secondary status and I/O range
            0x20 => self.memory_limit_registers,
            0x24 => self.prefetch_limit_registers,
            0x28 => self.prefetch_base_upper_register,
            0x2C => self.prefetch_limit_upper_register,
            0x30 => 0x0000_0000, // I/O base and limit 16 bit
            0x34 => 0x0000_0040, // Reserved and Capability pointer
            0x38 => 0x0000_0000, // Expansion ROM
            0x3C => 0x0000_0000, // Bridge control, interrupt pin/line

            // PCI Express capability structure
            0x40 => 0x0142_0010, // Capability header and PCI Express capabilities register
            0x44 => 0x0000_0000, // Device capabilities register
            0x48 => 0x0000_0000, // Device control and status registers
            0x4C => 0x0000_0000, // Link capabilities register
            0x50 => 0x0011_0000, // Link control and status registers
            0x54 => 0x0000_0000, // Slot capabilities register
            0x58 => 0x0000_0000, // Slot status and control registers
            0x5C => 0x0000_0000, // Root capabilities and control registers
            0x60 => 0x0000_0000, // Root status register
            0x64 => 0x0000_0000, // Device capabilities 2 register
            0x68 => 0x0000_0000, // Device status 2 and control 2 registers
            0x6C => 0x0000_0000, // Link capabilities 2 register
            0x70 => 0x0000_0000, // Link status 2 and control 2 registers
            0x74 => 0x0000_0000, // Slot capabilities 2 register
            0x78 => 0x0000_0000, // Slot status 2 and control 2 registers

            _ => 0xFFFF
        };

        tracing::trace!("config space read 0x{:04X}: 0x{:08X}", offset, value);
        IoResult::Ok
    }

    fn pci_cfg_write(&mut self, offset: u16, value: u32) -> IoResult {
        match offset {
            0x04 => { self.command_status_register = value },
            0x18 => { self.bus_number_registers = value },
            0x20 => { self.memory_limit_registers = value },
            0x24 => { self.prefetch_limit_registers = value },
            0x28 => { self.prefetch_base_upper_register = value },
            0x2C => { self.prefetch_limit_upper_register = value },
            _ => {}
        };
        IoResult::Ok
    }
}

mod save_restore {
    use super::*;
    use vmcore::save_restore::RestoreError;
    use vmcore::save_restore::SaveError;
    use vmcore::save_restore::SaveRestore;

    mod state {
        use mesh::payload::Protobuf;
        use vmcore::save_restore::SavedStateRoot;

        #[derive(Protobuf, SavedStateRoot)]
        #[mesh(package = "pcie.rc")]
        pub struct SaveState {
        }
    }

    impl SaveRestore for GenericPcieRootComplex {
        type SavedState = state::SaveState;

        fn save(&mut self) -> Result<Self::SavedState, SaveError> {
            Err(SaveError::NotSupported)
        }

        fn restore(&mut self, _state: Self::SavedState) -> Result<(), RestoreError> {
            Err(RestoreError::InvalidSavedState(anyhow::anyhow!("not supported")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_op() {
        assert_eq(1, 1);
    }
}