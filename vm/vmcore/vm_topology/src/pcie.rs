// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! PCI Express topology types.

/// A description of the VM's PCIe topology as visible to
/// the CPU.
pub struct PcieTopology(Vec<PcieRootComplexTopology>);

impl PcieTopology {
    /// Construct an empty topology object.
    pub fn new() -> Self {
        Self {
            0: Vec::new(),
        }
    }

    /// Add a root complex to the topology.
    pub fn add_root_complex(&mut self, segment: u16, start_bus: u8, end_bus: u8, ecam_base: u64) {
        self.0.push(PcieRootComplexTopology {
            segment,
            start_bus,
            end_bus,
            ecam_base,
        })
    }

    /// Returns whether any PCIe topology exists.
    pub fn empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns an iterator through the root complex descriptions.
    pub fn iter(&self) -> impl Iterator<Item = &PcieRootComplexTopology> {
        self.0.iter()
    }
}

/// The topology description for a single PCIe root
/// complex.
pub struct PcieRootComplexTopology {
    /// PCIe segment number
    pub segment: u16,
    /// Lowest valid bus number
    pub start_bus: u8,
    /// Highest valid bus number
    pub end_bus: u8,
    /// Base address of the MMIO range the guest
    /// can use to access configuration space.
    pub ecam_base: u64,
    // TODO: MMIO windows
}
