//! The installer package `SETUP.DAT` ("PQSetup data for AC").
//!
//! Layout:
//!
//! ```text
//! 0x00      text banner, NUL padded
//! ...       file payloads, each split into blocks of up to 128 KiB, every
//!           block an independent LZ stream (see `lz`)
//! dir       directory: `count` variable-size entries
//! end - 8   u32 directory offset, u32 entry count
//! ```
//!
//! Directory entry (`size` excludes the size field itself):
//!
//! ```text
//! +0x000  u32   size
//! +0x004  u32   unknown (0)
//! +0x008  u16   flags (0x8001)
//! +0x00a  char  name[0x20d]
//! +0x217  SYSTEMTIME (16 bytes) file timestamp
//! +0x227  u32   block count
//! +0x22b  u32   unknown (checksum?)
//! +0x22f  u32   compressed size of each block
//! ```

use crate::bytes::{latin1_cstr, Reader};
use crate::{invalid, lz, Result};

const NAME_LEN: usize = 0x20d;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
}

#[derive(Debug, Clone)]
pub struct InstallerFile {
    pub name: String,
    pub flags: u16,
    pub modified: Timestamp,
    /// Offset of the first block in the package.
    pub offset: usize,
    pub block_sizes: Vec<u32>,
}

impl InstallerFile {
    pub fn stored_size(&self) -> usize {
        self.block_sizes.iter().map(|&s| s as usize).sum()
    }
}

pub struct Installer<'a> {
    data: &'a [u8],
    files: Vec<InstallerFile>,
}

impl<'a> Installer<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        if data.len() < 8 {
            return Err(invalid("installer", "file too small"));
        }
        let mut tail = Reader::at(data, data.len() - 8, "installer trailer");
        let dir_offset = tail.u32()? as usize;
        let count = tail.u32()? as usize;
        if dir_offset >= data.len() {
            return Err(invalid("installer", "directory offset out of range"));
        }

        let mut files = Vec::with_capacity(count);
        let mut pos = dir_offset;
        for _ in 0..count {
            let mut r = Reader::at(data, pos, "installer directory");
            let size = r.u32()? as usize;
            let body = r.bytes(size)?;
            let mut b = Reader::new(body, "installer directory entry");
            let _unknown = b.u32()?;
            let flags = b.u16()?;
            let name = latin1_cstr(b.bytes(NAME_LEN)?);
            let year = b.u16()?;
            let month = b.u16()?;
            let _day_of_week = b.u16()?;
            let day = b.u16()?;
            let hour = b.u16()?;
            let minute = b.u16()?;
            let second = b.u16()?;
            let _millis = b.u16()?;
            let blocks = b.u32()? as usize;
            let _checksum = b.u32()?;
            let block_sizes = (0..blocks).map(|_| b.u32()).collect::<Result<Vec<_>>>()?;
            files.push(InstallerFile {
                name,
                flags,
                modified: Timestamp {
                    year,
                    month,
                    day,
                    hour,
                    minute,
                    second,
                },
                offset: 0,
                block_sizes,
            });
            pos += 4 + size;
        }

        // Payloads are stored back to back, ending right where the directory starts.
        let total: usize = files.iter().map(InstallerFile::stored_size).sum();
        let mut offset = dir_offset
            .checked_sub(total)
            .ok_or_else(|| invalid("installer", "payload sizes exceed directory offset"))?;
        for f in &mut files {
            f.offset = offset;
            offset += f.stored_size();
        }
        Ok(Self { data, files })
    }

    pub fn files(&self) -> &[InstallerFile] {
        &self.files
    }

    /// Decompresses one file.
    pub fn extract(&self, file: &InstallerFile) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(file.block_sizes.len() * 0x20000);
        let mut pos = file.offset;
        for &size in &file.block_sizes {
            let size = size as usize;
            let block = self.data.get(pos..pos + size).ok_or_else(|| {
                invalid("installer", format!("{}: block out of range", file.name))
            })?;
            let d = lz::decompress(block, 0x20000)?;
            if d.consumed != size {
                return Err(invalid(
                    "installer",
                    format!(
                        "{}: block consumed {} of {} bytes",
                        file.name, d.consumed, size
                    ),
                ));
            }
            out.extend_from_slice(&d.data);
            pos += size;
        }
        Ok(out)
    }
}
