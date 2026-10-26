//! The string table of a Windows executable (`ancient.exe`): its
//! `RT_STRING` resources, sixteen strings to a block, block `b` holding ids
//! `(b - 1) * 16 ..`.

use std::collections::HashMap;

use crate::invalid;
use crate::Result;

/// The executable's strings by id.
pub fn string_table(exe: &[u8]) -> Result<HashMap<u32, String>> {
    let u16_at = |o: usize| -> Result<u16> {
        exe.get(o..o + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .ok_or_else(|| invalid("executable", "truncated"))
    };
    let u32_at = |o: usize| -> Result<u32> {
        exe.get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| invalid("executable", "truncated"))
    };
    let pe = u32_at(0x3c)? as usize;
    if exe.get(pe..pe + 4) != Some(b"PE\0\0") {
        return Err(invalid("executable", "no PE header"));
    }
    let sections = u16_at(pe + 6)? as usize;
    let optional = u16_at(pe + 20)? as usize;
    // The resource directory: data directory 2 of the optional header.
    let rsrc_rva = u32_at(pe + 24 + 96 + 2 * 8)?;
    let mut table = Vec::new();
    for i in 0..sections {
        let s = pe + 24 + optional + i * 40;
        let (size, va, raw) = (u32_at(s + 8)?, u32_at(s + 12)?, u32_at(s + 20)?);
        table.push((va, size.max(1), raw));
    }
    let offset = |rva: u32| -> Result<usize> {
        table
            .iter()
            .find(|&&(va, size, _)| (va..va + size).contains(&rva))
            .map(|&(va, _, raw)| (rva - va + raw) as usize)
            .ok_or_else(|| invalid("executable", "resource outside the sections"))
    };
    let base = offset(rsrc_rva)?;
    // A resource directory's entries: (id, offset of what it points to).
    let entries = |o: usize| -> Result<Vec<(u32, usize)>> {
        let n = u16_at(o + 12)? as usize + u16_at(o + 14)? as usize;
        (0..n)
            .map(|i| {
                Ok((
                    u32_at(o + 16 + i * 8)?,
                    (u32_at(o + 20 + i * 8)? & 0x7fff_ffff) as usize,
                ))
            })
            .collect()
    };
    let mut strings = HashMap::new();
    for (kind, types) in entries(base)? {
        if kind != 6 {
            continue;
        }
        for (block, langs) in entries(base + types)? {
            for (_, leaf) in entries(base + langs)? {
                let mut o = offset(u32_at(base + leaf)?)?;
                for k in 0..16 {
                    let len = u16_at(o)? as usize;
                    let units = (0..len)
                        .map(|j| u16_at(o + 2 + 2 * j))
                        .collect::<Result<Vec<_>>>()?;
                    o += 2 + 2 * len;
                    if len > 0 {
                        let text = char::decode_utf16(units)
                            .map(|c| c.unwrap_or('?'))
                            .collect();
                        strings.insert((block - 1) * 16 + k, text);
                    }
                }
            }
        }
    }
    Ok(strings)
}
