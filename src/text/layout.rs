//! Paragraphs, line breaking and block layout.

use std::collections::HashMap;

use rustybuzz::ttf_parser::GlyphId;

use super::fixed::{Fixed, ceil, from_real, round, to_px};
use super::fonts::Fonts;
use super::shape::{Metrics, ShapedGlyph, cluster_text, metrics, shape, shape_runs};
use super::style::{BOLD, Format, ITALIC, format};
use crate::format::crdt::toposort;
use crate::format::{Block, List};
use crate::format::{CrdtId, END_MARKER, Item, RootText, TextValue};
use crate::template::Seg;

const MARGIN: Fixed = 4 * 64;

#[derive(Debug, Clone)]
pub struct Glyph {
    pub id: u16,
    /// Origin relative to the text origin.
    pub x: Fixed,
    /// The characters of the cluster this glyph starts (empty for its other glyphs).
    pub text: String,
    /// Inline character format (`BOLD`, `ITALIC`): the font is `format.font(flags)`.
    pub flags: u8,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub block: Block,
    pub format: Format,
    /// Baseline relative to the text origin.
    pub baseline: Fixed,
    pub glyphs: Vec<Glyph>,
    /// Whether this is the first line of its paragraph (where list markers go).
    pub first: bool,
}

#[derive(Debug, Default)]
pub struct Layout {
    pub lines: Vec<Line>,
    /// Line index of each character id (newlines belong to the line they end).
    pub char_line: HashMap<CrdtId, usize>,
    last_char_line: Option<usize>,
    /// List markers drawn as text (bullets, numbers), one line each.
    pub markers: Vec<Line>,
    /// Strikeout lines of checked to-do paragraphs.
    pub rules: Vec<Rule>,
    /// Checkboxes: even-odd filled paths relative to the text origin, in px.
    pub shapes: Vec<Vec<Seg>>,
}

/// A horizontal line stroked with a flat cap, relative to the text origin in px.
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub x0: f32,
    pub x1: f32,
    pub y: f64,
    pub width: f32,
}

fn checkbox(text_x: f32, top: f32, checked: bool) -> Vec<Seg> {
    let (l, t) = (text_x as f64 - 44.0, top as f64 + 2.0);
    let (a, b) = (8.0 / 3.0, 88.0 / 3.0);
    let mut segs = vec![
        Seg::M(a, a),
        Seg::L(b, a),
        Seg::L(b, b),
        Seg::L(a, b),
        Seg::Z,
    ];
    // originally drawn via an SVG
    if checked {
        segs.extend([
            Seg::M(24.01775, 12.25),
            Seg::L(13.5, 22.76774),
            Seg::L(7.982249, 17.25),
            Seg::L(9.75, 15.48225),
            Seg::L(11.732249, 17.46441),
            Seg::C(12.7085, 18.44074, 14.291416, 18.44074, 15.26775, 17.46441),
            Seg::L(22.25, 10.48225),
            Seg::L(24.01775, 12.25),
            Seg::Z,
        ]);
    } else {
        let (i0, i1, r) = (31.0 / 6.0, 161.0 / 6.0, 8.75);
        let k = r * 0.5522847498;
        segs.extend([
            Seg::M(i0, i0),
            Seg::L(i0, i1 - r),
            Seg::C(i0, i1 - r + k, i0 + r - k, i1, i0 + r, i1),
            Seg::L(i1, i1),
            Seg::L(i1, i0),
            Seg::L(i0, i0),
            Seg::Z,
        ]);
    }
    segs.into_iter().map(|s| s.offset(l, t)).collect()
}

fn marker(
    face: &rustybuzz::Face,
    f: Format,
    m: Metrics,
    top: Fixed,
    x: Fixed,
    right: bool,
    text: &str,
) -> Line {
    let chars: Vec<char> = text.chars().collect();
    let (adv, glyphs) = shape(face, f.px, &chars);
    let x0 = if right {
        round(x - 93 - bounding_width(face, f.px, &adv, &glyphs))
    } else {
        x
    };
    let mut char_x = vec![0; chars.len()];
    let mut cx = x0;
    for (i, a) in adv.iter().enumerate() {
        char_x[i] = cx;
        cx += a;
    }
    Line {
        block: Block::Body,
        format: f,
        baseline: top + round(m.ascent),
        glyphs: glyphs
            .iter()
            .map(|g| Glyph {
                id: g.id,
                x: char_x[g.cluster] + g.dx,
                text: cluster_text(&chars, &glyphs, g),
                flags: 0,
            })
            .collect(),
        first: true,
    }
}

fn bounding_width(face: &rustybuzz::Face, px: u32, adv: &[Fixed], glyphs: &[ShapedGlyph]) -> Fixed {
    let k = px as f64 / face.units_per_em() as f64;
    let char_x: Vec<Fixed> = adv
        .iter()
        .scan(0, |x, a| Some(std::mem::replace(x, *x + a)))
        .collect();
    let (mut lo, mut hi) = (Fixed::MAX, Fixed::MIN);
    for g in glyphs {
        if let Some(b) = face.glyph_bounding_box(GlyphId(g.id)) {
            let x = char_x[g.cluster] + g.dx;
            lo = lo.min(x + (b.x_min as f64 * k).floor() as Fixed * 64);
            hi = hi.max(x + (b.x_max as f64 * k).ceil() as Fixed * 64);
        }
    }
    if lo > hi { 0 } else { hi - lo }
}

struct Paragraph {
    block: Block,
    /// Id, character and inline format flags.
    chars: Vec<(CrdtId, char, u8)>,
}

/// The visible characters in document order, split into paragraphs at newlines. A paragraph's
/// style is keyed by the id of the newline before it (`END_MARKER` for the first).
///
/// An item of n characters stands for n consecutive ids, and `left`/`right` name character ids,
/// so the sequence is ordered per character (deleted runs included, as they anchor others).
fn paragraphs(text: &RootText) -> Vec<Paragraph> {
    let mut chars = vec![];
    let mut value = HashMap::new();
    let mut codes = HashMap::new();
    for it in &text.items {
        let s: Vec<char> = match &it.value {
            Some(TextValue::Str(s)) => s.chars().collect(),
            Some(TextValue::Format(c)) => {
                codes.insert(it.id, *c);
                vec!['\0']
            }
            None => vec!['\0'; it.deleted_length.max(1) as usize],
        };
        let n = s.len() as u64;
        for (i, c) in s.into_iter().enumerate() {
            let i = i as u64;
            let id = CrdtId(it.id.0, it.id.1 + i);
            chars.push(Item {
                id,
                left: if i == 0 {
                    it.left
                } else {
                    CrdtId(id.0, id.1 - 1)
                },
                right: if i + 1 == n {
                    it.right
                } else {
                    CrdtId(id.0, id.1 + 1)
                },
                deleted_length: 0,
                value: None::<()>,
            });
            if c != '\0' {
                value.insert(id, c);
            }
        }
    }
    let style_of = |k: CrdtId| text.styles.get(&k).map_or(Block::Body, |s| s.value.block());
    let mut out = vec![Paragraph {
        block: style_of(END_MARKER),
        chars: vec![],
    }];
    let mut flags = 0;
    for id in toposort(&chars) {
        match codes.get(&id) {
            Some(1) => flags |= BOLD,
            Some(2) => flags &= !BOLD,
            Some(3) => flags |= ITALIC,
            Some(4) => flags &= !ITALIC,
            _ => {}
        }
        let Some(&c) = value.get(&id) else { continue };
        out.last_mut().unwrap().chars.push((id, c, flags));
        if c == '\n' {
            out.push(Paragraph {
                block: style_of(id),
                chars: vec![],
            });
        }
    }
    out
}

fn break_lines(chars: &[char], adv: &[Fixed], width: Fixed) -> Vec<std::ops::Range<usize>> {
    let mut lines = vec![];
    let mut start = 0;
    while start < chars.len() {
        let (mut w, mut i, mut brk, mut end) = (0, start, None, chars.len());
        while i < chars.len() {
            if !matches!(chars[i], ' ' | '\n') {
                if w + adv[i] > width && i > start {
                    end = brk.unwrap_or(i);
                    break;
                }
                w += adv[i];
                i += 1;
                continue;
            }
            w += adv[i];
            i += 1;
            brk = Some(i);
        }
        lines.push(start..end);
        start = end;
    }
    if lines.is_empty() {
        lines.push(0..0);
    }
    lines
}

pub fn layout(text: &RootText, fonts: &mut Fonts) -> Layout {
    let mut out = Layout::default();
    let mut y = MARGIN;
    // Numbered list counters of the outer and the indented level.
    let mut number = [0; 2];
    for (pi, p) in paragraphs(text).into_iter().enumerate() {
        let f = format(p.block);
        let Some(data) = fonts.data(f.face) else {
            continue;
        };
        let Some(face) = rustybuzz::Face::from_slice(&data, 0) else {
            continue;
        };
        let m = metrics(&face, f.px);
        if pi > 0 {
            y += f.top_margin;
        }
        let spacing = from_real((m.ascent + m.descent) as f64 / 64.0 * f.line_spacing);
        let chars: Vec<char> = p.chars.iter().map(|c| c.1).collect();
        let mut flags: Vec<u8> = p.chars.iter().map(|c| c.2).collect();
        let (adv, glyphs, run_metrics) = shape_runs(fonts, f, &chars, &mut flags);
        let width = from_real(text.width as f64) - 2 * MARGIN - f.indent;
        let x0 = MARGIN + f.indent;
        match p.block {
            Block::List {
                kind: List::Numbered,
                indented,
            } => {
                let l = indented as usize;
                number[l] += 1;
                if l == 0 {
                    number[1] = 0;
                }
            }
            _ => number = [0; 2],
        }
        let top = y;
        if let Block::List { kind, indented } = p.block {
            match kind {
                // 5.5 drew "-" for indented bullets.
                List::Bullet => out.markers.push(marker(
                    &face,
                    f,
                    m,
                    top,
                    x0 - 28 * 64,
                    false,
                    if indented { "\u{25e6}" } else { "\u{2022}" },
                )),
                List::Numbered => out.markers.push(marker(
                    &face,
                    f,
                    m,
                    top,
                    x0 - 17 * 64,
                    true,
                    &format!("{}.", number[indented as usize]),
                )),
                List::Checkbox => out.shapes.push(checkbox(to_px(x0), to_px(top), false)),
                List::Checked => out.shapes.push(checkbox(to_px(x0), to_px(top), true)),
            }
        }
        let ranges = break_lines(&chars, &adv, width);
        let n_lines = ranges.len();
        for (li, r) in ranges.into_iter().enumerate() {
            let mut end = r.end;
            while end > r.start
                && matches!(chars[end - 1], ' ' | '\n')
                && (li + 1 < n_lines || chars[end - 1] == '\n')
            {
                end -= 1;
            }
            let shown = r.start..end;
            let lm = r
                .clone()
                .filter_map(|i| run_metrics.get(&flags[i]))
                .fold(None, |acc: Option<Metrics>, m| {
                    Some(acc.map_or(*m, |a| Metrics {
                        ascent: a.ascent.max(m.ascent),
                        descent: a.descent.max(m.descent),
                    }))
                })
                .unwrap_or(m);
            let mut x = x0;
            let mut char_x = vec![0; chars.len()];
            for i in r.clone() {
                char_x[i] = x;
                x += adv[i];
            }
            let line = Line {
                block: p.block,
                format: f,
                baseline: y + lm.ascent,
                glyphs: glyphs
                    .iter()
                    .filter(|g| shown.contains(&g.cluster))
                    .map(|g| Glyph {
                        id: g.id,
                        x: char_x[g.cluster] + g.dx,
                        text: cluster_text(&chars, &glyphs, g),
                        flags: flags[g.cluster],
                    })
                    .collect(),
                first: li == 0,
            };
            for i in r.clone() {
                out.char_line.insert(p.chars[i].0, out.lines.len());
                out.last_char_line = Some(out.lines.len());
            }
            if matches!(
                p.block,
                Block::List {
                    kind: List::Checked,
                    ..
                }
            ) {
                let w: Fixed = adv[shown.clone()].iter().sum();
                let upm = face.units_per_em() as f64;
                let thick = from_real(
                    face.underline_metrics().map_or(0.0, |u| u.thickness as f64) * f.px as f64
                        / upm,
                );
                let x0 = to_px(x0).floor();
                out.rules.push(Rule {
                    x0,
                    x1: (to_px(char_x[r.start]) + to_px(w)).floor(),
                    y: to_px(line.baseline) as f64 - to_px(m.ascent) as f64 / 3.0,
                    width: to_px(thick),
                });
            }
            out.lines.push(line);
            y += ceil(lm.ascent + lm.descent) + spacing;
        }
    }
    out
}

impl Layout {
    /// Baseline (relative to the text origin) an anchor id refers to: the first or last line for
    /// the special ids, else the line of that character.
    pub fn anchor_baseline(&self, id: CrdtId) -> Option<Fixed> {
        let line = match id {
            CrdtId(0, 0xfffffffffffe) => self.lines.first(),
            CrdtId(0, 0xffffffffffff) => self
                .last_char_line
                .map_or(self.lines.last(), |i| self.lines.get(i)),
            _ => self.lines.get(*self.char_line.get(&id)?),
        };
        line.map(|l| l.baseline)
    }
}
