//! Bounds-checked little-endian reading helpers.

use crate::{invalid, Result};

#[derive(Clone)]
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    what: &'static str,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], what: &'static str) -> Self {
        Self { data, pos: 0, what }
    }

    pub fn at(data: &'a [u8], pos: usize, what: &'static str) -> Self {
        Self { data, pos, what }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&end| end <= self.data.len())
            .ok_or_else(|| invalid(self.what, format!("truncated at offset {:#x}", self.pos)))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
}

/// Decodes a NUL-terminated Latin-1 string from a fixed-size field.
pub(crate) fn latin1_cstr(field: &[u8]) -> String {
    field
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .collect()
}
