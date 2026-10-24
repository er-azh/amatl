//! Embedded fonts: TrueType subsets as Type0/CIDFontType2 with a ToUnicode map.

use pdf_writer::{Finish, Name, Pdf, Rect, Ref};

/// The index of font `name` in `fonts`, adding it if new.
pub fn font_index(
    fonts: &mut Vec<Font>,
    name: &'static str,
    data: &std::sync::Arc<Vec<u8>>,
) -> usize {
    match fonts.iter().position(|f| f.name == name) {
        Some(i) => i,
        None => {
            fonts.push(Font::new(name, data.clone()));
            fonts.len() - 1
        }
    }
}

/// A font of the text, subset to the glyphs used; CIDs number them in order of first use, with
/// the subset's glyph ids equal to them.
pub struct Font {
    pub name: &'static str,
    data: std::sync::Arc<Vec<u8>>,
    remap: subsetter::GlyphRemapper,
    text: Vec<String>,
}

impl Font {
    pub fn new(name: &'static str, data: std::sync::Arc<Vec<u8>>) -> Font {
        Font {
            name,
            data,
            remap: subsetter::GlyphRemapper::new(),
            text: vec![String::new()],
        }
    }

    pub fn cid(&mut self, gid: u16, text: &str) -> u16 {
        let cid = self.remap.remap(gid);
        if cid as usize == self.text.len() {
            self.text.push(text.into());
        }
        cid
    }

    pub fn write(&self, pdf: &mut Pdf, alloc: &mut impl FnMut() -> Ref) -> Ref {
        use pdf_writer::Str;
        use pdf_writer::types::{CidFontType, FontFlags, SystemInfo, UnicodeCmap};
        let (type0, cid, desc, file, cmap) = (alloc(), alloc(), alloc(), alloc(), alloc());
        let face = rustybuzz::ttf_parser::Face::parse(&self.data, 0).ok();
        let upm = face.as_ref().map_or(1000.0, |f| f.units_per_em() as f32);
        let k = 1000.0 / upm;
        let olds: Vec<u16> = self.remap.remapped_gids().collect();
        let widths: Vec<f32> = olds
            .iter()
            .map(|&g| {
                let a = face
                    .as_ref()
                    .and_then(|f| f.glyph_hor_advance(rustybuzz::ttf_parser::GlyphId(g)));
                (a.unwrap_or(0) as f32 * k).round()
            })
            .collect();
        // Subset tag: six letters from the glyph set.
        let mut h: u32 = 2166136261;
        for &g in &olds {
            h = (h ^ g as u32).wrapping_mul(16777619);
        }
        let tag: String = (0..6)
            .map(|i| (b'A' + ((h >> (i * 5)) & 31) as u8 % 26) as char)
            .collect();
        let base = format!("{tag}+{}", self.name);
        let info = SystemInfo {
            registry: Str(b"Adobe"),
            ordering: Str(b"Identity"),
            supplement: 0,
        };

        pdf.type0_font(type0)
            .base_font(Name(base.as_bytes()))
            .encoding_predefined(Name(b"Identity-H"))
            .descendant_font(cid)
            .to_unicode(cmap);
        let mut c = pdf.cid_font(cid);
        c.subtype(CidFontType::Type2)
            .base_font(Name(base.as_bytes()))
            .system_info(info)
            .font_descriptor(desc)
            .cid_to_gid_map_predefined(Name(b"Identity"));
        c.widths().consecutive(0, widths);
        c.finish();

        let (asc, desc_, bbox) = match &face {
            Some(f) => {
                let b = f.global_bounding_box();
                (
                    f.ascender() as f32 * k,
                    f.descender() as f32 * k,
                    Rect::new(
                        b.x_min as f32 * k,
                        b.y_min as f32 * k,
                        b.x_max as f32 * k,
                        b.y_max as f32 * k,
                    ),
                )
            }
            None => (0.0, 0.0, Rect::new(0.0, 0.0, 0.0, 0.0)),
        };
        pdf.font_descriptor(desc)
            .name(Name(base.as_bytes()))
            .flags(FontFlags::SYMBOLIC)
            .bbox(bbox)
            .italic_angle(0.0)
            .ascent(asc)
            .descent(desc_)
            .cap_height(asc)
            .stem_v(50.0)
            .font_file2(file);
        let sub =
            subsetter::subset(&self.data, 0, &self.remap).unwrap_or_else(|_| self.data.to_vec());
        pdf.stream(file, &sub)
            .pair(Name(b"Length1"), sub.len() as i32);

        let mut u = UnicodeCmap::new(Name(b"Adobe-Identity-UCS"), info);
        for (i, t) in self.text.iter().enumerate().skip(1) {
            if !t.is_empty() {
                u.pair_with_multiple(i as u16, t.chars());
            }
        }
        pdf.cmap(cmap, &u.finish());
        type0
    }
}
