//! Executable file reader (PE/ELF/Mach-O).
//!
//! Extracts basic header info from executable files.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_exe(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 4 {
        return Err(Error::InvalidData("file too small".into()));
    }

    // PE (MZ header)
    if data.starts_with(b"MZ") {
        return read_pe(data);
    }
    // ELF
    if data.starts_with(&[0x7F, b'E', b'L', b'F']) {
        return read_elf(data);
    }
    // Mach-O (32-bit)
    if data.starts_with(&[0xFE, 0xED, 0xFA, 0xCE]) || data.starts_with(&[0xCE, 0xFA, 0xED, 0xFE]) {
        return read_macho(data, false);
    }
    // Mach-O (64-bit)
    if data.starts_with(&[0xFE, 0xED, 0xFA, 0xCF]) || data.starts_with(&[0xCF, 0xFA, 0xED, 0xFE]) {
        return read_macho(data, true);
    }
    // Mach-O Universal/Fat
    if data.starts_with(&[0xCA, 0xFE, 0xBA, 0xBE]) {
        let mut tags = Vec::new();
        tags.push(mk("ExeType", "Executable Type", Value::String("Mach-O Universal Binary".into())));
        if data.len() >= 8 {
            let num_arch = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
            tags.push(mk("NumArchitectures", "Architectures", Value::U32(num_arch)));
        }
        return Ok(tags);
    }
    // AR archive (.a files)
    if data.starts_with(b"!<arch>\n") {
        let mut tags = Vec::new();
        tags.push(mk("ExeType", "Executable Type", Value::String("AR Archive (static library)".into())));
        return Ok(tags);
    }

    Err(Error::InvalidData("unknown executable format".into()))
}

fn read_pe(data: &[u8]) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();
    tags.push(mk("ExeType", "Executable Type", Value::String("Windows PE".into())));

    if data.len() < 64 {
        return Ok(tags);
    }

    // PE header offset at 0x3C
    let pe_offset = u32::from_le_bytes([data[0x3C], data[0x3D], data[0x3E], data[0x3F]]) as usize;
    if pe_offset + 24 > data.len() || &data[pe_offset..pe_offset + 4] != b"PE\0\0" {
        return Ok(tags);
    }

    let coff = &data[pe_offset + 4..];
    if coff.len() < 20 {
        return Ok(tags);
    }

    let machine = u16::from_le_bytes([coff[0], coff[1]]);
    let machine_str = match machine {
        0x014C => "Intel 386",
        0x0200 => "Intel Itanium",
        0x8664 => "AMD64",
        0xAA64 => "ARM64",
        0x01C0 => "ARM",
        _ => "Unknown",
    };
    tags.push(mk("MachineType", "Machine Type", Value::String(machine_str.into())));

    let num_sections = u16::from_le_bytes([coff[2], coff[3]]);
    tags.push(mk("NumSections", "Number of Sections", Value::U16(num_sections)));

    // TimeDateStamp
    let timestamp = u32::from_le_bytes([coff[4], coff[5], coff[6], coff[7]]);
    if timestamp > 0 {
        tags.push(mk("TimeStamp", "Time Stamp", Value::U32(timestamp)));
    }

    // Optional header magic
    if coff.len() >= 22 {
        let opt_magic = u16::from_le_bytes([coff[20], coff[21]]);
        let subsystem = match opt_magic {
            0x10B => "PE32",
            0x20B => "PE32+",
            _ => "Unknown",
        };
        tags.push(mk("PEType", "PE Type", Value::String(subsystem.into())));
    }

    Ok(tags)
}

fn read_elf(data: &[u8]) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();

    if data.len() < 20 {
        return Ok(tags);
    }

    let class = match data[4] {
        1 => "32-bit",
        2 => "64-bit",
        _ => "Unknown",
    };
    let endian = match data[5] {
        1 => "Little-endian",
        2 => "Big-endian",
        _ => "Unknown",
    };
    let os_abi = match data[7] {
        0 => "UNIX System V",
        3 => "Linux",
        6 => "Solaris",
        9 => "FreeBSD",
        _ => "Other",
    };

    tags.push(mk("ExeType", "Executable Type", Value::String(format!("ELF {}", class))));
    tags.push(mk("ByteOrder", "Byte Order", Value::String(endian.into())));
    tags.push(mk("OSABI", "OS/ABI", Value::String(os_abi.into())));

    let is_le = data[5] == 1;
    let elf_type = if is_le {
        u16::from_le_bytes([data[16], data[17]])
    } else {
        u16::from_be_bytes([data[16], data[17]])
    };
    let type_str = match elf_type {
        1 => "Relocatable",
        2 => "Executable",
        3 => "Shared Object",
        4 => "Core Dump",
        _ => "Unknown",
    };
    tags.push(mk("ObjectFileType", "Object File Type", Value::String(type_str.into())));

    let machine = if is_le {
        u16::from_le_bytes([data[18], data[19]])
    } else {
        u16::from_be_bytes([data[18], data[19]])
    };
    let machine_str = match machine {
        3 => "Intel 386",
        8 => "MIPS",
        20 => "PowerPC",
        40 => "ARM",
        62 => "AMD64",
        183 => "ARM64",
        _ => "Unknown",
    };
    tags.push(mk("CPUArchitecture", "CPU Architecture", Value::String(machine_str.into())));

    Ok(tags)
}

fn read_macho(data: &[u8], is_64: bool) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();
    let is_le = data[0] == 0xCE || data[0] == 0xCF;

    tags.push(mk("ExeType", "Executable Type", Value::String(
        format!("Mach-O {}", if is_64 { "64-bit" } else { "32-bit" })
    )));

    if data.len() < 16 {
        return Ok(tags);
    }

    let cpu_type = if is_le {
        u32::from_le_bytes([data[4], data[5], data[6], data[7]])
    } else {
        u32::from_be_bytes([data[4], data[5], data[6], data[7]])
    };

    let cpu_str = match cpu_type & 0xFF {
        7 => "x86",
        12 => "ARM",
        18 => "PowerPC",
        _ => "Unknown",
    };
    let is_64_cpu = cpu_type & 0x01000000 != 0;
    tags.push(mk("CPUType", "CPU Type", Value::String(
        format!("{}{}", cpu_str, if is_64_cpu { " (64-bit)" } else { "" })
    )));

    let file_type = if is_le {
        u32::from_le_bytes([data[12], data[13], data[14], data[15]])
    } else {
        u32::from_be_bytes([data[12], data[13], data[14], data[15]])
    };
    let type_str = match file_type {
        1 => "Object",
        2 => "Executable",
        3 => "Fixed VM Library",
        4 => "Core Dump",
        5 => "Preloaded Executable",
        6 => "Dynamic Library",
        7 => "Dynamic Linker",
        8 => "Bundle",
        _ => "Unknown",
    };
    tags.push(mk("ObjectFileType", "Object File Type", Value::String(type_str.into())));

    Ok(tags)
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup { family0: "EXE".into(), family1: "EXE".into(), family2: "Other".into() },
        raw_value: value, print_value: pv, priority: 0,
    }
}
