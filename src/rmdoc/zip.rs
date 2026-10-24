//! Minimal zip reader for `.rmdoc` archives: stored and deflated entries, read through the
//! central directory (so entries written with data descriptors work). No zip64 or encryption.

fn u16_at(d: &[u8], i: usize) -> Option<usize> {
    Some(u16::from_le_bytes(d.get(i..i + 2)?.try_into().ok()?) as usize)
}

fn u32_at(d: &[u8], i: usize) -> Option<usize> {
    Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?) as usize)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no end of central directory")]
    NoDirectory,
    #[error("truncated central directory")]
    BadDirectory,
    #[error("{0}: encrypted")]
    Encrypted(String),
    #[error("{0}: zip64 is not supported")]
    Zip64(String),
    #[error("{0}: bad local header")]
    BadLocalHeader(String),
    #[error("{0}: truncated")]
    Truncated(String),
    #[error("{0}: inflate: {1:?}")]
    Inflate(String, miniz_oxide::inflate::TINFLStatus),
    #[error("{0}: compression method {1} is not supported")]
    Method(String, usize),
}

/// Upper bound on one entry's inflated size.
const MAX_ENTRY: usize = 1 << 30;

/// The archive's file entries as (name, contents), in central directory order.
pub fn entries(d: &[u8]) -> Result<Vec<(String, Vec<u8>)>, Error> {
    // End of central directory: 22 bytes plus a comment of up to 64 KiB.
    let lo = d.len().saturating_sub(22 + 0xffff);
    let eocd = (lo..d.len().saturating_sub(21))
        .rev()
        .find(|&i| d[i..i + 4] == *b"PK\x05\x06")
        .ok_or(Error::NoDirectory)?;
    let count = u16_at(d, eocd + 10).ok_or(Error::BadDirectory)?;
    let mut at = u32_at(d, eocd + 16).ok_or(Error::BadDirectory)?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if d.get(at..at + 4) != Some(b"PK\x01\x02") {
            return Err(Error::BadDirectory);
        }
        let flags = u16_at(d, at + 8).ok_or(Error::BadDirectory)?;
        let method = u16_at(d, at + 10).ok_or(Error::BadDirectory)?;
        let csize = u32_at(d, at + 20).ok_or(Error::BadDirectory)?;
        let usize_ = u32_at(d, at + 24).ok_or(Error::BadDirectory)?;
        let (nlen, xlen, clen) = (
            u16_at(d, at + 28).ok_or(Error::BadDirectory)?,
            u16_at(d, at + 30).ok_or(Error::BadDirectory)?,
            u16_at(d, at + 32).ok_or(Error::BadDirectory)?,
        );
        let local = u32_at(d, at + 42).ok_or(Error::BadDirectory)?;
        let name =
            String::from_utf8_lossy(d.get(at + 46..at + 46 + nlen).ok_or(Error::BadDirectory)?)
                .into_owned();
        at += 46 + nlen + xlen + clen;
        if name.ends_with('/') {
            continue;
        }
        if flags & 1 != 0 {
            return Err(Error::Encrypted(name));
        }
        if csize == 0xffff_ffff || usize_ == 0xffff_ffff || local == 0xffff_ffff {
            return Err(Error::Zip64(name));
        }
        if d.get(local..local + 4) != Some(b"PK\x03\x04") {
            return Err(Error::BadLocalHeader(name));
        }
        let start = local
            + 30
            + u16_at(d, local + 26).ok_or(Error::BadDirectory)?
            + u16_at(d, local + 28).ok_or(Error::BadDirectory)?;
        let raw = d
            .get(start..start + csize)
            .ok_or_else(|| Error::Truncated(name.clone()))?;
        let data = match method {
            0 => raw.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, MAX_ENTRY)
                .map_err(|e| Error::Inflate(name.clone(), e.status))?,
            m => return Err(Error::Method(name, m)),
        };
        out.push((name, data));
    }
    Ok(out)
}
