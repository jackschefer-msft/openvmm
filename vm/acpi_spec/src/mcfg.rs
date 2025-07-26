// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use super::Table;
use crate::packed_nums::*;
use core::mem::size_of;
use static_assertions::const_assert_eq;
use zerocopy::FromBytes;
use zerocopy::Immutable;
use zerocopy::IntoBytes;
use zerocopy::KnownLayout;
use zerocopy::Ref;
use zerocopy::Unaligned;

#[repr(C)]
#[derive(Copy, Clone, Debug, IntoBytes, Immutable, KnownLayout, FromBytes, Unaligned)]
pub struct McfgHeader {
    pub rsvd: u64_ne,
}

const_assert_eq!(size_of::<McfgHeader>(), 8);

impl McfgHeader {
    pub fn new() -> McfgHeader {
        McfgHeader {
            rsvd: 0.into(),
        }
    }
}

impl Table for McfgHeader {
    const SIGNATURE: [u8; 4] = *b"MCFG";
}

pub const MCFG_REVISION: u8 = 1;

#[repr(C)]
#[derive(Copy, Clone, Debug, IntoBytes, Immutable, KnownLayout, FromBytes, Unaligned)]
pub struct McfgSegmentBusRange {
    pub ecam_base: u64_ne,
    pub segment: u16_ne,
    pub start_bus: u8,
    pub end_bus: u8,
    pub rsvd: u32_ne,
}

const_assert_eq!(size_of::<McfgSegmentBusRange>(), 16);

impl McfgSegmentBusRange {
    pub fn new(ecam_base: u64, segment: u16, start_bus: u8, end_bus: u8) -> Self {
        Self {
            ecam_base: ecam_base.into(),
            segment: segment.into(),
            start_bus,
            end_bus,
            rsvd: 0.into(),
        }
    }
}

#[derive(Debug)]
pub enum ParseMcfgError {
    MissingAcpiHeader,
    InvalidSignature([u8; 4]),
    MismatchedLength { in_header: usize, actual: usize },
    MissingFixedHeader,
    BadSegmentBusRange,
}

impl core::fmt::Display for ParseMcfgError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingAcpiHeader => write!(f, "could not read standard ACPI header"),
            Self::InvalidSignature(sig) => {
                write!(f, "invalid signature. expected b\"MCFG\", found {sig:?}")
            }
            Self::MismatchedLength { in_header, actual } => {
                write!(f, "mismatched len. in_header: {in_header}, actual {actual}")
            }
            Self::MissingFixedHeader => write!(f, "missing fixed MCFG header"),
            Self::BadSegmentBusRange => write!(f, "could not read segment bus range structure"),
        }
    }
}

impl core::error::Error for ParseMcfgError {}

pub fn parse_mcfg<'a>(
    raw_mcfg: &'a [u8],
    mut on_segment_bus_range: impl FnMut(&'a McfgSegmentBusRange),
) -> Result<(&'a crate::Header, &'a McfgHeader), ParseMcfgError> {
    let raw_mcfg_len = raw_mcfg.len();
    let (acpi_header, buf) = Ref::<_, crate::Header>::from_prefix(raw_mcfg)
        .map_err(|_| ParseMcfgError::MissingAcpiHeader)?; // TODO: zerocopy: map_err (https://github.com/microsoft/openvmm/issues/759)

    if acpi_header.signature != *b"MCFG" {
        return Err(ParseMcfgError::InvalidSignature(acpi_header.signature));
    }

    if acpi_header.length.get() as usize != raw_mcfg_len {
        return Err(ParseMcfgError::MismatchedLength {
            in_header: acpi_header.length.get() as usize,
            actual: raw_mcfg_len,
        });
    }

    let (mcfg_header, mut buf) =
        Ref::<_, McfgHeader>::from_prefix(buf).map_err(|_| ParseMcfgError::MissingFixedHeader)?; // TODO: zerocopy: map_err (https://github.com/microsoft/openvmm/issues/759)

    while !buf.is_empty() {
        let (sbr, rest) =
            Ref::<_, McfgSegmentBusRange>::from_prefix(buf).map_err(|_| ParseMcfgError::BadSegmentBusRange)?; // TODO: zerocopy: map_err (https://github.com/microsoft/openvmm/issues/759)
        on_segment_bus_range(Ref::into_ref(sbr));
        buf = rest
    }

    Ok((Ref::into_ref(acpi_header), Ref::into_ref(mcfg_header)))
}
