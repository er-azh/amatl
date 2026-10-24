//! Page templates in the page stream, as Qt's PDF engine writes them.

use std::fmt::Write as _;
use std::sync::Arc;

use super::font::{Font, font_index};
use super::number::{qnum, qrgb};
use crate::template::Image;

/// `a` then `b`, as `QTransform` `a * b`.
fn concat(a: &[f64; 6], b: &[f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn map(m: &[f64; 6], x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

/// Template draws in the page stream as Qt's PDF engine (`QPdfEngine`) writes them: each new
/// transform closes and reopens the `q` block, writing the `cm` (for solid pens; other pens get
/// their coordinates mapped instead) and then brush and pen; otherwise only a changed pen or
/// brush is written. Text goes in its own `q` with the pen's color as fill.
pub fn template(
    c: &mut String,
    draws: &[crate::template::Draw],
    fonts: &mut Vec<Font>,
    page_fonts: &mut Vec<usize>,
    images: &mut Vec<Arc<Image>>,
    page_images: &mut Vec<usize>,
) {
    use crate::template::{Kind, Pen, Seg};
    const ID: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let brush = |c: &mut String, f: u32| {
        let _ = write!(c, "/CSp cs\n{} scn\n/GSa gs\n", qrgb(f));
    };
    let pen = |c: &mut String, p: &Pen| {
        let dash = p.dash.iter().fold(String::new(), |mut s, d| {
            let _ = write!(s, "{} ", qnum(*d));
            s
        });
        let _ = write!(
            c,
            "/CSp CS\n{} SCN\n{} w {} J {} j [{dash}]0  d\n",
            qrgb(p.color),
            qnum(p.width),
            p.cap,
            p.join
        );
    };
    // The painter's setup, ending in an open block the first draw restarts.
    c.push_str("q q\nQ\nQ q\nq\nQ\nq\n");
    let (mut cur_pen, mut cur_fill, mut simple): (Option<&Pen>, Option<u32>, bool) =
        (None, None, true);
    for d in draws {
        let s = d.pen.as_ref().is_some_and(|p| p.dash.is_empty());
        let fill = match &d.kind {
            Kind::Path { fill, .. } => *fill,
            Kind::Text(_) | Kind::Image { .. } => cur_fill,
            Kind::Flush => None,
        };
        if d.block || s != simple {
            simple = s;
            c.push_str("Q\nq\n");
            if simple && d.m != ID {
                let m = d.m.map(qnum);
                let _ = writeln!(
                    c,
                    "{} {} {} {} {} {} cm",
                    m[0], m[1], m[2], m[3], m[4], m[5]
                );
            }
            if let Some(f) = fill {
                brush(c, f);
            }
            if let Some(p) = &d.pen {
                pen(c, p);
            }
        } else {
            if fill != cur_fill
                && let Some(f) = fill
            {
                brush(c, f);
            }
            let flush = matches!(d.kind, Kind::Flush);
            if let Some(p) = d.pen.as_ref().filter(|&p| flush || Some(p) != cur_pen) {
                pen(c, p);
            }
        }
        cur_pen = d.pen.as_ref();
        cur_fill = fill;
        let m = if simple { ID } else { d.m };
        match &d.kind {
            Kind::Path { fill, segs } => {
                let (mut start, mut cur) = ((0.0, 0.0), (0.0, 0.0));
                for sg in segs {
                    match *sg {
                        Seg::M(x, y) => {
                            let (px, py) = map(&m, x, y);
                            let _ = writeln!(c, "{} {} m", qnum(px), qnum(py));
                            (start, cur) = ((x, y), (x, y));
                        }
                        Seg::L(x, y) => {
                            let (px, py) = map(&m, x, y);
                            let _ = writeln!(c, "{} {} l", qnum(px), qnum(py));
                            cur = (x, y);
                        }
                        Seg::C(x1, y1, x2, y2, x, y) => {
                            let p: Vec<String> = [(x1, y1), (x2, y2), (x, y)]
                                .iter()
                                .flat_map(|&(x, y)| {
                                    let (px, py) = map(&m, x, y);
                                    [qnum(px), qnum(py)]
                                })
                                .collect();
                            let _ = writeln!(c, "{} c", p.join(" "));
                            cur = (x, y);
                        }
                        Seg::Z => {
                            // QPainterPath::closeSubpath adds the line back to the start.
                            if cur != start {
                                let (px, py) = map(&m, start.0, start.1);
                                let _ = writeln!(c, "{} {} l", qnum(px), qnum(py));
                                cur = start;
                            }
                            c.push_str("h\n");
                        }
                    }
                }
                c.push_str(match (d.pen.is_some(), fill.is_some()) {
                    (true, true) => "B*\n",
                    (true, false) => "S\n",
                    _ => "f*\n",
                });
            }
            Kind::Text(t) => {
                let fi = font_index(fonts, t.font, &t.data);
                page_fonts.push(fi);
                let f = &mut fonts[fi];
                let _ = write!(
                    c,
                    "q\n/CSp cs\n{} scn\n/GSa gs\nBT\n/F{fi} {} Tf 1 0 0 -1 0 0 Tm\n",
                    qrgb(t.color),
                    t.px
                );
                let mut x = 0.0;
                for (i, (g, gx, txt)) in t.glyphs.iter().enumerate() {
                    let cid = f.cid(*g, txt);
                    let (dx, dy) = if i == 0 {
                        (t.x + gx, -t.y)
                    } else {
                        (gx - x, 0.0)
                    };
                    let _ = writeln!(c, "{} {} Td <{cid:04X}> Tj", qnum(dx), qnum(dy));
                    x = *gx;
                }
                c.push_str("ET\nQ\n");
            }
            Kind::Image { image, rect } => {
                // QPdfEngine::drawImage: the rect's scale and offset, mapped unless the block's
                // `cm` already holds the transform, then the unit image.
                let i = images
                    .iter()
                    .position(|x| Arc::ptr_eq(x, image))
                    .unwrap_or_else(|| {
                        images.push(image.clone());
                        images.len() - 1
                    });
                page_images.push(i);
                let r = [
                    rect[2] / image.w as f64,
                    0.0,
                    0.0,
                    rect[3] / image.h as f64,
                    rect[0],
                    rect[1],
                ];
                let t = if simple { r } else { concat(&r, &d.m) };
                let t = t.map(qnum);
                let _ = write!(
                    c,
                    "q\n/GSa gs\n{} {} {} {} {} {} cm\n",
                    t[0], t[1], t[2], t[3], t[4], t[5]
                );
                let _ = write!(
                    c,
                    "{} 0 0 -{} 0 {} cm /Im{i}  Do\nQ\n",
                    image.w, image.h, image.h
                );
            }
            Kind::Flush => {}
        }
    }
    c.push_str("Q Q\n");
}
