// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! PCI Express topology types.

/// A description of a PCI Express Root Complex, as visible to the CPU.
pub struct PcieHostBridge {
    /// A unique integer index of this host bridge in the VM.
    pub index: u32,
    /// PCIe segment number.
    pub segment: u16,
    /// Lowest valid bus number.
    pub start_bus: u8,
    /// Highest valid bus number.
    pub end_bus: u8,
    /// Base address of the MMIO range used to
    /// access configuration space.
    pub ecam_base: u64,
    /// Base address of MMIO region below 4GB.
    pub low_mmio_base: u32,
    /// Size of MMIO region below 4GB.
    pub low_mmio_size: u32,
    /// Base address of MMIO region above 4GB.
    pub high_mmio_base: u64,
    /// Size of MMIO region above 4GB.
    pub high_mmio_size: u64,
}
