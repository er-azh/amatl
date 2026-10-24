use super::ballpoint::noise;
use super::pencil::ThresholdAndWidth;
use super::raster::{ClipBox, Raster, SpanFill, Var, Vertex, clip_triangle, lerp_triangle};
use super::stroker::{Cap, Join, Stroker};
use super::{Canvas, Renderer};
use crate::format::{Line, Point, Tool};

struct Fill<'a> {
    canvas: &'a mut Canvas,
    color: u32,
    clip: ClipBox,
}

fn hash(x: i32, y: i32) -> u32 {
    super::ballpoint::lattice(x, y)
}

impl SpanFill for Fill<'_> {
    fn span(&mut self, x0: i32, x1: i32, y: i32, v: Var, dv: Var) {
        let mut v = v;
        for x in x0..x1 {
            let n = ((noise(v[0], v[1]) * 3 + hash(x, y)) >> 2) as f32;
            let e = (v[1] / v[3]).abs();
            let t = if e < 0.5 {
                v[2]
            } else if e <= 1.0 {
                let u = (e - 0.5) + (e - 0.5);
                (1.0 - (3.0 - (u + u)) * u * u) * v[2]
            } else {
                v[2] * 0.0
            };
            v += dv;
            if n <= t {
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

fn smoothstep(t: f32) -> f32 {
    (3.0 - (t + t)) * t * t
}

fn paintbrush(p: &Point, scale: f32) -> (f32, f32) {
    let s = p.speed as f32 * 0.25;
    let w = p.width as f32 * 0.25 * scale;
    let pr = p.pressure as f32 / 255.0;
    if s < 5.0 {
        let t = if pr <= 0.7 {
            smoothstep(pr / 0.7) * 255.0
        } else {
            255.0
        };
        (w, t)
    } else if s > 50.0 {
        let t = if pr <= 0.7 {
            smoothstep(pr / 0.7) * 120.0
        } else {
            120.0
        };
        (w * 0.7, t)
    } else {
        let a = if pr <= 0.7 { smoothstep(pr / 0.7) } else { 1.0 };
        let k = (s - 5.0) / 45.0;
        let w = (1.0 - smoothstep(k) * 0.3) * w;
        let t = if s <= 30.0 {
            (255.0 - smoothstep((s - 5.0) / 25.0) * 135.0) * a
        } else {
            a * 120.0
        };
        (w, t)
    }
}

fn marker(p: &Point, scale: f32) -> (f32, f32) {
    let s = p.speed as f32 * 0.25;
    let t = if s < 3.0 {
        255.0
    } else if s <= 100.0 {
        (1.0 - smoothstep((s - 3.0) / 97.0) * 0.4) * 255.0
    } else {
        153.0
    };
    (p.width as f32 * 0.25 * scale, t)
}

pub fn draw(r: &mut Renderer, line: &Line) {
    let per_point = if line.tool == Tool::MarkerV2 {
        marker
    } else {
        paintbrush
    };
    let pts = &line.points;
    let Some(k) = pts.iter().position(|p| p.x != pts[0].x || p.y != pts[0].y) else {
        return;
    };
    let scale = r.scale;
    let t = r.transform();
    let clip = r.clip_box();
    let mut st = Stroker::new(ThresholdAndWidth {
        threshold: 100.0,
        k1: 0.02 / scale,
        k2: 1.5 / scale,
    });
    st.join = Join::Round;
    st.cap = Cap::Round;
    st.length = line.starting_length * scale;
    let mut fill = Fill {
        canvas: &mut r.canvas,
        color: line.color.to_argb(line.argb),
        clip,
    };
    for p in &pts[k - 1..] {
        (st.width, st.varying.threshold) = per_point(p, scale);
        let (x, y) = t.map(p.x, p.y);
        st.line_to(&mut fill, x, y);
    }
    st.finish(&mut fill);
}
