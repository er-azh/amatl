//! Shaping with HarfBuzz (rustybuzz) and FreeType's advance arithmetic.

use std::collections::HashMap;

use rustybuzz::ttf_parser::GlyphId;

use super::fixed::Fixed;
use super::fonts::{Face, Fonts};
use super::style::Format;

/// Vertical metrics of a font at a pixel size, as Qt's FreeType engine reports them.
#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    pub ascent: Fixed,
    pub descent: Fixed,
}

pub(super) fn metrics(face: &rustybuzz::Face, px: u32) -> Metrics {
    let upm = face.units_per_em() as f64;
    let fixed = |v: i16| (v as f64 * px as f64 / upm * 64.0) as Fixed;
    Metrics {
        ascent: fixed(face.ascender()),
        descent: -fixed(face.descender()),
    }
}

fn ft_div_fix(a: Fixed, b: Fixed) -> Fixed {
    ((a << 16) + b / 2) / b
}

fn linear_advance(units: Fixed, x_scale: Fixed) -> Fixed {
    ((units * x_scale + 32) / 64) >> 10
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ShapedGlyph {
    pub id: u16,
    /// Index of the first character of the glyph's cluster.
    pub cluster: usize,
    /// Offset from that character in 26.6.
    pub dx: Fixed,
}

pub(super) fn shape(
    face: &rustybuzz::Face,
    px: u32,
    text: &[char],
) -> (Vec<Fixed>, Vec<ShapedGlyph>) {
    let s: String = text.iter().collect();
    let byte_to_char: HashMap<usize, usize> = s
        .char_indices()
        .enumerate()
        .map(|(i, (b, _))| (b, i))
        .collect();
    let mut buf = rustybuzz::UnicodeBuffer::new();
    buf.push_str(&s);
    let out = rustybuzz::shape(face, &[], buf);
    let upm = face.units_per_em() as f64;
    let x_scale = ft_div_fix(px as Fixed * 64, face.units_per_em() as Fixed);
    let mut adv = vec![0; text.len()];
    let mut glyphs = vec![];
    for (info, pos) in out.glyph_infos().iter().zip(out.glyph_positions()) {
        let ci = byte_to_char[&(info.cluster as usize)];
        let lin = face
            .glyph_hor_advance(GlyphId(info.glyph_id as u16))
            .unwrap_or(0) as Fixed;
        let a = linear_advance(lin, x_scale)
            + ((pos.x_advance as Fixed - lin) as f64 * px as f64 * 64.0 / upm).round() as Fixed;
        glyphs.push(ShapedGlyph {
            id: info.glyph_id as u16,
            cluster: ci,
            dx: adv[ci],
        });
        adv[ci] += a;
    }
    (adv, glyphs)
}

pub(super) fn cluster_text(chars: &[char], glyphs: &[ShapedGlyph], g: &ShapedGlyph) -> String {
    if g.dx != 0
        || glyphs
            .iter()
            .position(|h| h.cluster == g.cluster)
            .map(|i| &glyphs[i])
            != Some(g)
    {
        return String::new();
    }
    let end = glyphs
        .iter()
        .map(|h| h.cluster)
        .filter(|&c| c > g.cluster)
        .min()
        .unwrap_or(chars.len());
    chars[g.cluster..end]
        .iter()
        .filter(|&&c| c != '\n')
        .collect()
}

type Shaped = (Vec<Fixed>, Vec<ShapedGlyph>, HashMap<u8, Metrics>);

/// Shape a paragraph in runs of equal inline format (each run in its own font, so no kerning
/// across a format change). Returns the advances and glyphs as `shape` does, and the vertical
/// metrics of each run's font. Characters whose font is missing fall back to the plain one
/// (their flags are cleared).
pub(super) fn shape_runs(fonts: &mut Fonts, f: Format, chars: &[char], flags: &mut [u8]) -> Shaped {
    let mut data = HashMap::new();
    for fl in flags.iter_mut() {
        if !data.contains_key(fl) {
            match fonts.data(f.font(*fl)) {
                Some(d) => {
                    data.insert(*fl, d);
                }
                None => *fl = 0,
            }
        }
    }
    let faces: HashMap<u8, rustybuzz::Face> = data
        .iter()
        .filter_map(|(&k, d)| Some((k, rustybuzz::Face::from_slice(d, 0)?)))
        .collect();
    let mut adv = vec![0; chars.len()];
    let mut glyphs = vec![];
    let mut start = 0;
    while start < chars.len() {
        let fl = flags[start];
        let end = (start..chars.len())
            .find(|&i| flags[i] != fl)
            .unwrap_or(chars.len());
        if let Some(face) = faces.get(&fl) {
            let (a, g) = shape(face, f.px, &chars[start..end]);
            adv[start..end].copy_from_slice(&a);
            glyphs.extend(g.into_iter().map(|g| ShapedGlyph {
                cluster: g.cluster + start,
                ..g
            }));
        }
        start = end;
    }
    let metrics = faces
        .iter()
        .map(|(&k, face)| (k, metrics(face, f.px)))
        .collect();
    (adv, glyphs, metrics)
}

pub fn shape_run(
    fonts: &mut Fonts,
    font: Face,
    px: u32,
    text: &str,
) -> Option<crate::template::TextRun> {
    let data = fonts.data(font)?;
    let face = rustybuzz::Face::from_slice(&data, 0)?;
    let chars: Vec<char> = text.chars().collect();
    let (_, glyphs) = shape(&face, px, &chars);
    let mut buf = rustybuzz::UnicodeBuffer::new();
    buf.push_str(text);
    let shaped = rustybuzz::shape(&face, &[], buf);
    let upm = face.units_per_em() as f64;
    let k = px as f64 / upm;
    let hinted = HintedAdvances::new(&data, px)?;
    let (mut x, mut out, mut ink) = (0.0, vec![], (f64::MAX, f64::MIN));
    for (g, pos) in glyphs.iter().zip(shaped.glyph_positions()) {
        if let Some(b) = face.glyph_bounding_box(GlyphId(g.id)) {
            ink.0 = ink.0.min(x + b.x_min as f64 * k);
            ink.1 = ink.1.max(x + b.x_max as f64 * k);
        }
        out.push((g.id, x, cluster_text(&chars, &glyphs, g)));
        let lin = face.glyph_hor_advance(GlyphId(g.id)).unwrap_or(0) as Fixed;
        let kern = (pos.x_advance as Fixed - lin) as f64 * k;
        x += hinted.get(g.id)? + kern.round();
    }
    if ink.0 > ink.1 {
        ink = (0.0, 0.0);
    }
    Some(crate::template::TextRun {
        font: font.name(),
        data: data.clone(),
        px,
        x: 0.0,
        y: 0.0,
        color: 0xff00_0000,
        glyphs: out,
        ink,
    })
}

struct HintedAdvances<'a> {
    outlines: skrifa::outline::OutlineGlyphCollection<'a>,
    hinter: skrifa::outline::HintingInstance,
}

impl<'a> HintedAdvances<'a> {
    fn new(data: &'a [u8], px: u32) -> Option<Self> {
        use skrifa::MetadataProvider;
        let font = skrifa::FontRef::new(data).ok()?;
        let outlines = font.outline_glyphs();
        let hinter = skrifa::outline::HintingInstance::new(
            &outlines,
            skrifa::instance::Size::new(px as f32),
            skrifa::instance::LocationRef::default(),
            skrifa::outline::HintingOptions::default(),
        )
        .ok()?;
        Some(Self { outlines, hinter })
    }

    fn get(&self, glyph: u16) -> Option<f64> {
        let g = self.outlines.get(skrifa::GlyphId::new(glyph as u32))?;
        let m = g
            .draw(
                skrifa::outline::DrawSettings::hinted(&self.hinter, false),
                &mut skrifa::outline::pen::NullPen,
            )
            .ok()?;
        Some(m.advance_width? as f64)
    }
}
