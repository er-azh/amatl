//! Finding the TrueType fonts compiled into the xochitl binary (Qt resources), which are the
//! fonts its PDF export embeds.
use skrifa::{FontRef, MetadataProvider, raw::TableProvider, string::StringId};

/// Each TrueType font stored whole in `bin`, as (PostScript name, file bytes).
pub fn fonts(bin: &[u8]) -> Vec<(String, Vec<u8>)> {
    let be16 = |i: usize| {
        bin.get(i..i + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
    };
    let be32 = |i: usize| {
        bin.get(i..i + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = bin[at..]
        .windows(4)
        .position(|w| w == [0, 1, 0, 0])
        .map(|i| i + at)
    {
        at = i + 1;
        let Some(n) = be16(i + 4).filter(|n| (4..=64).contains(n)) else {
            continue;
        };
        // The header's searchRange is 16 * the largest power of two <= numTables.
        if be16(i + 6) != Some(16 << n.ilog2()) {
            continue;
        }
        let mut end = 0;
        let mut ok = true;
        for t in 0..n {
            let r = i + 12 + t * 16;
            let tag = bin.get(r..r + 4).unwrap_or(&[0]);
            match (be32(r + 8), be32(r + 12)) {
                (Some(off), Some(len))
                    if tag.iter().all(|c| c.is_ascii_graphic() || *c == b' ') =>
                {
                    end = end.max(off + len)
                }
                _ => ok = false,
            }
        }
        let Some(data) = bin.get(i..i + end).filter(|_| ok) else {
            continue;
        };
        let Ok(font) = FontRef::new(data) else {
            continue;
        };
        if font.glyf().is_err() {
            continue;
        }
        let Some(name) = font
            .localized_strings(StringId::POSTSCRIPT_NAME)
            .english_or_first()
        else {
            continue;
        };
        let name: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        if !name.is_empty() {
            out.push((name, data.to_vec()));
            at = i + end;
        }
    }
    out
}
