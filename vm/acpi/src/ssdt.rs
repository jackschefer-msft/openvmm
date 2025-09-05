// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

pub use crate::sdt::*;
use memory_range::MemoryRange;
use zerocopy::FromBytes;
use zerocopy::Immutable;
use zerocopy::IntoBytes;
use zerocopy::KnownLayout;

#[repr(C, packed)]
#[derive(Copy, Clone, Debug, IntoBytes, Immutable, KnownLayout, FromBytes)]
pub struct DescriptionHeader {
    pub signature: u32,
    _length: u32,
    pub revision: u8,
    _checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: u64,
    pub oem_revision: u32,
    pub creator_id: u32,
    pub creator_rev: u32,
}

fn encode_pcie_name(mut pcie_index: u32) -> Vec<u8> {
    assert!(pcie_index < 1000);
    let mut temp = format!("PCI0").as_bytes().to_vec();
    let mut i = temp.len() - 1;
    while pcie_index > 0 {
        temp[i] = b'0' + (pcie_index % 10) as u8;
        pcie_index /= 10;
        i -= 1;
    }
    temp
}

pub struct Ssdt {
    description_header: DescriptionHeader,
    objects: Vec<u8>,
}

impl Ssdt {
    pub fn new() -> Self {
        Self {
            description_header: DescriptionHeader {
                signature: u32::from_le_bytes(*b"SSDT"),
                _length: 0,
                revision: 2,
                _checksum: 0,
                oem_id: *b"MSFTVM",
                oem_table_id: 0x313054445353, // b'SSDT01'
                oem_revision: 1,
                creator_id: u32::from_le_bytes(*b"MSFT"),
                creator_rev: 0x01000000,
            },
            objects: vec![],
            //ecam_ranges: vec![],
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut byte_stream = Vec::new();
        byte_stream.extend_from_slice(self.description_header.as_bytes());
        byte_stream.extend_from_slice(&self.objects);

        let length = byte_stream.len();
        byte_stream[4..8].copy_from_slice(&u32::try_from(length).unwrap().to_le_bytes());
        let mut checksum: u8 = 0;
        for byte in &byte_stream {
            checksum = checksum.wrapping_add(*byte);
        }

        byte_stream[9] = (!checksum).wrapping_add(1);
        byte_stream
    }

    pub fn add_object(&mut self, obj: &impl SdtObject) {
        obj.append_to_vec(&mut self.objects);
    }

    /// Adds a PCI Express root complex with the specified bus number and MMIO ranges.
    ///
    /// ```text
    /// Device(\_SB.PCI<N>)
    /// {
    ///     Name(_HID, PNP0A08)
    ///     Name(_UID, <index>)
    ///     Name(_SEG, <segment>)
    ///     Name(_BBN, <bus number>)
    ///     Name(_CRS, ResourceTemplate()
    ///     {
    ///         WordBusNumber(...) // Bus translation info
    ///         QWordMemory() // Low MMIO
    ///         QWordMemory() // High MMIO
    ///     })
    /// }
    /// ```
    pub fn add_pcie(
        &mut self,
        index: u32,
        segment: u16,
        start_bus: u8,
        end_bus: u8,
        low_mmio: MemoryRange,
        high_mmio: MemoryRange,
    ) {
        let mut pcie = Device::new(encode_pcie_name(index).as_slice());
        pcie.add_object(&NamedObject::new(b"_HID", &EisaId(*b"PNP0A08")));
        pcie.add_object(&NamedInteger::new(b"_UID", index.into()));
        pcie.add_object(&NamedInteger::new(b"_SEG", segment.into()));
        pcie.add_object(&NamedInteger::new(b"_BBN", start_bus.into()));

        // TODO: Lots of work needed for _OSC.

        let mut crs = CurrentResourceSettings::new();
        crs.add_resource(&BusNumber::new(start_bus.into(), (end_bus as u16) - (start_bus as u16) + 1));
        crs.add_resource(&QwordMemory::new(low_mmio.start(), low_mmio.end() - low_mmio.start()));
        crs.add_resource(&QwordMemory::new(high_mmio.start(), high_mmio.end() - high_mmio.start()));
        pcie.add_object(&crs);

        self.add_object(&pcie);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdt::test_helpers::verify_expected_bytes;

    pub fn verify_header(bytes: &[u8]) {
        assert!(bytes.len() >= 36);

        // signature
        assert_eq!(bytes[0], b'S');
        assert_eq!(bytes[1], b'S');
        assert_eq!(bytes[2], b'D');
        assert_eq!(bytes[3], b'T');

        // length
        let ssdt_len = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        assert_eq!(ssdt_len as usize, bytes.len());

        // revision
        assert_eq!(bytes[8], 2);

        // Validate checksum bytes[9] by verifying content adds to zero.
        let mut checksum: u8 = 0;
        for byte in bytes.iter() {
            checksum = checksum.wrapping_add(*byte);
        }
        assert_eq!(checksum, 0);

        // oem_id
        assert_eq!(bytes[10], b'M');
        assert_eq!(bytes[11], b'S');
        assert_eq!(bytes[12], b'F');
        assert_eq!(bytes[13], b'T');
        assert_eq!(bytes[14], b'V');
        assert_eq!(bytes[15], b'M');

        // oem_table_id
        assert_eq!(bytes[16], b'S');
        assert_eq!(bytes[17], b'S');
        assert_eq!(bytes[18], b'D');
        assert_eq!(bytes[19], b'T');
        assert_eq!(bytes[20], b'0');
        assert_eq!(bytes[21], b'1');
        assert_eq!(bytes[22], 0);
        assert_eq!(bytes[23], 0);

        // oem_revision
        let oem_revision = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        assert_eq!(oem_revision, 1);

        // creator_id
        assert_eq!(bytes[28], b'M');
        assert_eq!(bytes[29], b'S');
        assert_eq!(bytes[30], b'F');
        assert_eq!(bytes[31], b'T');

        // creator_rev
        let creator_rev = u32::from_le_bytes(bytes[32..36].try_into().unwrap());
        assert_eq!(creator_rev, 0x01000000);
    }

    #[test]
    pub fn verify_pcie_name_encoding() {
        assert_eq!(encode_pcie_name(0), b"PCI0".to_vec());
        assert_eq!(encode_pcie_name(1), b"PCI1".to_vec());
        assert_eq!(encode_pcie_name(2), b"PCI2".to_vec());
        assert_eq!(encode_pcie_name(54), b"PC54".to_vec());
        assert_eq!(encode_pcie_name(294), b"P294".to_vec());
    }

    #[test]
    fn verify_simple_table() {
        let mut dsdt = Ssdt::new();
        let nobj = NamedObject::new(b"_S0", &Package(vec![0, 0]));
        dsdt.add_object(&nobj);
        let bytes = dsdt.to_bytes();
        verify_header(&bytes);
        verify_expected_bytes(&bytes[36..], &[8, b'_', b'S', b'0', b'_', 0x12, 4, 2, 0, 0]);
    }

    //#[test]
    //fn verify_table() {
    //    let mut dsdt = Dsdt::new();
    //    dsdt.add_object(&NamedObject::new(b"\\_S0", &Package(vec![0, 0])));
    //    dsdt.add_object(&NamedObject::new(b"\\_S5", &Package(vec![0, 0])));

    //    let mut apic = Device::new(b"\\_SB.APIC");
    //    apic.add_object(&NamedObject::new(b"_HID", &EisaId(*b"PNP0003")));
    //    let mut apic_crs = CurrentResourceSettings::new();
    //    apic_crs.add_resource(&Memory32Fixed::new(0xfee00000, 0x1000, true));
    //    apic_crs.add_resource(&Memory32Fixed::new(0xfec00000, 0x1000, true));
    //    apic.add_object(&apic_crs);
    //    dsdt.add_object(&apic);

    //    let mut uart = Device::new(b"\\_SB.UAR1");
    //    uart.add_object(&NamedObject::new(b"_HID", &EisaId(*b"PNP0501")));
    //    uart.add_object(&NamedString::new(b"_DDN", b"COM1"));
    //    uart.add_object(&NamedInteger::new(b"_UID", 1));
    //    let mut uart_crs = CurrentResourceSettings::new();
    //    uart_crs.add_resource(&IoPort::new(0x3f8, 0x3f8, 8));
    //    let mut intr = Interrupt::new(4);
    //    intr.is_edge_triggered = true;
    //    uart_crs.add_resource(&intr);
    //    uart.add_object(&uart_crs);
    //    dsdt.add_object(&uart);

    //    let mut vmod = Device::new(b"\\_SB.VMOD");
    //    vmod.add_object(&NamedString::new(b"_HID", b"ACPI0004"));
    //    vmod.add_object(&NamedInteger::new(b"_UID", 0));
    //    let mut vmod_crs = CurrentResourceSettings::new();
    //    vmod_crs.add_resource(&QwordMemory::new(0x100000000, 0x100000000));
    //    vmod.add_object(&vmod_crs);
    //    dsdt.add_object(&vmod);

    //    let mut vmbs = Device::new(b"\\_SB.VMOD.VMBS");
    //    vmbs.add_object(&NamedInteger::new(b"STA", 0xf));
    //    vmbs.add_object(&NamedInteger::new(b"_ADR", 0));
    //    vmbs.add_object(&NamedString::new(b"_DDN", b"VMBUS"));
    //    vmbs.add_object(&NamedString::new(b"_HID", b"VMBus"));
    //    vmbs.add_object(&NamedInteger::new(b"_UID", 0));
    //    let op = AndOp {
    //        operand1: vec![b'S', b'T', b'A', b'_'],
    //        operand2: encode_integer(13),
    //        target_name: vec![b'S', b'T', b'A', b'_'],
    //    };
    //    let mut method = Method::new(b"_DIS");
    //    method.add_operation(&op);
    //    vmbs.add_object(&method);
    //    let op = OrOp {
    //        operand1: vec![b'S', b'T', b'A', b'_'],
    //        operand2: encode_integer(15),
    //        target_name: vec![b'S', b'T', b'A', b'_'],
    //    };
    //    let mut method = Method::new(b"_PS0");
    //    method.add_operation(&op);
    //    vmbs.add_object(&method);
    //    let op = ReturnOp {
    //        result: vec![b'S', b'T', b'A', b'_'],
    //    };
    //    let mut method = Method::new(b"_STA");
    //    method.add_operation(&op);
    //    vmbs.add_object(&method);
    //    vmbs.add_object(&NamedInteger::new(b"_PS3", 0));
    //    dsdt.add_object(&vmbs);

    //    let mut rtc = Device::new(b"\\_SB.RTC0");
    //    rtc.add_object(&NamedObject::new(b"_HID", &EisaId(*b"PNP0B00")));
    //    rtc.add_object(&NamedInteger::new(b"_UID", 0));
    //    let mut rtc_crs = CurrentResourceSettings::new();
    //    let mut ioport = IoPort::new(0x70, 0x70, 2);
    //    ioport.alignment = 0;
    //    rtc_crs.add_resource(&ioport);
    //    let mut intr = Interrupt::new(8);
    //    intr.is_edge_triggered = true;
    //    rtc_crs.add_resource(&intr);
    //    rtc.add_object(&rtc_crs);
    //    dsdt.add_object(&rtc);

    //    for proc_index in 1..3 {
    //        let mut proc = Device::new(format!("P{:03}", proc_index).as_bytes());
    //        proc.add_object(&NamedString::new(b"_HID", b"ACPI0007"));
    //        proc.add_object(&NamedInteger::new(b"_UID", proc_index as u64));
    //        let mut method = Method::new(b"_STA");
    //        method.add_operation(&ReturnOp {
    //            result: encode_integer(0xf),
    //        });
    //        proc.add_object(&method);
    //        dsdt.add_object(&proc);
    //    }

    //    let bytes = dsdt.to_bytes();
    //    verify_header(&bytes);
    //    verify_expected_bytes(
    //        &bytes[36..],
    //        &[
    //            0x08, 0x5C, 0x5F, 0x53, 0x30, 0x5F, 0x12, 0x04, 0x02, 0x00, 0x00, 0x08, 0x5C, 0x5F,
    //            0x53, 0x35, 0x5F, 0x12, 0x04, 0x02, 0x00, 0x00, 0x5B, 0x82, 0x38, 0x5C, 0x2E, 0x5F,
    //            0x53, 0x42, 0x5F, 0x41, 0x50, 0x49, 0x43, 0x08, 0x5F, 0x48, 0x49, 0x44, 0x0C, 0x41,
    //            0xD0, 0x00, 0x03, 0x08, 0x5F, 0x43, 0x52, 0x53, 0x11, 0x1D, 0x0A, 0x1A, 0x86, 0x09,
    //            0x00, 0x01, 0x00, 0x00, 0xE0, 0xFE, 0x00, 0x10, 0x00, 0x00, 0x86, 0x09, 0x00, 0x01,
    //            0x00, 0x00, 0xC0, 0xFE, 0x00, 0x10, 0x00, 0x00, 0x79, 0x00, 0x5B, 0x82, 0x43, 0x04,
    //            0x5C, 0x2E, 0x5F, 0x53, 0x42, 0x5F, 0x55, 0x41, 0x52, 0x31, 0x08, 0x5F, 0x48, 0x49,
    //            0x44, 0x0C, 0x41, 0xD0, 0x05, 0x01, 0x08, 0x5F, 0x44, 0x44, 0x4E, 0x0D, 0x43, 0x4F,
    //            0x4D, 0x31, 0x00, 0x08, 0x5F, 0x55, 0x49, 0x44, 0x01, 0x08, 0x5F, 0x43, 0x52, 0x53,
    //            0x11, 0x16, 0x0A, 0x13, 0x47, 0x01, 0xF8, 0x03, 0xF8, 0x03, 0x01, 0x08, 0x89, 0x06,
    //            0x00, 0x03, 0x01, 0x04, 0x00, 0x00, 0x00, 0x79, 0x00, 0x5B, 0x82, 0x4A, 0x05, 0x5C,
    //            0x2E, 0x5F, 0x53, 0x42, 0x5F, 0x56, 0x4D, 0x4F, 0x44, 0x08, 0x5F, 0x48, 0x49, 0x44,
    //            0x0D, 0x41, 0x43, 0x50, 0x49, 0x30, 0x30, 0x30, 0x34, 0x00, 0x08, 0x5F, 0x55, 0x49,
    //            0x44, 0x00, 0x08, 0x5F, 0x43, 0x52, 0x53, 0x11, 0x33, 0x0A, 0x30, 0x8A, 0x2B, 0x00,
    //            0x00, 0x0C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    //            0x00, 0x01, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0x00, 0x00, 0x00, 0x00,
    //            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    //            0x00, 0x79, 0x00, 0x5B, 0x82, 0x42, 0x07, 0x5C, 0x2F, 0x03, 0x5F, 0x53, 0x42, 0x5F,
    //            0x56, 0x4D, 0x4F, 0x44, 0x56, 0x4D, 0x42, 0x53, 0x08, 0x53, 0x54, 0x41, 0x5F, 0x0A,
    //            0x0F, 0x08, 0x5F, 0x41, 0x44, 0x52, 0x00, 0x08, 0x5F, 0x44, 0x44, 0x4E, 0x0D, 0x56,
    //            0x4D, 0x42, 0x55, 0x53, 0x00, 0x08, 0x5F, 0x48, 0x49, 0x44, 0x0D, 0x56, 0x4D, 0x42,
    //            0x75, 0x73, 0x00, 0x08, 0x5F, 0x55, 0x49, 0x44, 0x00, 0x14, 0x11, 0x5F, 0x44, 0x49,
    //            0x53, 0x00, 0x7B, 0x53, 0x54, 0x41, 0x5F, 0x0A, 0x0D, 0x53, 0x54, 0x41, 0x5F, 0x14,
    //            0x11, 0x5F, 0x50, 0x53, 0x30, 0x00, 0x7D, 0x53, 0x54, 0x41, 0x5F, 0x0A, 0x0F, 0x53,
    //            0x54, 0x41, 0x5F, 0x14, 0x0B, 0x5F, 0x53, 0x54, 0x41, 0x00, 0xA4, 0x53, 0x54, 0x41,
    //            0x5F, 0x08, 0x5F, 0x50, 0x53, 0x33, 0x00, 0x5B, 0x82, 0x37, 0x5C, 0x2E, 0x5F, 0x53,
    //            0x42, 0x5F, 0x52, 0x54, 0x43, 0x30, 0x08, 0x5F, 0x48, 0x49, 0x44, 0x0C, 0x41, 0xD0,
    //            0x0B, 0x00, 0x08, 0x5F, 0x55, 0x49, 0x44, 0x00, 0x08, 0x5F, 0x43, 0x52, 0x53, 0x11,
    //            0x16, 0x0A, 0x13, 0x47, 0x01, 0x70, 0x00, 0x70, 0x00, 0x00, 0x02, 0x89, 0x06, 0x00,
    //            0x03, 0x01, 0x08, 0x00, 0x00, 0x00, 0x79, 0x00, 0x5B, 0x82, 0x24, 0x50, 0x30, 0x30,
    //            0x31, 0x08, 0x5F, 0x48, 0x49, 0x44, 0x0D, 0x41, 0x43, 0x50, 0x49, 0x30, 0x30, 0x30,
    //            0x37, 0x00, 0x08, 0x5F, 0x55, 0x49, 0x44, 0x01, 0x14, 0x09, 0x5F, 0x53, 0x54, 0x41,
    //            0x00, 0xA4, 0x0A, 0x0F, 0x5B, 0x82, 0x25, 0x50, 0x30, 0x30, 0x32, 0x08, 0x5F, 0x48,
    //            0x49, 0x44, 0x0D, 0x41, 0x43, 0x50, 0x49, 0x30, 0x30, 0x30, 0x37, 0x00, 0x08, 0x5F,
    //            0x55, 0x49, 0x44, 0x0A, 0x02, 0x14, 0x09, 0x5F, 0x53, 0x54, 0x41, 0x00, 0xA4, 0x0A,
    //            0x0F,
    //        ],
    //    );
    //}
}
