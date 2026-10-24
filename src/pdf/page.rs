//! The page stream, as Qt's PDF engine writes it: the `72/227` y-down transform, then the
//! template, the root text, strikeouts and checkboxes in device pixels.

use std::fmt::Write as _;
use std::sync::Arc;

use super::Page;
use super::font::{Font, font_index};
use super::number::{num, qnum};
use super::template::template;
use crate::render::DPI;
use crate::template::Image;

/// The page stream of `page` (`hpt` points high), and the indices of the fonts and images it
/// uses (registered in `fonts` and `images`).
pub fn stream(
    page: &Page,
    hpt: f32,
    fonts: &mut Vec<Font>,
    images: &mut Vec<Arc<Image>>,
) -> (String, Vec<usize>, Vec<usize>) {
    let k = 72.0 / DPI;
    let mut c = format!(
        "/GSa gs /CSp cs /CSp CS\n{} 0 0 -{} 0 {} cm\n",
        qnum(k),
        qnum(k),
        qnum(hpt as f64)
    );
    let (mut page_fonts, mut page_images) = (vec![], vec![]);
    if let Some(t) = &page.template {
        template(&mut c, t, fonts, &mut page_fonts, images, &mut page_images);
    }
    for l in &page.text {
        let fi = font_index(fonts, l.font, &l.data);
        page_fonts.push(fi);
        let f = &mut fonts[fi];
        let _ = write!(
            c,
            "q\n1 0 0 1 {} {} cm\n0 0 0 rg\nBT\n/F{fi} {} Tf 1 0 0 -1 0 0 Tm\n",
            num(l.origin.0),
            num(l.origin.1),
            l.px
        );
        let mut x = 0.0;
        for (i, (g, gx, t)) in l.glyphs.iter().enumerate() {
            let cid = f.cid(*g, t);
            let y = if i == 0 { num(-l.baseline) } else { "0".into() };
            let _ = writeln!(c, "{} {y} Td <{cid:04X}> Tj", num(gx - x));
            x = *gx;
        }
        c += "ET\nQ\n";
    }
    for r in &page.rules {
        let (ox, oy) = r.origin;
        let (x0, x1, y, w) = (r.x0, r.x1, r.y, r.width);
        let _ = write!(
            c,
            "q\n1 0 0 1 {} {} cm\n0 0 0 RG\n{} w 0 J 2 j\n",
            num(ox),
            num(oy),
            num(w)
        );
        let _ = write!(
            c,
            "{} {} m\n{} {} l\nS\nQ\n",
            num(x0),
            num(y),
            num(x1),
            num(y)
        );
    }
    for sh in &page.shapes {
        use crate::template::Seg;
        c += "q\n0 0 0 rg\n";
        for seg in sh {
            let n = |v: f64| num(v as f32);
            let _ = match *seg {
                Seg::M(x, y) => writeln!(c, "{} {} m", n(x), n(y)),
                Seg::L(x, y) => writeln!(c, "{} {} l", n(x), n(y)),
                Seg::C(x1, y1, x2, y2, x, y) => writeln!(
                    c,
                    "{} {} {} {} {} {} c",
                    n(x1),
                    n(y1),
                    n(x2),
                    n(y2),
                    n(x),
                    n(y)
                ),
                Seg::Z => writeln!(c, "h"),
            };
        }
        c += "f*\nQ\n";
    }
    page_fonts.sort();
    page_fonts.dedup();
    page_images.sort();
    page_images.dedup();
    (c, page_fonts, page_images)
}
