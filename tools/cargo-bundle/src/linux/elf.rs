/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Reads the machine and the needed shared libraries of an ELF executable.

const PT_LOAD: u64 = 1;
const PT_DYNAMIC: u64 = 2;
const DT_NEEDED: u64 = 1;
const DT_STRTAB: u64 = 5;

pub(crate) const EM_386: u16 = 3;
pub(crate) const EM_ARM: u16 = 40;
pub(crate) const EM_X86_64: u16 = 62;
pub(crate) const EM_AARCH64: u16 = 183;
pub(crate) const EM_RISCV: u16 = 243;

pub(crate) struct Elf {
    pub machine: u16,
    pub is_64bit: bool,
    /// The `DT_NEEDED` sonames, empty for static executables
    pub needed: Vec<String>,
}

struct Reader<'a> {
    data: &'a [u8],
    is_64bit: bool,
    big_endian: bool,
}

impl Reader<'_> {
    fn uint(&self, offset: u64, size: usize) -> Option<u64> {
        let start = usize::try_from(offset).ok()?;
        let bytes = self.data.get(start..start.checked_add(size)?)?;
        let fold = |value, byte: &u8| (value << 8) | u64::from(*byte);
        Some(if self.big_endian {
            bytes.iter().fold(0, fold)
        } else {
            bytes.iter().rev().fold(0, fold)
        })
    }

    fn word(&self, offset: u64) -> Option<u64> {
        self.uint(offset, if self.is_64bit { 8 } else { 4 })
    }

    fn string(&self, offset: u64) -> Option<String> {
        let bytes = self.data.get(usize::try_from(offset).ok()?..)?;
        let end = bytes.iter().position(|&byte| byte == 0)?;
        String::from_utf8(bytes[..end].to_vec()).ok()
    }
}

/// Read the machine and word size from an ELF header.
pub(crate) fn identify(data: &[u8]) -> Option<(u16, bool)> {
    let reader = reader(data)?;
    Some((reader.uint(18, 2)? as u16, reader.is_64bit))
}

fn reader(data: &[u8]) -> Option<Reader<'_>> {
    if data.get(..4)? != b"\x7fELF" {
        return None;
    }
    Some(Reader {
        data,
        is_64bit: *data.get(4)? == 2,
        big_endian: *data.get(5)? == 2,
    })
}

pub(crate) fn parse(data: &[u8]) -> Option<Elf> {
    let elf = reader(data)?;
    let (machine, is_64bit) = identify(data)?;
    let (phoff, phentsize, phnum) = if is_64bit {
        (elf.uint(32, 8)?, elf.uint(54, 2)?, elf.uint(56, 2)?)
    } else {
        (elf.uint(28, 4)?, elf.uint(42, 2)?, elf.uint(44, 2)?)
    };

    // Program headers as (type, file offset, virtual address, file size)
    let segments = (0..phnum)
        .map(|index| {
            let header = phoff + index * phentsize;
            Some(if is_64bit {
                (
                    elf.uint(header, 4)?,
                    elf.uint(header + 8, 8)?,
                    elf.uint(header + 16, 8)?,
                    elf.uint(header + 32, 8)?,
                )
            } else {
                (
                    elf.uint(header, 4)?,
                    elf.uint(header + 4, 4)?,
                    elf.uint(header + 8, 4)?,
                    elf.uint(header + 16, 4)?,
                )
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let Some(&(_, dynamic, _, dynamic_size)) =
        segments.iter().find(|segment| segment.0 == PT_DYNAMIC)
    else {
        return Some(Elf {
            machine,
            is_64bit,
            needed: Vec::new(),
        });
    };

    let entry_size = if is_64bit { 16 } else { 8 };
    let mut strtab = None;
    let mut needed_offsets = Vec::new();
    for entry in (dynamic..dynamic + dynamic_size).step_by(entry_size) {
        let value = elf.word(entry + entry_size as u64 / 2)?;
        match elf.word(entry)? {
            0 => break,
            DT_NEEDED => needed_offsets.push(value),
            DT_STRTAB => strtab = Some(value),
            _ => {}
        }
    }

    // The string table is referenced by its virtual address
    let strtab = strtab?;
    let strtab = segments
        .iter()
        .find(|(kind, _, address, size)| {
            *kind == PT_LOAD && (*address..address + size).contains(&strtab)
        })
        .map(|(_, offset, address, _)| offset + strtab - address)?;
    let needed = needed_offsets
        .into_iter()
        .map(|offset| elf.string(strtab + offset))
        .collect::<Option<_>>()?;
    Some(Elf {
        machine,
        is_64bit,
        needed,
    })
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal 64-bit little-endian executable needing libc.so.6 and libm.so.6.
    fn fixture() -> Vec<u8> {
        let mut data = vec![0u8; 0x200];
        data[..6].copy_from_slice(b"\x7fELF\x02\x01");
        data[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());
        data[32..40].copy_from_slice(&64u64.to_le_bytes());
        data[54..56].copy_from_slice(&56u16.to_le_bytes());
        data[56..58].copy_from_slice(&2u16.to_le_bytes());
        // PT_LOAD maps file offset 0 to address 0x1000
        let load = 64;
        data[load..load + 4].copy_from_slice(&1u32.to_le_bytes());
        data[load + 16..load + 24].copy_from_slice(&0x1000u64.to_le_bytes());
        data[load + 32..load + 40].copy_from_slice(&0x200u64.to_le_bytes());
        // PT_DYNAMIC at file offset 0x100 with four entries
        let dynamic = 120;
        data[dynamic..dynamic + 4].copy_from_slice(&2u32.to_le_bytes());
        data[dynamic + 8..dynamic + 16].copy_from_slice(&0x100u64.to_le_bytes());
        data[dynamic + 32..dynamic + 40].copy_from_slice(&64u64.to_le_bytes());
        for (index, (tag, value)) in [(1u64, 1u64), (5, 0x1180), (1, 11), (0, 0)]
            .into_iter()
            .enumerate()
        {
            let entry = 0x100 + index * 16;
            data[entry..entry + 8].copy_from_slice(&tag.to_le_bytes());
            data[entry + 8..entry + 16].copy_from_slice(&value.to_le_bytes());
        }
        data[0x180..0x195].copy_from_slice(b"\0libc.so.6\0libm.so.6\0");
        data
    }

    #[test]
    fn reads_needed_libraries() {
        let elf = parse(&fixture()).expect("valid ELF");
        assert_eq!(elf.machine, EM_X86_64);
        assert!(elf.is_64bit);
        assert_eq!(elf.needed, ["libc.so.6", "libm.so.6"]);
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse(b"#!/bin/sh\n").is_none());
        assert!(identify(&fixture()[..10]).is_none());
        assert_eq!(identify(&fixture()), Some((EM_X86_64, true)));
    }
}
