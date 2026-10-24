//! Solid pens (fineliner 4/17, calligraphy 21, tools 19/20, highlighters 5/18 when not exported
//! as vectors): `Pipeline<Quill, VaryingNoop>` with `Solid::fill32`, a plain color stamp.

use super::raster::{ClipBox, Raster, SpanFill, Var, Vertex, clip_triangle, lerp_triangle};
use super::stroker::{Cap, Join, NoVarying, Stroker};
use super::{Canvas, Renderer, premultiply};
use crate::format::{Line, Tool};

pub struct Fill<'a> {
    pub canvas: &'a mut Canvas,
    pub color: u32,
    /// Triangles are clipped to this box before scan conversion.
    pub clip: ClipBox,
}

impl SpanFill for Fill<'_> {
    fn span(&mut self, x0: i32, x1: i32, y: i32, _: Var, _: Var) {
        for x in x0..x1 {
            self.canvas.put(x, y, self.color);
        }
    }
}

impl Raster for Fill<'_> {
    fn triangle(&mut self, a: Vertex, b: Vertex, c: Vertex, va: Var, vb: Var, vc: Var) {
        clip_triangle(
            self.clip,
            a,
            b,
            c,
            va,
            vb,
            vc,
            &mut |a, b, c, va, vb, vc| lerp_triangle(self, a, b, c, va, vb, vc),
        );
    }
}

fn style(tool: Tool) -> (Join, Cap) {
    match tool {
        Tool::Highlighter => (Join::Bevel, Cap::Square),
        Tool::HighlighterV2 => (Join::Bevel, Cap::Flat),
        _ => (Join::Round, Cap::Round),
    }
}

pub fn draw(r: &mut Renderer, line: &Line) {
    let scale = r.scale;
    let t = r.transform();
    let clip = r.clip_box();
    let mut st = Stroker::new(NoVarying);
    (st.join, st.cap) = style(line.tool);
    st.length = line.starting_length * scale;
    let mut fill = Fill {
        canvas: &mut r.canvas,
        color: premultiply(line.color.to_argb(line.argb)),
        clip,
    };
    for p in &line.points {
        st.width = p.width as f32 / 4.0 * scale;
        let (x, y) = t.map(p.x, p.y);
        st.line_to(&mut fill, x, y);
        st.start_cap(&mut fill);
    }
    st.finish(&mut fill);
}
