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

    tags.push(mk("CPUType", "CPU Type", Value::String(format!("ELF {}", class))));
    tags.push(mk("CPUByteOrder", "CPU Byte Order", Value::String(endian.into())));

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

    // Determine byte order and bit depth from magic number
    // \xFE\xED\xFA\xCE = 32-bit big endian
    // \xCE\xFA\xED\xFE = 32-bit little endian
    // \xFE\xED\xFA\xCF = 64-bit big endian
    // \xCF\xFA\xED\xFE = 64-bit little endian
    let is_le = data[0] == 0xCE || data[0] == 0xCF;
    let arch_str = if is_64 { "64 bit" } else { "32 bit" };
    let order_str = if is_le { "Little endian" } else { "Big endian" };

    tags.push(mk("CPUArchitecture", "CPU Architecture", Value::String(arch_str.into())));
    tags.push(mk("CPUByteOrder", "CPU Byte Order", Value::String(order_str.into())));

    if data.len() < 28 {
        return Ok(tags);
    }

    // Mach header layout:
    //  0: magic (4 bytes)
    //  4: cputype (int32s)
    //  8: cpusubtype (int32s)
    // 12: filetype (int32u)
    // 16: ncmds (int32u)
    // 20: sizeofcmds (int32u)
    // 24: flags (int32u)

    let read_u32 = |offset: usize| -> u32 {
        if is_le {
            u32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]])
        } else {
            u32::from_be_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]])
        }
    };
    let read_i32 = |offset: usize| -> i32 {
        read_u32(offset) as i32
    };

    let cpu_type = read_i32(4);
    let cpu_subtype = read_i32(8);
    let file_type = read_u32(12);
    let flags = read_u32(24);

    // CPUType: strip 64-bit flag (0x1000000) for lookup, add "64-bit" suffix
    let cpu_base = cpu_type & 0x00FFFFFF;
    let has_64_flag = (cpu_type as u32) & 0x01000000 != 0;
    let cpu_name = match cpu_base {
        0xFFFFFF => "Any",  // -1 & 0xFFFFFF
        1 => "VAX",
        2 => "ROMP",
        4 => "NS32032",
        5 => "NS32332",
        6 => "MC680x0",
        7 => "x86",
        8 => "MIPS",
        9 => "NS32532",
        10 => "MC98000",
        11 => "HPPA",
        12 => "ARM",
        13 => "MC88000",
        14 => "SPARC",
        15 => "i860 big endian",
        16 => "i860 little endian",
        17 => "RS6000",
        18 => "PowerPC",
        255 => "VEO",
        _ => "Unknown",
    };
    let cpu_type_str = if has_64_flag {
        format!("{} 64-bit", cpu_name)
    } else {
        cpu_name.to_string()
    };
    tags.push(mk("CPUType", "CPU Type", Value::String(cpu_type_str)));

    // CPUSubtype: lookup by "cputype subtype" key, adding "64-bit" suffix if high bit set
    let sub_base = cpu_subtype & 0x7FFFFFFF;
    let sub_has_64 = (cpu_subtype as u32) & 0x80000000 != 0;
    let lookup_key = format!("{} {}", cpu_base, sub_base);
    let subtype_name = macho_cpu_subtype(&lookup_key);
    let cpu_subtype_str = if sub_has_64 || (has_64_flag && !subtype_name.is_empty()) {
        format!("{} 64-bit", if subtype_name.is_empty() {
            format!("Unknown ({} {})", cpu_type, cpu_subtype)
        } else {
            subtype_name.to_string()
        })
    } else if subtype_name.is_empty() {
        format!("Unknown ({} {})", cpu_type, cpu_subtype)
    } else {
        subtype_name.to_string()
    };
    tags.push(mk("CPUSubtype", "CPU Subtype", Value::String(cpu_subtype_str)));

    // ObjectFileType
    let obj_type_str = match file_type {
        1 => "Relocatable object",
        2 => "Demand paged executable",
        3 => "Fixed VM shared library",
        4 => "Core",
        5 => "Preloaded executable",
        6 => "Dynamically bound shared library",
        7 => "Dynamic link editor",
        8 => "Dynamically bound bundle",
        9 => "Shared library stub for static linking",
        10 => "Debug information",
        11 => "x86_64 kexts",
        _ => "Unknown",
    };
    tags.push(mk("ObjectFileType", "Object File Type", Value::String(obj_type_str.into())));

    // ObjectFlags: bitmask decoding
    let flag_names = [
        (0, "No undefs"),
        (1, "Incrementa link"),
        (2, "Dyld link"),
        (3, "Bind at load"),
        (4, "Prebound"),
        (5, "Split segs"),
        (6, "Lazy init"),
        (7, "Two level"),
        (8, "Force flat"),
        (9, "No multi defs"),
        (10, "No fix prebinding"),
        (11, "Prebindable"),
        (12, "All mods bound"),
        (13, "Subsections via symbols"),
        (14, "Canonical"),
        (15, "Weak defines"),
        (16, "Binds to weak"),
        (17, "Allow stack execution"),
        (18, "Dead strippable dylib"),
        (19, "Root safe"),
        (20, "No reexported dylibs"),
        (21, "Random address"),
    ];
    let mut flag_parts: Vec<&str> = Vec::new();
    for (bit, name) in &flag_names {
        if flags & (1 << bit) != 0 {
            flag_parts.push(name);
        }
    }
    let flags_str = if flag_parts.is_empty() {
        format!("0x{:x}", flags)
    } else {
        flag_parts.join(", ")
    };
    tags.push(mk("ObjectFlags", "Object Flags", Value::String(flags_str)));

    Ok(tags)
}

/// Lookup Mach-O CPU subtype name by "cputype subtype" key.
fn macho_cpu_subtype(key: &str) -> &'static str {
    match key {
        "1 0" => "VAX (all)",
        "1 1" => "VAX780",
        "1 2" => "VAX785",
        "1 3" => "VAX750",
        "1 4" => "VAX730",
        "1 5" => "UVAXI",
        "1 6" => "UVAXII",
        "1 7" => "VAX8200",
        "1 8" => "VAX8500",
        "1 9" => "VAX8600",
        "1 10" => "VAX8650",
        "1 11" => "VAX8800",
        "1 12" => "UVAXIII",
        "2 0" => "RT (all)",
        "2 1" => "RT PC",
        "2 2" => "RT APC",
        "2 3" => "RT 135",
        "4 0" => "NS32032 (all)",
        "4 1" => "NS32032 DPC (032 CPU)",
        "4 2" => "NS32032 SQT",
        "4 3" => "NS32032 APC FPU (32081)",
        "4 4" => "NS32032 APC FPA (Weitek)",
        "4 5" => "NS32032 XPC (532)",
        "5 0" => "NS32332 (all)",
        "5 1" => "NS32332 DPC (032 CPU)",
        "5 2" => "NS32332 SQT",
        "5 3" => "NS32332 APC FPU (32081)",
        "5 4" => "NS32332 APC FPA (Weitek)",
        "5 5" => "NS32332 XPC (532)",
        "6 1" => "MC680x0 (all)",
        "6 2" => "MC68040",
        "6 3" => "MC68030",
        "7 3" => "i386 (all)",
        "7 4" => "i486",
        "7 132" => "i486SX",
        "7 5" => "i586",
        "7 22" => "Pentium Pro",
        "7 54" => "Pentium II M3",
        "7 86" => "Pentium II M5",
        "7 103" => "Celeron",
        "7 119" => "Celeron Mobile",
        "7 8" => "Pentium III",
        "7 24" => "Pentium III M",
        "7 40" => "Pentium III Xeon",
        "7 9" => "Pentium M",
        "7 10" => "Pentium 4",
        "7 26" => "Pentium 4 M",
        "7 11" => "Itanium",
        "7 27" => "Itanium 2",
        "7 12" => "Xeon",
        "7 28" => "Xeon MP",
        "8 0" => "MIPS (all)",
        "8 1" => "MIPS R2300",
        "8 2" => "MIPS R2600",
        "8 3" => "MIPS R2800",
        "8 4" => "MIPS R2000a",
        "8 5" => "MIPS R2000",
        "8 6" => "MIPS R3000a",
        "8 7" => "MIPS R3000",
        "10 0" => "MC98000 (all)",
        "10 1" => "MC98601",
        "11 0" => "HPPA (all)",
        "11 1" => "HPPA 7100LC",
        "12 0" => "ARM (all)",
        "12 1" => "ARM A500 ARCH",
        "12 2" => "ARM A500",
        "12 3" => "ARM A440",
        "12 4" => "ARM M4",
        "12 5" => "ARM A680/V4T",
        "12 6" => "ARM V6",
        "12 7" => "ARM V5TEJ",
        "12 8" => "ARM XSCALE",
        "12 9" => "ARM V7",
        "13 0" => "MC88000 (all)",
        "13 1" => "MC88100",
        "13 2" => "MC88110",
        "14 0" => "SPARC (all)",
        "14 1" => "SUN 4/260",
        "14 2" => "SUN 4/110",
        "15 0" => "i860 (all)",
        "15 1" => "i860 860",
        "16 0" => "i860 little (all)",
        "16 1" => "i860 little",
        "17 0" => "RS6000 (all)",
        "17 1" => "RS6000",
        "18 0" => "PowerPC (all)",
        "18 1" => "PowerPC 601",
        "18 2" => "PowerPC 602",
        "18 3" => "PowerPC 603",
        "18 4" => "PowerPC 603e",
        "18 5" => "PowerPC 603ev",
        "18 6" => "PowerPC 604",
        "18 7" => "PowerPC 604e",
        "18 8" => "PowerPC 620",
        "18 9" => "PowerPC 750",
        "18 10" => "PowerPC 7400",
        "18 11" => "PowerPC 7450",
        "18 100" => "PowerPC 970",
        "255 1" => "VEO 1",
        "255 2" => "VEO 2",
        _ => "",
    }
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
