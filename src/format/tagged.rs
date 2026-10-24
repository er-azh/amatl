//! Low-level reader for the v6 ".lines" tagged binary encoding.
//!
//! A file is `HEADER` followed by blocks:
//!   u32 length, u8 unknown, u8 min_version, u8 current_version, u8 block_type, payload[length]
//! A payload is a sequence of tagged values. A tag is a varuint `(index << 4) | type`:
//!   0x1 u8, 0x4 u32/f32, 0x8 f64, 0xC u32 length + nested bytes, 0xF CRDT id (u8 + varuint).

use crate::Error;
use crate::format::crdt::CrdtId;

pub const HEADER: &[u8; 43] = b"reMarkable .lines file, version=6          ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TagType {
    Byte1 = 0x1,
    Byte4 = 0x4,
    Byte8 = 0x8,
    Length4 = 0xC,
    Id = 0xF,
}

impl TagType {
    fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0x1 => Self::Byte1,
            0x4 => Self::Byte4,
            0x8 => Self::Byte8,
            0xC => Self::Length4,
            0xF => Self::Id,
            _ => return None,
        })
    }
}

/// Cursor over a byte slice. All reads are bounds checked and report the absolute file offset.
#[derive(Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    end: usize,
    /// Absolute offset of `data[0]` in the file, for error messages.
    base: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader {
            data,
            pos: 0,
            end: data.len(),
            base: 0,
        }
    }

    pub fn pos(&self) -> usize {
        self.base + self.pos
    }

    pub fn remaining(&self) -> usize {
        self.end - self.pos
    }

    pub fn is_empty(&self) -> bool {
        self.pos >= self.end
    }

    fn err(&self, msg: impl Into<String>) -> Error {
        Error::Format {
            offset: self.pos(),
            msg: msg.into(),
        }
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.remaining() < n {
            return Err(self.err(format!("need {n} bytes, have {}", self.remaining())));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.bytes(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub fn f64(&mut self) -> Result<f64, Error> {
        Ok(f64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }

    pub fn varuint(&mut self) -> Result<u64, Error> {
        let mut r = 0u64;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            if shift >= 64 {
                return Err(self.err("varuint overflow"));
            }
            r |= u64::from(b & 0x7f) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                return Ok(r);
            }
        }
    }

    pub fn crdt_id(&mut self) -> Result<CrdtId, Error> {
        let a = self.u8()?;
        let b = self.varuint()?;
        Ok(CrdtId(a, b))
    }

    /// Split off the next `n` bytes as a sub-reader and advance past them.
    pub fn sub(&mut self, n: usize) -> Result<Reader<'a>, Error> {
        let start = self.pos;
        self.bytes(n)?;
        Ok(Reader {
            data: self.data,
            pos: start,
            end: start + n,
            base: self.base,
        })
    }

    /// Peek the next tag without consuming it.
    pub fn peek_tag(&self) -> Option<(u64, TagType)> {
        let mut c = self.clone();
        let x = c.varuint().ok()?;
        Some((x >> 4, TagType::from_u8((x & 0xf) as u8)?))
    }

    pub fn has_tag(&self, index: u64, ty: TagType) -> bool {
        self.peek_tag() == Some((index, ty))
    }

    fn expect_tag(&mut self, index: u64, ty: TagType) -> Result<(), Error> {
        let at = self.pos();
        let x = self.varuint()?;
        let (gi, gt) = (x >> 4, (x & 0xf) as u8);
        if gi != index || gt != ty as u8 {
            return Err(Error::Format {
                offset: at,
                msg: format!("expected tag [{index}] {ty:?}, got [{gi}] type {gt:#x}"),
            });
        }
        Ok(())
    }

    pub fn tag_u8(&mut self, i: u64) -> Result<u8, Error> {
        self.expect_tag(i, TagType::Byte1)?;
        self.u8()
    }
    pub fn tag_bool(&mut self, i: u64) -> Result<bool, Error> {
        Ok(self.tag_u8(i)? != 0)
    }
    pub fn tag_u32(&mut self, i: u64) -> Result<u32, Error> {
        self.expect_tag(i, TagType::Byte4)?;
        self.u32()
    }
    pub fn tag_f32(&mut self, i: u64) -> Result<f32, Error> {
        self.expect_tag(i, TagType::Byte4)?;
        self.f32()
    }
    pub fn tag_f64(&mut self, i: u64) -> Result<f64, Error> {
        self.expect_tag(i, TagType::Byte8)?;
        self.f64()
    }
    pub fn tag_id(&mut self, i: u64) -> Result<CrdtId, Error> {
        self.expect_tag(i, TagType::Id)?;
        self.crdt_id()
    }
    /// A length-prefixed nested block.
    pub fn tag_sub(&mut self, i: u64) -> Result<Reader<'a>, Error> {
        self.expect_tag(i, TagType::Length4)?;
        let n = self.u32()? as usize;
        self.sub(n)
    }

    /// Read an optional tag if it is next in the stream.
    pub fn opt<T>(
        &mut self,
        i: u64,
        ty: TagType,
        f: impl FnOnce(&mut Self, u64) -> Result<T, Error>,
    ) -> Result<Option<T>, Error> {
        if self.has_tag(i, ty) {
            f(self, i).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Consume and return whatever is left; callers record it so nothing is dropped silently.
    pub fn rest(&mut self) -> &'a [u8] {
        let s = &self.data[self.pos..self.end];
        self.pos = self.end;
        s
    }
}

/// Header of one top-level block.
#[derive(Debug, Clone, Copy)]
pub struct BlockInfo {
    pub offset: usize,
    pub block_type: u8,
}

/// Iterate the top-level blocks of a v6 file.
pub fn blocks(data: &[u8]) -> Result<Vec<(BlockInfo, Reader<'_>)>, Error> {
    let mut r = Reader::new(data);
    if r.bytes(HEADER.len()).ok() != Some(&HEADER[..]) {
        return Err(Error::Format {
            offset: 0,
            msg: "not a v6 .rm file".into(),
        });
    }
    let mut out = Vec::new();
    while !r.is_empty() {
        let offset = r.pos();
        let size = r.u32()?;
        // unknown, min_version, current_version
        r.bytes(3)?;
        let block_type = r.u8()?;
        let body = r.sub(size as usize)?;
        out.push((BlockInfo { offset, block_type }, body));
    }
    Ok(out)
}
