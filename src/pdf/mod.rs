//! PDF output in the layout of xochitl's exporter (PDFium): a page stream that leaves a
//! `72/227` y-down transform in place (where templates draw), then an annotation stream in
//! points (y up), mapped back into that pixel space by its own `cm`. The annotation stream
//! holds the image items (as image objects) and pass-through strokes first, in z order, then
//! each traced region as one even-odd filled path. A page with nothing for that stream gets
//! none: PDFium leaves pages it adds no objects to as Qt wrote them.
//! Graphics state is written the way PDFium's content generator does: defaults once, then per
//! object `q`, colors, non-default line parameters, and an ExtGState only when it changes.

mod annots;
mod font;
mod image;
pub(crate) mod jpeg;
mod number;
pub(crate) mod overlay;

pub use overlay::Error as OverlayError;
mod page;
mod template;

use std::sync::Arc;

use pdf_writer::{Finish, Name, Pdf, Rect, Ref};

use crate::render::DPI;
use crate::template::Image;
use crate::trace::Group;
use font::Font;

/// PDF points per device pixel.
pub(crate) const PT_PER_PX: f32 = 72.0 / DPI as f32;

/// A stroked polyline (fineliner/highlighter), coordinates in device pixels.
pub(crate) struct Stroke {
    /// Non-premultiplied ARGB.
    pub color: u32,
    pub width: f32,
    /// PDF line cap / join codes.
    pub cap: u8,
    pub join: u8,
    /// Composited with `Darken` (highlighters).
    pub darken: bool,
    pub points: Vec<(f32, f32)>,
}

/// One line of the root text, drawn in the page stream as Qt's PDF engine writes text: a glyph
/// at a time, as CIDs of an embedded subset of the TrueType font.
pub(crate) struct TextLine {
    /// PostScript name of the font.
    pub font: &'static str,
    pub data: std::sync::Arc<Vec<u8>>,
    pub px: u32,
    /// Text origin and baseline in device pixels.
    pub origin: (f32, f32),
    pub baseline: f32,
    /// Glyph id, x relative to the origin, and the characters it stands for.
    pub glyphs: Vec<(u16, f32, String)>,
}

/// A black horizontal line (strikeout) from `x0` to `x1` at `y`, relative to the text origin;
/// device pixels.
pub(crate) struct Rule {
    pub origin: (f32, f32),
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    pub width: f32,
}

/// A text highlight: color (ARGB) and rects (x, y, w, h) in device pixels.
pub(crate) struct Highlight {
    pub color: u32,
    pub rects: Vec<(f32, f32, f32, f32)>,
}

/// Straight (not premultiplied) RGBA pixels, row by row.
pub(crate) struct Bitmap {
    pub w: u32,
    pub h: u32,
    pub px: Vec<[u8; 4]>,
    /// Whether the alpha channel is written (as a soft mask): xochitl hands PDFium an
    /// `RGB32` image as BGRx, anything else converted to `ARGB32` as BGRA.
    pub alpha: bool,
}

/// An image item, drawn in the annotation stream as a PDFium image object.
pub(crate) struct Picture {
    pub bitmap: Arc<Bitmap>,
    /// The item's transform and its group's offset, in device pixels: it maps the rect
    /// (-0.5, -0.5, 1, 1) the image fills.
    pub m: crate::render::Affine,
    /// How many of the page's strokes come before it: PDFium gets image objects and the
    /// pass-through strokes in z order.
    pub before: usize,
}

/// One page's objects, until the shared ones (graphics states, fonts, images) are written.
struct PageObjects {
    id: Ref,
    base: Ref,
    /// None for a page with nothing in the annotation layer.
    annots: Option<Ref>,
    /// Media box size in points.
    w: f32,
    h: f32,
    /// Names of the ExtGStates the annotation stream uses.
    states: Vec<String>,
    fonts: Vec<usize>,
    images: Vec<usize>,
    /// Image items' bitmaps, as `/FXXn` in order.
    pictures: Vec<Ref>,
}

/// One PDF page as [`write()`] draws it; built by [`crate::export::page`].
pub struct Page {
    /// Page size in device pixels.
    pub(crate) w: u32,
    pub(crate) h: f64,
    /// Template draws, first in the page stream.
    pub(crate) template: Option<Vec<crate::template::Draw>>,
    pub(crate) text: Vec<TextLine>,
    /// Strikeouts.
    pub(crate) rules: Vec<Rule>,
    /// Black even-odd filled paths drawn with the text (checkboxes), in device pixels.
    pub(crate) shapes: Vec<Vec<crate::template::Seg>>,
    /// Text highlights on a PDF page.
    pub(crate) highlights: Vec<Highlight>,
    pub(crate) strokes: Vec<Stroke>,
    /// Image items.
    pub(crate) pictures: Vec<Picture>,
    /// Traced regions, coordinates in 1/8 device pixels.
    pub(crate) groups: Vec<Group>,
}

pub fn write(pages: &[Page]) -> Vec<u8> {
    write_pages(pages, false)
}

pub(crate) fn write_pages(pages: &[Page], annots: bool) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let mut next = Ref::new(1);
    let mut alloc = || next.bump();
    let catalog = alloc();
    let tree = alloc();
    let page_ids: Vec<Ref> = pages.iter().map(|_| alloc()).collect();

    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree)
        .kids(page_ids.iter().copied())
        .count(pages.len() as i32);

    let mut states = annots::States::default();
    let mut fonts: Vec<Font> = vec![];
    let mut images: Vec<Arc<Image>> = vec![];
    let mut page_states = Vec::new();
    for (page, &id) in pages.iter().zip(&page_ids) {
        let (wpt, hpt) = (
            (page.w as f32 * PT_PER_PX).round(),
            (page.h as f32 * PT_PER_PX).round(),
        );
        let base = alloc();
        let (c, page_fonts, page_images) = page::stream(page, hpt, &mut fonts, &mut images);
        pdf.stream(base, c.as_bytes());
        let empty = !annots
            && page.highlights.is_empty()
            && page.strokes.is_empty()
            && page.pictures.is_empty()
            && page.groups.is_empty();
        let (annot_stream, used) = if empty {
            (None, vec![])
        } else {
            let (out, used) = annots::stream(page, hpt, &mut states);
            let r = alloc();
            pdf.stream(r, out.as_bytes());
            (Some(r), used)
        };
        // PDFium writes each image object's bitmap as its own XObject.
        let pictures = page
            .pictures
            .iter()
            .map(|p| image::write_bitmap(&mut pdf, &mut alloc, &p.bitmap))
            .collect();
        page_states.push(PageObjects {
            id,
            base,
            annots: annot_stream,
            w: wpt,
            h: hpt,
            states: used,
            fonts: page_fonts,
            images: page_images,
            pictures,
        });
    }

    let gs_ids: Vec<Ref> = states.0.iter().map(|_| alloc()).collect();
    for (st, &gid) in states.0.iter().zip(&gs_ids) {
        let mut g = pdf.ext_graphics(gid);
        g.blend_mode(if st.darken {
            pdf_writer::types::BlendMode::Darken
        } else {
            pdf_writer::types::BlendMode::Normal
        });
        if let Some(a) = st.stroke_alpha {
            g.stroking_alpha(a as f32 / 255.0);
        }
        if let Some(a) = st.fill_alpha {
            g.non_stroking_alpha(a as f32 / 255.0);
        }
    }
    // Qt's graphics state for opaque painting, which the page stream sets first.
    let gsa = alloc();
    pdf.ext_graphics(gsa)
        .stroking_alpha(1.0)
        .non_stroking_alpha(1.0)
        .pair(Name(b"SA"), true)
        .pair(Name(b"SM"), 0.02)
        .pair(Name(b"AIS"), false)
        .pair(Name(b"SMask"), Name(b"None"));
    let font_ids: Vec<Ref> = fonts
        .iter()
        .map(|f| f.write(&mut pdf, &mut alloc))
        .collect();
    let image_ids: Vec<Ref> = images
        .iter()
        .map(|i| image::write_image(&mut pdf, &mut alloc, i))
        .collect();
    for po in page_states {
        let mut p = pdf.page(po.id);
        p.parent(tree).media_box(Rect::new(0.0, 0.0, po.w, po.h));
        match po.annots {
            Some(a) => p.contents_array([po.base, a]),
            None => p.contents(po.base),
        };
        let mut res = p.resources();
        res.insert(Name(b"ColorSpace"))
            .dict()
            .pair(Name(b"CSp"), Name(b"DeviceRGB"));
        let mut g = res.ext_g_states();
        g.pair(Name(b"GSa"), gsa);
        for name in &po.states {
            let i: usize = name[4..].parse().unwrap();
            g.pair(Name(&name.as_bytes()[1..]), gs_ids[i - 1]);
        }
        g.finish();
        if !po.fonts.is_empty() {
            let mut f = res.fonts();
            for i in po.fonts {
                f.pair(Name(format!("F{i}").as_bytes()), font_ids[i]);
            }
        }
        if !po.images.is_empty() || !po.pictures.is_empty() {
            let mut x = res.x_objects();
            for i in po.images {
                x.pair(Name(format!("Im{i}").as_bytes()), image_ids[i]);
            }
            for (i, &r) in po.pictures.iter().enumerate() {
                x.pair(Name(format!("FXX{}", i + 1).as_bytes()), r);
            }
        }
        res.finish();
        p.finish();
    }
    pdf.finish()
}
