// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::BusIdPcie;
use crate::chipset::PcieConflict;
use crate::chipset::PcieConflictReason;
use chipset_device::ChipsetDevice;
use closeable_mutex::CloseableMutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Weak;

/// An abstraction over a PCIe root complex implementation that is able to
/// route accesses to `Weak<CloseableMutex<dyn ChipsetDevice>>` devices.
pub trait RegisterWeakMutexPcie: Send {
    /// Try to add a PCI device to the bus, reporting any conflicts.
    fn add_pcie_function(
        &mut self,
        rid: pcie::Rid,
        name: Arc<str>,
        dev: Weak<CloseableMutex<dyn ChipsetDevice>>,
    ) -> Result<(), PcieConflict>;
}

pub struct WeakMutexPcieEntry {
    pub rid: pcie::Rid,
    pub name: Arc<str>,
    pub dev: Weak<CloseableMutex<dyn ChipsetDevice>>,
}

#[derive(Default)]
pub struct BusResolverWeakMutexPcie {
    pub root_complexes: HashMap<BusIdPcie, Box<dyn RegisterWeakMutexPcie>>,
    pub functions: HashMap<BusIdPcie, Vec<WeakMutexPcieEntry>>,
}

impl BusResolverWeakMutexPcie {
    pub fn resolve(mut self) -> Result<(), Vec<PcieConflict>> {
        let mut errs = Vec::new();

        for (bus_id, entries) in self.functions {
            for WeakMutexPcieEntry { rid, name, dev } in entries {
                let root_complex = match self.root_complexes.get_mut(&bus_id) {
                    Some(bus) => bus,
                    None => {
                        errs.push(PcieConflict {
                            rid,
                            conflict_dev: name.clone(),
                            reason: PcieConflictReason::MissingBus,
                        });
                        continue;
                    }
                };

                match root_complex.add_pcie_function(rid, name, dev) {
                    Ok(()) => {}
                    Err(conflict) => {
                        errs.push(conflict);
                        continue;
                    }
                };
            }
        }

        if !errs.is_empty() { Err(errs) } else { Ok(()) }
    }
}
