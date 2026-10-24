//! Triangle scan conversion with linearly interpolated varyings (xochitl's strokev2 copy of
//! Quill's `LerpRaster`, including its divide-by-zero guards).

/// The varyings interpolated across a triangle (up to four, meaning per pipeline).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Var(pub [f32; 4]);

impl Var {
    /// `self / d`, or zero when `d == 0`
    fn div0(self, d: f32) -> Var {
        if d == 0.0 {
            Var::default()
        } else {
            Var(self.0.map(|v| v / d))
        }
    }
}

impl std::ops::Add for Var {
    type Output = Var;
    fn add(self, o: Var) -> Var {
        Var(std::array::from_fn(|i| self.0[i] + o.0[i]))
    }
}

impl std::ops::AddAssign for Var {
    fn add_assign(&mut self, o: Var) {
        *self = *self + o;
    }
}

impl std::ops::Sub for Var {
    type Output = Var;
    fn sub(self, o: Var) -> Var {
        Var(std::array::from_fn(|i| self.0[i] - o.0[i]))
    }
}

impl std::ops::Mul<f32> for Var {
    type Output = Var;
    fn mul(self, s: f32) -> Var {
        Var(self.0.map(|v| v * s))
    }
}

impl std::ops::Index<usize> for Var {
    type Output = f32;
    fn index(&self, i: usize) -> &f32 {
        &self.0[i]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub x: f32,
    pub y: f32,
}

/// Receives the horizontal spans of a rasterized triangle.
pub trait SpanFill {
    /// Fill pixels `x0..x1` of row `y`. `v` is the varying at the center of pixel `x0`, `dv` its
    /// per-pixel step.
    fn span(&mut self, x0: i32, x1: i32, y: i32, v: Var, dv: Var);
}

/// The triangle sink the stroker draws into.
pub trait Raster {
    fn triangle(&mut self, a: Vertex, b: Vertex, c: Vertex, va: Var, vb: Var, vc: Var);
}

/// A clip box: x0, x1, y0, y1 (inclusive).
pub type ClipBox = [f32; 4];

#[allow(clippy::too_many_arguments)]
pub fn clip_triangle<O: FnMut(Vertex, Vertex, Vertex, Var, Var, Var)>(
    bx: ClipBox,
    a: Vertex,
    b: Vertex,
    c: Vertex,
    va: Var,
    vb: Var,
    vc: Var,
    out: &mut O,
) {
    let inside = |p: Vertex| bx[0] <= p.x && p.x <= bx[1] && bx[2] <= p.y && p.y <= bx[3];
    if inside(a) && inside(b) && inside(c) {
        return out(a, b, c, va, vb, vc);
    }
    clip_stage(0, &bx, [(a, va), (b, vb), (c, vc)], out);
}

type ClipVertex = (Vertex, Var);

fn clip_stage<O: FnMut(Vertex, Vertex, Vertex, Var, Var, Var)>(
    k: usize,
    bx: &ClipBox,
    t: [ClipVertex; 3],
    out: &mut O,
) {
    if k == 4 {
        let [(a, va), (b, vb), (c, vc)] = t;
        return out(a, b, c, va, vb, vc);
    }
    let bound = bx[k];
    let y = k >= 2;
    let key = |p: &ClipVertex| if y { p.0.y } else { p.0.x };
    let cut = |p: &ClipVertex, q: &ClipVertex| -> ClipVertex {
        let f = (bound - key(p)) / (key(q) - key(p));
        let v = p.1 + (q.1 - p.1) * f;
        let pos = if y {
            Vertex {
                x: p.0.x + (q.0.x - p.0.x) * f,
                y: bound,
            }
        } else {
            Vertex {
                x: bound,
                y: p.0.y + (q.0.y - p.0.y) * f,
            }
        };
        (pos, v)
    };
    let [mut a, mut b, mut c] = t;
    if k.is_multiple_of(2) {
        if key(&b) < key(&a) {
            std::mem::swap(&mut a, &mut b);
        }
        if key(&c) < key(&a) {
            std::mem::swap(&mut a, &mut c);
        }
        if key(&c) < key(&b) {
            std::mem::swap(&mut b, &mut c);
        }
        if bound < key(&a) {
            clip_stage(k + 1, bx, [a, b, c], out);
        } else if bound < key(&b) {
            let (p, q) = (cut(&a, &b), cut(&a, &c));
            clip_stage(k + 1, bx, [p, q, b], out);
            clip_stage(k + 1, bx, [q, b, c], out);
        } else if bound < key(&c) {
            clip_stage(k + 1, bx, [cut(&c, &a), cut(&c, &b), c], out);
        }
    } else if key(&c) < bound {
        clip_stage(k + 1, bx, [a, b, c], out);
    } else if key(&b) < bound {
        let (p, q) = (cut(&c, &a), cut(&c, &b));
        if k == 1 {
            clip_stage(k + 1, bx, [b, p, q], out);
            clip_stage(k + 1, bx, [a, b, p], out);
        } else {
            clip_stage(k + 1, bx, [a, b, p], out);
            clip_stage(k + 1, bx, [b, p, q], out);
        }
    } else if key(&a) < bound {
        clip_stage(k + 1, bx, [a, cut(&a, &c), cut(&a, &b)], out);
    }
}

/// Pixel rows start at the first center at or after `y` (`top`) or end at the last center
/// before it (bottom), with Quill's half-pixel rule.
fn top_row(y: f32) -> f32 {
    let f = y.floor();
    if y - f < 0.5 { f + 0.5 } else { f + 1.5 }
}
fn bottom_row(y: f32) -> f32 {
    let f = y.floor();
    if y - f < 0.5 { f - 0.5 } else { f + 0.5 }
}

pub fn lerp_triangle<F: SpanFill>(
    fill: &mut F,
    mut a: Vertex,
    mut b: Vertex,
    mut c: Vertex,
    mut va: Var,
    mut vb: Var,
    mut vc: Var,
) {
    if b.y < a.y {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut va, &mut vb);
    }
    if c.y < a.y {
        std::mem::swap(&mut a, &mut c);
        std::mem::swap(&mut va, &mut vc);
    }
    if c.y < b.y {
        std::mem::swap(&mut c, &mut b);
        std::mem::swap(&mut vc, &mut vb);
    }
    let mut y = top_row(a.y);
    let yb = bottom_row(b.y);
    let yc = bottom_row(c.y);

    let row = |fill: &mut F, y: f32, left: f32, right: f32, var_left: Var, var_dx: Var| {
        let l = (left + 0.5) as i32;
        let r = (right - 0.5) as i32;
        if l <= r {
            let v = var_left + var_dx * ((l as f32 + 0.5) - left);
            fill.span(l, r + 1, y as i32, v, var_dx);
        }
    };

    if y <= yb {
        let dyab = b.y - a.y;
        let dyac = c.y - a.y;
        let mut dxl = (b.x - a.x) / dyab;
        let mut dxr = (c.x - a.x) / dyac;
        let mut dxld = (vb - va).div0(dyab);
        let mut dxrd = (vc - va).div0(dyac);
        if dxl > dxr {
            std::mem::swap(&mut dxl, &mut dxr);
            std::mem::swap(&mut dxld, &mut dxrd);
        }
        let height = yb - a.y;
        let bx = a.x + height * dxl;
        let acbx = a.x + height * dxr;
        let wb = acbx - bx;
        let var_b = va + dxld * height;
        let var_acb = va + dxrd * height;
        let var_dx = (var_acb - var_b).div0(wb);
        let yoff = y - a.y;
        let mut left = a.x + yoff * dxl;
        let mut right = a.x + yoff * dxr;
        let mut var_left = va + dxld * yoff;
        while y <= yb {
            row(fill, y, left, right, var_left, var_dx);
            left += dxl;
            right += dxr;
            y += 1.0;
            var_left += dxld;
        }
    }

    if y <= yc {
        let dyac = c.y - a.y;
        let dybc = c.y - b.y;
        let mut dxl = (c.x - b.x) / dybc;
        let mut dxr = (c.x - a.x) / dyac;
        let mut dxld = (vc - vb).div0(dybc);
        let mut dxrd = (vc - va).div0(dyac);
        if dxl < dxr {
            std::mem::swap(&mut dxl, &mut dxr);
            std::mem::swap(&mut dxld, &mut dxrd);
        }
        let height = c.y - y;
        let mut left = c.x - height * dxl;
        let right0 = c.x - height * dxr;
        let mut right = right0;
        let mut var_left = vc - dxld * height;
        let var_right = vc - dxrd * height;
        let var_dx = (var_right - var_left).div0(right0 - left);
        while y <= yc {
            row(fill, y, left, right, var_left, var_dx);
            left += dxl;
            right += dxr;
            y += 1.0;
            var_left += dxld;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(v: &[f32]) -> String {
        v.iter()
            .map(|x| format!("{:08x}", x.to_bits()))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Cases of one input line (box x0 x1 y0 y1, three varyings, three vertices; f32 bits in
    /// hex), the triangles the emulated clipper hands on in the same layout, then "END".
    #[test]
    fn clipped_triangles_match_xochitl() {
        let data = include_str!("../../testdata/clip.txt");
        let mut cases = 0;
        let mut lines = data.lines();
        while let Some(input) = lines.next() {
            let v: Vec<f32> = input
                .split(' ')
                .map(|t| f32::from_bits(u32::from_str_radix(t, 16).unwrap()))
                .collect();
            let var = |i: usize| Var([v[4 + 4 * i], v[5 + 4 * i], v[6 + 4 * i], v[7 + 4 * i]]);
            let pt = |i: usize| Vertex {
                x: v[16 + 2 * i],
                y: v[17 + 2 * i],
            };
            let mut got = Vec::new();
            clip_triangle(
                [v[0], v[1], v[2], v[3]],
                pt(0),
                pt(1),
                pt(2),
                var(0),
                var(1),
                var(2),
                &mut |a, b, c, va, vb, vc| {
                    let f: Vec<f32> =
                        va.0.iter()
                            .chain(&vb.0)
                            .chain(&vc.0)
                            .copied()
                            .chain([a.x, a.y, b.x, b.y, c.x, c.y])
                            .collect();
                    got.push(hex(&f));
                },
            );
            let want: Vec<&str> = lines.by_ref().take_while(|&l| l != "END").collect();
            assert_eq!(got, want, "case {cases}: {input}");
            cases += 1;
        }
        assert!(cases > 0);
    }
}
