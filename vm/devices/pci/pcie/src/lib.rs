// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! PCI Express definitions and implementation.

use anyhow::Context;
use inspect::Inspect;
use inspect::InspectMut;
use mesh::payload::Protobuf;
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

use chipset_device::ChipsetDevice;
use chipset_device::io::IoError;
use chipset_device::io::IoResult;
use chipset_device::mmio::ControlMmioIntercept;
use chipset_device::mmio::MmioIntercept;
use chipset_device::mmio::RegisterMmioIntercept;
use pci_bus::GenericPciBusDevice;
use vmcore::device_state::ChangeDeviceState;
use vmcore::save_restore::SaveError;
use vmcore::save_restore::SaveRestore;
use vmcore::save_restore::SavedStateNotSupported;

#[derive(Copy, Clone, Debug, Inspect)]
/// Represents a range of bus numbers on a PCIe segment.
pub struct SegmentBusRange {
    /// The segment number where the bus range is valid.
    pub segment: 16,
    /// The lowest valid bus number in the range.
    pub start_bus: u8,
    /// The highest valid bus number in the range.
    pub end_bus: u8,
}

impl SegmentBusRange {
    /// Construct a new bus range.
    pub fn new(segment: u16, start_bus: u8, end_bus: u8) -> Self {
        assert!(end_bus >= start_bus);
        Self {
            segment,
            start_bus,
            end_bus,
        }
    }

    /// The number of valid bus numbers in the range.
    pub fn count(&self) -> u16 {
        (self.end_bus as u16) - (self.start_bus as u16) + 1
    }
}

/// A range of memory addresses
pub struct MemoryWindow {
    pub base_address: u64,
    pub size: u64,
}

/// A chipset device emulator for a PCIe root complex.
#[derive(InspectMut)]
pub struct RootComplexEmulator {
    /// The range of valid bus numbers under the root complex.
    segment_bus_range: SegmentBusRange,
    /// The window of accessible memory resources under the root complex.
    memory_window: MemoryWindow,
    /// The window of accessible prefetchable memory resources under the
    /// root complex.
    prefetch_window: MemoryWindow,
    /// Intercept control for the ECAM region.
    ecam: Box<dyn ControlMmioIntercept>,
    /// Map of registered root ports under the root complex.
    root_ports: Vec<RootPort>,

    // PCIE_TODO: resources?
}

///
struct DownstreamPortEmulator {
}

pub struct RootPortEmulator {
    rid: Rid,
    downstream_port: DownstreamPortEmulator,
}

impl RootComplexEmulator {
    /// Construct a new `RootComplex`
    pub fn new(
        register_mmio: &mut dyn RegisterMmioIntercept,
        description: RootComplexDescription,
        ecam_base: u64
    ) -> Self {
        let ecam_size = (description.bus_range.count() as u64) * 255 * 4096;
        let mut ecam = register_mmio.new_io_region("ecam", ecam_size);
        ecam.map(ecam_base);
        Self {
            description,
            ecam,
            functions: BTreeMap::new(),
        }
    }

    /// Returns the segment number of the root complex.
    //pub fn segment(&self) -> u16 {
    //    self.segment_id
    //}

    /// Returns the range of valid busses under the root complex.
    //pub fn bus_range(&self) -> BusRange {
    //    self.bus_range
    //}

    /// Returns the base address of the ECAM region for the root complex.
    pub fn ecam_base(&self) -> u64 {
        self.ecam.addr().expect("ecam not mapped")
    }

    /// Try to add a device, returning (device, existing_device_name) if the
    /// rid is already occupied.
    pub fn add_pcie_function<D: GenericPciBusDevice>(
        &mut self,
        rid: Rid,
        name: impl AsRef<str>,
        dev: D,
    ) -> Result<(), (D, Arc<str>)> {
        if let Some((name, _)) = self.functions.get(&rid) {
            return Err((dev, name.clone()));
        }
        self.functions
            .insert(rid, (name.as_ref().into(), Box::new(dev)));
        Ok(())
    }

    fn parse_ecam_access(&self, address: u64, size: usize) -> Result<Rid, IoError> {
        if !matches!(size, 1 | 2 | 4) {
            return Err(IoError::InvalidAccessSize);
        }

        if !((size == 4 && address & 3 == 0)
            || (size == 2 && address & 1 == 0)
            || (size == 1))
        {
            return Err(IoError::UnalignedAccess);
        }

        let ecam_offset = self.ecam.offset_of(address).expect("unregistered intercept");
        let bdf = (ecam_offset / 4096) & 0xFFFF;

        Ok(Rid {
            segment: self.description.segment_id,
            bus: ((bdf & 0xFF00) >> 8) as u8,
            device_function: (bdf & 0xFF) as u8,
        })
    }
}

impl ChipsetDevice for RootComplexEmulator {
    fn supports_mmio(&mut self) -> Option<&mut dyn MmioIntercept> {
        Some(self)
    }
}

impl ChangeDeviceState for RootComplexEmulator {
    fn start(&mut self) {}
    async fn stop(&mut self) {}
    async fn reset(&mut self) {}
}

impl SaveRestore for RootComplexEmulator {
    type SavedState = SavedStateNotSupported;

    fn save(&mut self) -> Result<Self::SavedState, SaveError> {
        Err(SaveError::NotSupported)
    }

    fn restore(
        &mut self,
        state: Self::SavedState,
    ) -> Result<(), vmcore::save_restore::RestoreError> {
        match state {}
    }
}

impl MmioIntercept for RootComplexEmulator {
    fn mmio_read(&mut self, addr: u64, data: &mut [u8]) -> IoResult {
        let _rid = match self.parse_ecam_access(addr, data.len()) {
            Ok(rid) => rid,
            Err(err) => return IoResult::Err(err),
        };

        //let mut value = 0;
        //match self.pci_devices.get_mut(&address) {
        //    Some((name, device)) => {
        //        let res = device.pci_cfg_read(cfg_offset.try_into().unwrap(), &mut value);
        //        if let Some(_result) = res {
        //            tracing::info!(
        //                device = &**name,
        //                %address,
        //                cfg_offset,
        //                value,
        //                "cfg space read"
        //            );
        //        } else {
        //            // TODO: should probably unregister from bus?
        //            // but then again, shouldn't the device do that as part of
        //            // its destructor?
        //            tracelimit::warn_ratelimited!(
        //                device = &**name,
        //                %address,
        //                cfg_offset,
        //                "cfg space read failed, device went away"
        //            );
        //            value = !0;
        //        }
        //    }
        //    None => {
        //        tracing::trace!(%address, "no device found - returning F's");
        //        value = !0;
        //    }
        //}

        //data.copy_from_slice(&value.as_bytes()[..data.len()]);
        IoResult::Ok
    }

    fn mmio_write(&mut self, addr: u64, data: &[u8]) -> IoResult {
        let _rid = match self.parse_ecam_access(addr, data.len()) {
            Ok(rid) => rid,
            Err(err) => return IoResult::Err(err),
        };

        //match self.pci_devices.get_mut(&address) {
        //    Some((name, device)) => {
        //        let res = device.pci_cfg_write(cfg_offset.try_into().unwrap(), new_value);
        //        if let Some(result) = res {
        //            tracing::info!(
        //                device = &**name,
        //                %address,
        //                cfg_offset,
        //                new_value,
        //                "cfg space write"
        //            );
        //            result
        //        } else {
        //            // TODO: should probably unregister from bus?
        //            // but then again, shouldn't the device do that as part of
        //            // its destructor?
        //            tracelimit::warn_ratelimited!(
        //                device = &**name,
        //                %address,
        //                cfg_offset,
        //                "cfg space write failed, device went away"
        //            );
        //            IoResult::Ok
        //        }
        //    }
        //    None => {
        //        tracing::trace!(%address, "no device found - dropping");
        //        IoResult::Ok
        //    }
        //}
        IoResult::Ok
    }
}

/// A PCI Express routing ID
#[derive(Copy, Clone, Debug, Eq, Inspect, Ord, PartialEq, PartialOrd, Protobuf)]
#[inspect(display)]
#[allow(missing_docs)] // self explanatory constants
pub struct Rid {
    pub segment: u16,
    pub bus: u8,
    /// Combined device and function numbers.
    pub device_function: u8,
}

impl Rid {
    /// Construct a RID from components.
    pub fn new(segment: u16, bus: u8, device_function: u8) -> Self {
        Self {
            segment,
            bus,
            device_function,
        }
    }

    /// Retrieve the device number of the RID.
    pub fn device(&self) -> u8 {
        return self.device_function >> 3;
    }

    /// Retrieve the function number of the RID.
    pub fn function(&self) -> u8 {
        return self.device_function & 0b111;
    }
}

impl std::fmt::Display for Rid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Use standard-ish SBDF notation (ssss:bb:dd.f).
        write!(
            f,
            "{:04x}:{:02x}:{:02x}.{:x}",
            self.segment, self.bus, self.device(), self.function()
        )
    }
}

impl FromStr for Rid {
    type Err = anyhow::Error;

    fn from_str(sbdf_str: &str) -> Result<Self, Self::Err> {
        let (s, rest_bdf) = sbdf_str.split_once(':').context("expected segment")?;
        let segment = u16::from_str_radix(s, 16).context("failed to parse segment number")?;

        let (b, rest_df) = rest_bdf.split_once(':').context("expected bus")?;
        let bus = u8::from_str_radix(b, 16).context("failed to parse bus number")?;

        let (d, f) = rest_df.split_once('.').context("expected device")?;
        let device = u8::from_str_radix(d, 16).context("failed to parse device number")?;
        if device >= 32 {
            anyhow::bail!("invalid devce number: '{device}'");
        }

        let function = u8::from_str_radix(f, 16).context("failed to parse function number")?;
        if function >= 8 {
            anyhow::bail!("invalid function number: '{function}'");
        }

        Ok(Self::new(segment, bus, (device << 3) | function))
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rid_parsing() {
        assert_eq!(
            Rid::from_str("0000:00:0.0").unwrap(),
            Rid { segment: 0, bus: 0, device_function: 0 }
        );

        assert_eq!(
            Rid::from_str("0004:01:0.0").unwrap(),
            Rid { segment: 4, bus: 1, device_function: 0 }
        );
        assert_eq!(
            Rid::from_str("0000:1:0.1").unwrap(),
            Rid { segment: 0, bus: 1, device_function: 1 }
        );
        assert_eq!(
            Rid::from_str("0020:0f:3.1").unwrap(),
            Rid { segment: 32, bus: 15, device_function: 25 } // 25 = 3 << 3 | 1
        );

        // Test error cases
        assert!(Rid::from_str("invalid").is_err());
        assert!(Rid::from_str("0:0.0").is_err()); // missing segment or bus
        assert!(Rid::from_str("0:0:0").is_err()); // missing function
        assert!(Rid::from_str("0000:00:0.9").is_err()); // function out of range
    }
}
