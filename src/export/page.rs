//! One page through the pipeline: lay out text, rasterize and trace the strokes, and collect
//! everything a PDF page draws.
use super::Assets;
use crate::format::{ItemValue, Line, Rect, RootText, Scene, Tool};
use crate::pdf::{Highlight, Rule, Stroke, TextLine};
use crate::render::{DPI, Paper, Renderer, anchor_offsets, content_rect};
use crate::rmdoc::Orientation;
use crate::text::{Fonts, Layout, layout, to_px};
use crate::trace::{P, Pt};

/// One page of `paper` as xochitl's PDF exporter draws it.
pub fn page(
    sc: &Scene,
    template: &str,
    orientation: Orientation,
    paper: Paper,
    assets: &mut Assets,
    warn: &mut dyn FnMut(String),
) -> crate::pdf::Page {
    draw(
        sc,
        template,
        paper,
        Size::Grow(paper.page(orientation)),
        assets,
        warn,
    )
    .0
}

/// A page's size in device pixels.
#[derive(Debug, Clone, Copy)]
pub enum Size {
    /// A note page: widened or grown down to hold its content, as xochitl does.
    Grow((u32, u32)),
    /// A PDF page annotated over: the content may reach past it.
    Fixed((u32, u32)),
}

/// A page of `size` (the scene's x origin at its center) and the content rect (x from the
/// center, y, w, h). `paper` is the template's.
pub fn draw(
    sc: &Scene,
    template: &str,
    paper: Paper,
    size: Size,
    assets: &mut Assets,
    warn: &mut dyn FnMut(String),
) -> (crate::pdf::Page, Option<Rect>) {
    let fonts = &mut assets.fonts;
    let layout = sc
        .text
        .as_ref()
        .map(|t| layout(t, fonts))
        .unwrap_or_default();
    let offsets = anchor_offsets(sc, &layout);
    let content = content_rect(sc, &offsets);
    let (pw, h) = match size {
        Size::Grow((w, h)) => page_size(w, h, content, sc.text.as_ref().map(|t| (t, &layout))),
        Size::Fixed((w, h)) => (w, h as f64),
    };
    // The exporter's raster covers the content rect, which may reach past the page; the
    // canvas covers both, its pixel (0, 0) at page pixel `base`.
    let page = (pw as i32, h.ceil() as i32);
    let base = match content {
        Some(rect) => {
            let mut probe = Renderer::new(pw, 0);
            probe.align_to(rect);
            let ((x, y), (w, h)) = probe.image().unwrap();
            (x.min(0), y.min(0), (x + w).max(page.0), (y + h).max(page.1))
        }
        None => (0, 0, page.0, page.1),
    };
    let renderer = || {
        let mut r = Renderer::new((base.2 - base.0) as u32, (base.3 - base.1) as u32);
        r.center = (pw as i32 / 2 - base.0, -base.1);
        r.direct = Some(Vec::new());
        r.offsets = offsets.clone();
        r
    };
    let mut r = renderer();
    let mut groups = Vec::new();
    if let Some(rect) = content {
        r.align_to(rect);
        r.draw_scene(sc);
        let class = has_shading(sc).then(|| {
            let mut p = renderer();
            p.no_shading = true;
            p.align_to(rect);
            p.draw_scene(sc);
            p.canvas
                .px
                .iter()
                .map(|&c| (c == 0) as u8)
                .collect::<Vec<_>>()
        });
        groups = crate::trace::vectorize(
            &r.canvas.px,
            class.as_deref(),
            r.canvas.w as i32,
            r.canvas.h as i32,
            r.image().unwrap(),
        );
        let shift = Pt::new(base.0 * P, base.1 * P);
        for g in &mut groups {
            for sp in &mut g.subpaths {
                for p in sp.iter_mut() {
                    *p += shift;
                }
            }
        }
    }
    for w in &r.warnings {
        warn(w.to_string());
    }
    let strokes = r
        .direct
        .take()
        .unwrap_or_default()
        .iter()
        .map(|l| stroke(pw, l))
        .collect();
    let (mut text, mut shapes, mut rules) = (vec![], vec![], vec![]);
    if let Some(t) = &sc.text {
        text = text_lines(pw, t, &layout, fonts, warn);
        let o = (pw as f32 / 2.0 + t.x as f32, t.y as f32);
        shapes = layout
            .shapes
            .iter()
            .map(|s| s.iter().map(|g| g.offset(o.0 as f64, o.1 as f64)).collect())
            .collect();
        rules = layout
            .rules
            .iter()
            .map(|r| Rule {
                origin: o,
                x0: r.x0,
                x1: r.x1,
                y: r.y as f32,
                width: r.width,
            })
            .collect();
    }
    let tpl = crate::template::load(
        template,
        pw as f64,
        h,
        paper,
        assets.templates.as_deref(),
        &mut assets.fonts,
    );
    if tpl.is_none() && !template.is_empty() && template != "Blank" {
        warn(format!("template {template:?} not found or not supported"));
    }
    let highlights = highlights(pw, sc);
    (
        crate::pdf::Page {
            w: pw,
            h,
            template: tpl,
            text,
            rules,
            shapes,
            highlights,
            strokes,
            groups,
        },
        content,
    )
}

/// Text highlights on a PDF page (glyph ranges): their color and rects in device pixels,
/// filled in `Darken` mode as PdfExporter does when it doesn't write highlight annotations.
fn highlights(pw: u32, sc: &Scene) -> Vec<Highlight> {
    let cx = pw as f64 / 2.0;
    sc.items
        .values()
        .flatten()
        .filter_map(|it| match &it.value {
            Some(ItemValue::Glyph(g)) => Some(Highlight {
                color: g.color.to_argb(g.argb),
                rects: g
                    .rects
                    .iter()
                    .map(|r| ((r.x + cx) as f32, r.y as f32, r.w as f32, r.h as f32))
                    .collect(),
            }),
            _ => None,
        })
        .collect()
}

/// Pass-through stroke for a direct line (`PageVectorizer::rasterVectorize`): raw points,
/// width from the first point's width (quarter units), cap/join per tool.
fn stroke(pw: u32, (l, o): &(Line, (f32, f32))) -> Stroke {
    let (cap, join, darken) = match l.tool {
        Tool::Highlighter => (2, 2, true),
        Tool::HighlighterV2 => (0, 2, true),
        _ => (1, 1, false),
    };
    let (ox, oy) = (pw as f32 / 2.0 + o.0, o.1);
    Stroke {
        color: l.color.to_argb(l.argb),
        width: l.points.first().map_or(0.0, |p| p.width as f32 * 0.25),
        cap,
        join,
        darken,
        points: l.points.iter().map(|p| (p.x + ox, p.y + oy)).collect(),
    }
}

/// The laid-out root text, with the text origin at (`x` from the page center, `y`).
fn text_lines(
    pw: u32,
    t: &RootText,
    layout: &Layout,
    fonts: &mut Fonts,
    warn: &mut dyn FnMut(String),
) -> Vec<TextLine> {
    let origin = (pw as f32 / 2.0 + t.x as f32, t.y as f32);
    let mut missing = Vec::new();
    let lines = layout
        .lines
        .iter()
        .chain(&layout.markers)
        // One text object per run of glyphs in the same font.
        .flat_map(|l| {
            l.glyphs
                .chunk_by(|a, b| a.flags == b.flags)
                .map(move |run| (l, run))
        })
        .filter_map(|(l, run)| {
            let face = l.format.font(run[0].flags);
            let Some(data) = fonts.data(face) else {
                if !missing.contains(&face) {
                    missing.push(face);
                }
                return None;
            };
            Some(TextLine {
                font: face.name(),
                data,
                px: l.format.px,
                origin,
                baseline: to_px(l.baseline),
                glyphs: run
                    .iter()
                    .map(|g| (g.id, to_px(g.x), g.text.clone()))
                    .collect(),
            })
        })
        .collect();
    for f in missing {
        warn(format!("font {} not found; its text is left out", f.file()));
    }
    lines
}

/// Whether a rasterized line uses a shading tool (the shader; highlighters are direct).
fn has_shading(sc: &Scene) -> bool {
    sc.items
        .values()
        .flatten()
        .any(|it| matches!(&it.value, Some(ItemValue::Line(l)) if l.tool == Tool::ShadingMarker))
}

fn page_size(
    w: u32,
    h: u32,
    content: Option<Rect>,
    text: Option<(&RootText, &Layout)>,
) -> (u32, f64) {
    const MARGIN: f64 = 5.0 / 25.4 * DPI;
    let mut rect = content.unwrap_or_default();
    if let Some((t, l)) = text
        && let Some(last) = l.lines.last()
    {
        let bottom = t.y + to_px(last.baseline) as f64 + last.format.px as f64 * 0.3;
        rect.h = rect.h.max(bottom - rect.y);
    }
    let (mut w, h) = (w, h as f64);
    if w < h as u32
        && (rect.x <= -(w as f64) / 2.0 - MARGIN || w as f64 / 2.0 + MARGIN <= rect.right())
    {
        w = h as u32;
    }
    let bottom = rect.bottom();
    if h + MARGIN < bottom {
        (w, bottom + MARGIN)
    } else {
        (w, h)
    }
}
