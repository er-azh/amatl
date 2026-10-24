use super::ballpoint::noise;
use super::raster::{ClipBox, Raster, SpanFill, Var, Vertex, clip_triangle, lerp_triangle};
use super::stroker::{Cap, Join, Stroker, VaryingGen};
use super::{Canvas, Renderer, premultiply};
use crate::format::Line;

/// `VaryingGenerator_ThresholdAndWidth`:
/// (length * k1, side * half_width * k2, threshold, half_width * k2).
pub struct ThresholdAndWidth {
    pub threshold: f32,
    pub k1: f32,
    pub k2: f32,
}

impl VaryingGen for ThresholdAndWidth {
    fn varying(&self, length: f32, half_width: f32, side: f32) -> Var {
        let w = half_width * self.k2;
        Var([
            length * self.k1,
            side * half_width * self.k2,
            self.threshold,
            w,
        ])
    }
}

pub struct Fill<'a> {
    pub canvas: &'a mut Canvas,
    pub color: u32,
    /// Noise lattice scale (`1/s`, s = 1 from makePipeline).
    pub inv: f32,
    /// Noise origin in device pixels.
    pub ox: f32,
    pub oy: f32,
    /// Triangles are clipped to this box before scan conversion.
    pub clip: ClipBox,
}

fn page_noise(inv: f32, x: f32, y: f32) -> u32 {
    let n1 = noise(inv * x, inv * y);
    let n3 = noise(inv * 3.0 * x, inv * 3.0 * y);
    let n7 = noise(inv * 7.0 * x, inv * 7.0 * y);
    (((n1 * 2) / 6 + (n3 * 3) / 6 + n7 / 6) * 7) >> 3
}

impl SpanFill for Fill<'_> {
    fn span(&mut self, x0: i32, x1: i32, y: i32, v: Var, dv: Var) {
        let py = y as f32 - self.oy;
        let mut v = v;
        for x in x0..x1 {
            let mut t = v[2];
            if v[3] > 2.0 {
                let e = v[1].abs() / v[3];
                if e >= 0.5 {
                    let f = if e < 1.0 {
                        let u = (e - 0.5) + (e - 0.5);
                        1.0 - (3.0 - (u + u)) * u * u
                    } else {
                        0.0
                    };
                    t = f * v[2];
                }
            }
            let n = page_noise(self.inv, x as f32 - self.ox, py) + (noise(v[0], v[1]) >> 3);
            v += dv;
            if n as f32 <= t {
                self.canvas.put(x, y, self.color);
            }
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

pub fn draw(r: &mut Renderer, line: &Line) {
    let scale = r.scale;
    let t = r.transform();
    let (ox, oy) = t.map(0.0, 0.0);
    let clip = r.clip_box();
    let mut st = Stroker::new(ThresholdAndWidth {
        threshold: 100.0,
        k1: 0.02,
        k2: 1.5,
    });
    st.join = Join::Round;
    st.cap = Cap::Round;
    st.length = line.starting_length * scale;
    let mut fill = Fill {
        canvas: &mut r.canvas,
        color: premultiply(line.color.to_argb(line.argb)),
        inv: 1.0,
        ox,
        oy,
        clip,
    };
    for p in &line.points {
        st.varying.threshold = p.pressure as f32 / 255.0 * 165.0 + 35.0;
        st.width = p.width as f32 / 4.0 * scale;
        let (x, y) = t.map(p.x, p.y);
        st.line_to(&mut fill, x, y);
        st.start_cap(&mut fill);
    }
    st.finish(&mut fill);
}
