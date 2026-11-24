//! The annotation stream, as PDFium writes it: points, y up, mapped back into the page's pixel
//! space by its own `cm`. Graphics state follows PDFium's content generator: defaults once, then
//! per object `q`, colors, non-default line parameters, and an ExtGState only when it changes.

use std::fmt::Write as _;

use super::number::{num, rgb};
use super::{Highlight, PT_PER_PX, Page, Picture};
use crate::render::{premultiply, unpremultiply};
use crate::trace::P;

/// An ExtGState: `Darken` or `Normal` blending, stroking alpha (`CA`), non-stroking alpha (`ca`).
#[derive(Clone, Copy, PartialEq)]
pub struct State {
    pub darken: bool,
    pub stroke_alpha: Option<u8>,
    pub fill_alpha: Option<u8>,
}

impl State {
    fn new(darken: bool, stroke_alpha: Option<u8>, fill_alpha: Option<u8>) -> State {
        State {
            darken,
            stroke_alpha,
            fill_alpha,
        }
    }
}

/// Blend states used by the annotation stream, as `/FXEn` names in order of first use.
#[derive(Default)]
pub struct States(pub Vec<State>);

impl States {
    fn name(&mut self, st: State) -> String {
        let i = match self.0.iter().position(|&s| s == st) {
            Some(i) => i,
            None => {
                self.0.push(st);
                self.0.len() - 1
            }
        };
        format!("/FXE{}", i + 1)
    }
}

struct Stream<'a> {
    out: String,
    states: &'a mut States,
    /// ExtGState currently in effect outside the per-object `q`.
    current: String,
    hpt: f32,
}

impl Stream<'_> {
    fn pt(&self, x: f32, y: f32) -> String {
        format!("{} {}", num(x * PT_PER_PX), num(self.hpt - y * PT_PER_PX))
    }

    /// `q`, colors and line state for one object; ends ready for its path.
    fn begin(
        &mut self,
        fill: [f32; 3],
        stroke: [f32; 3],
        width: f32,
        cap: u8,
        join: u8,
        gs: String,
    ) {
        self.out += "q ";
        let _ = write!(self.out, "{} rg {} RG ", rgb(fill), rgb(stroke));
        if width != 1.0 {
            let _ = write!(self.out, "{} w ", num(width));
        }
        if cap != 0 {
            let _ = write!(self.out, "{cap} J ");
        }
        if join != 0 {
            let _ = write!(self.out, "{join} j ");
        }
        if gs != self.current {
            let _ = write!(self.out, "{gs} gs ");
        }
        self.out += "0 0 m ";
    }
}

fn picture(s: &mut Stream, n: usize, p: &Picture) {
    use crate::render::Affine;
    let k = 72.0 / crate::render::DPI;
    let unit = Affine {
        m22: -1.0,
        ..Affine::IDENTITY
    }
    .then(&Affine::translation(-0.5, 0.5));
    let engine = Affine {
        m11: k,
        m22: -k,
        dy: s.hpt as f64,
        ..Affine::IDENTITY
    };
    let m = unit.then(&p.m).then(&engine);
    let m = [m.m11, m.m12, m.m21, m.m22, m.dx, m.dy].map(|v| num(v as f32));
    let _ = writeln!(s.out, "q {} cm /FXX{n} Do Q", m.join(" "));
}

/// The annotation stream of `page` (`hpt` points high), and the names of the ExtGStates it
/// uses (registered in `states`).
pub fn stream(page: &Page, hpt: f32, states: &mut States) -> (String, Vec<String>) {
    let k = 1.0 / PT_PER_PX;
    let normal = states.name(State::new(false, Some(255), Some(255)));
    let mut s = Stream {
        out: String::new(),
        states,
        current: normal.clone(),
        hpt,
    };
    let _ = writeln!(s.out, "q\n{} 0 0 {} 0 {} cm", num(k), num(-k), num(hpt * k));
    let _ = write!(s.out, "0 0 0 RG 0 0 0 rg 1 w 0 J 0 j\n{normal} gs ");
    let mut used = vec![normal.clone()];
    for &Highlight { color, ref rects } in &page.highlights {
        let a = (color >> 24) as u8;
        let gs = s.states.name(State::new(true, None, Some(a)));
        used.push(gs.clone());
        s.begin(
            [16, 8, 0].map(|k| ((color >> k) & 0xff) as f32 / 255.0),
            [0.0; 3],
            1.0,
            0,
            0,
            gs,
        );
        for &(x, y, w, h) in rects {
            for (i, (px, py)) in [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
                .into_iter()
                .enumerate()
            {
                let p = s.pt(px, py);
                let _ = write!(s.out, "{p} {} ", if i == 0 { "m" } else { "l" });
            }
            s.out += "h ";
        }
        s.out += "f Q\n";
    }
    // Image objects and strokes in z order; the images' `FXXn` names count in drawing order.
    let mut pictures = page.pictures.iter().enumerate().peekable();
    let mut next_pictures = |s: &mut Stream, upto: usize| {
        while let Some((i, p)) = pictures.next_if(|(_, p)| p.before <= upto) {
            picture(s, i + 1, p);
        }
    };
    for (k, st) in page.strokes.iter().enumerate() {
        next_pictures(&mut s, k);
        let a = (st.color >> 24) as u8;
        let gs = s.states.name(State::new(st.darken, Some(a), Some(a)));
        used.push(gs.clone());
        let col = unpremultiply(premultiply(st.color));
        s.begin([0.0; 3], col, st.width * PT_PER_PX, st.cap, st.join, gs);
        for (i, &(x, y)) in st.points.iter().enumerate() {
            let p = s.pt(x, y);
            let _ = write!(s.out, "{p} {} ", if i == 0 { "m" } else { "l" });
        }
        s.out += "S Q\n";
    }
    next_pictures(&mut s, usize::MAX);
    for g in &page.groups {
        // Region colors keep their alpha, which PDFium writes as `ca` alone.
        let a = (g.color >> 24) as u8;
        let gs = if g.darken || a != 255 {
            s.states.name(State::new(g.darken, None, Some(a)))
        } else {
            normal.clone()
        };
        used.push(gs.clone());
        s.begin(
            [16, 8, 0].map(|k| ((g.color >> k) & 0xff) as f32 / 255.0),
            [0.0; 3],
            1.0,
            0,
            0,
            gs,
        );
        for sp in &g.subpaths {
            for (i, p) in sp.iter().enumerate() {
                let p = s.pt(p.x as f32 / P as f32, p.y as f32 / P as f32);
                let _ = write!(s.out, "{p} {} ", if i == 0 { "m" } else { "l" });
            }
        }
        s.out += "f* Q\n";
    }
    s.out += "Q\n";
    let out = s.out;
    used.sort();
    used.dedup();
    (out, used)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::pdf::{Bitmap, Stroke};

    fn stroke() -> Stroke {
        Stroke {
            color: 0xff00_0000,
            width: 2.0,
            cap: 1,
            join: 1,
            darken: false,
            points: vec![(0.0, 0.0), (10.0, 10.0)],
        }
    }

    #[test]
    fn images_and_strokes_in_z_order() {
        let bitmap = Arc::new(Bitmap {
            w: 1,
            h: 1,
            px: vec![[0, 0, 0, 255]],
            alpha: true,
        });
        let picture = |before| Picture {
            bitmap: bitmap.clone(),
            m: crate::render::Affine {
                m11: 227.0,
                m22: 227.0,
                dx: 113.5,
                dy: 113.5,
                ..crate::render::Affine::IDENTITY
            },
            before,
        };
        let page = Page {
            w: 1404,
            h: 1872.0,
            template: None,
            text: vec![],
            rules: vec![],
            shapes: vec![],
            highlights: vec![],
            strokes: vec![stroke(), stroke()],
            pictures: vec![picture(0), picture(1), picture(2)],
            groups: vec![],
        };
        let (out, _) = stream(&page, 594.0, &mut States::default());
        let order: Vec<&str> = out
            .lines()
            .filter_map(|l| {
                l.split(' ')
                    .find(|w| w.starts_with("/FXX"))
                    .or(l.ends_with("S Q").then_some("S"))
            })
            .collect();
        assert_eq!(order, ["/FXX1", "S", "/FXX2", "S", "/FXX3"]);
        // A 1 inch square at the top left: 72 pt, its bottom edge 72 pt below the top.
        assert!(out.contains("q 72 0 0 72 0 522 cm /FXX1 Do Q"), "{out}");
    }
}
