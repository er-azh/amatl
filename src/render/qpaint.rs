//! Drawing an image item into the exporter's raster passes, following the path Qt's raster paint
//! engine takes for `QPainter::drawImage(QRectF, QImage, QRectF)`: the generic texture path,
//! antialiased edges, nearest sampling. Premultiplying and blending use plain rounded division
//! rather than the tablet's SIMD rounding; on the device exports checked the result is the same.

use crate::render::color::{div255, premultiply_round};

use super::Canvas;

/// `QTransform` restricted to affine matrices: points are row vectors,
/// `(x, y) -> (m11 x + m21 y + dx, m12 x + m22 y + dy)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Affine {
    pub m11: f64,
    pub m12: f64,
    pub m21: f64,
    pub m22: f64,
    pub dx: f64,
    pub dy: f64,
}

/// `QTransform::TransformationType`, without projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    None,
    Translate,
    Scale,
    Rotate,
    Shear,
}

fn fuzzy_null(d: f64) -> bool {
    d.abs() <= 0.000000000001
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        m11: 1.0,
        m12: 0.0,
        m21: 0.0,
        m22: 1.0,
        dx: 0.0,
        dy: 0.0,
    };

    pub fn translation(dx: f64, dy: f64) -> Affine {
        Affine {
            dx,
            dy,
            ..Affine::IDENTITY
        }
    }

    /// `QTransform::type()`, computed from the values as for a matrix marked dirty.
    pub fn kind(&self) -> Kind {
        if !fuzzy_null(self.m12) || !fuzzy_null(self.m21) {
            let dot = self.m11 * self.m21 + self.m12 * self.m22;
            return if fuzzy_null(dot) {
                Kind::Rotate
            } else {
                Kind::Shear
            };
        }
        if !fuzzy_null(self.m11 - 1.0) || !fuzzy_null(self.m22 - 1.0) {
            Kind::Scale
        } else if !fuzzy_null(self.dx) || !fuzzy_null(self.dy) {
            Kind::Translate
        } else {
            Kind::None
        }
    }

    /// `self * o`: `self` applied first (`QTransform::operator*`, per the larger type).
    pub fn then(&self, o: &Affine) -> Affine {
        let (tk, ok) = (self.kind(), o.kind());
        if ok == Kind::None {
            return *self;
        }
        if tk == Kind::None {
            return *o;
        }
        let mut t = Affine::IDENTITY;
        match tk.max(ok) {
            Kind::None => {}
            Kind::Translate => {
                t.dx = self.dx + o.dx;
                t.dy = self.dy + o.dy;
            }
            Kind::Scale => {
                t.m11 = self.m11 * o.m11;
                t.m22 = self.m22 * o.m22;
                t.dx = self.dx * o.m11 + o.dx;
                t.dy = self.dy * o.m22 + o.dy;
            }
            Kind::Rotate | Kind::Shear => {
                t.m11 = self.m11 * o.m11 + self.m12 * o.m21;
                t.m12 = self.m11 * o.m12 + self.m12 * o.m22;
                t.m21 = self.m21 * o.m11 + self.m22 * o.m21;
                t.m22 = self.m21 * o.m12 + self.m22 * o.m22;
                t.dx = self.dx * o.m11 + self.dy * o.m21 + o.dx;
                t.dy = self.dx * o.m12 + self.dy * o.m22 + o.dy;
            }
        }
        t
    }

    /// `QTransform::translate`: the translation applies before the matrix.
    pub fn translate(&mut self, dx: f64, dy: f64) {
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        match self.kind() {
            Kind::None => {
                self.dx = dx;
                self.dy = dy;
            }
            Kind::Translate => {
                self.dx += dx;
                self.dy += dy;
            }
            Kind::Scale => {
                self.dx += dx * self.m11;
                self.dy += dy * self.m22;
            }
            Kind::Rotate | Kind::Shear => {
                self.dx += dx * self.m11 + dy * self.m21;
                self.dy += dy * self.m22 + dx * self.m12;
            }
        }
    }

    /// `QTransform::scale`: the scale applies before the matrix.
    pub fn scale(&mut self, sx: f64, sy: f64) {
        if sx == 1.0 && sy == 1.0 {
            return;
        }
        match self.kind() {
            Kind::None | Kind::Translate => {
                self.m11 = sx;
                self.m22 = sy;
            }
            Kind::Rotate | Kind::Shear => {
                self.m12 *= sx;
                self.m21 *= sy;
                self.m11 *= sx;
                self.m22 *= sy;
            }
            Kind::Scale => {
                self.m11 *= sx;
                self.m22 *= sy;
            }
        }
    }

    pub fn map(&self, x: f64, y: f64) -> (f64, f64) {
        match self.kind() {
            Kind::None => (x, y),
            Kind::Translate => (x + self.dx, y + self.dy),
            Kind::Scale => (self.m11 * x + self.dx, self.m22 * y + self.dy),
            _ => (
                self.m11 * x + self.m21 * y + self.dx,
                self.m12 * x + self.m22 * y + self.dy,
            ),
        }
    }

    /// `QTransform::inverted` (identity when singular).
    fn inverted(&self) -> Affine {
        match self.kind() {
            Kind::None => Affine::IDENTITY,
            Kind::Translate => Affine::translation(-self.dx, -self.dy),
            Kind::Scale => {
                if fuzzy_null(self.m11) || fuzzy_null(self.m22) {
                    return Affine::IDENTITY;
                }
                let (m11, m22) = (1.0 / self.m11, 1.0 / self.m22);
                Affine {
                    m11,
                    m22,
                    dx: -self.dx * m11,
                    dy: -self.dy * m22,
                    ..Affine::IDENTITY
                }
            }
            _ => {
                // adjoint() / determinant(), the affine terms of Qt's general case.
                let det = self.m11 * self.m22 - self.m12 * self.m21;
                if fuzzy_null(det) {
                    return Affine::IDENTITY;
                }
                let (h11, h12) = (self.m22, -self.m12);
                let (h21, h22) = (-self.m21, self.m11);
                let h31 = self.m21 * self.dy - self.dx * self.m22;
                let h32 = -(self.m11 * self.dy - self.dx * self.m12);
                Affine {
                    m11: h11 / det,
                    m12: h12 / det,
                    m21: h21 / det,
                    m22: h22 / det,
                    dx: h31 / det,
                    dy: h32 / det,
                }
            }
        }
    }
}

/// A straight-alpha image as Qt holds an ARGB32 one: `0xAARRGGBB` per pixel, row by row.
pub(crate) struct Texture<'a> {
    pub w: i32,
    pub h: i32,
    pub px: &'a [u32],
}

const FIXED: i32 = 1 << 16;
const BUFFER_SIZE: usize = 2048;

/// Premultiplied source-over of `src` at `coverage` (0-255).
fn source_over(dst: &mut u32, src: u32, coverage: u32) {
    let ch = |v: u32, s: u32| (v >> s) & 0xff;
    let ia = 255 - div255((src >> 24) * coverage);
    let mut out = 0;
    for s in [0, 8, 16, 24] {
        out |= (div255(ch(src, s) * coverage) + div255(ch(*dst, s) * ia)).min(255) << s;
    }
    *dst = out;
}

/// The texture's inverse mapping as `QSpanData::setupMatrix` keeps it.
fn span_inverse(m: &Affine) -> Affine {
    Affine::translation(1.0 / 65536.0, 1.0 / 65536.0)
        .then(m)
        .inverted()
}

/// `fetchTransformed<BlendTransformed, BPP32>` for a scaled (not rotated) texture: the nearest
/// texel of each pixel of row `y` from `x`, premultiplied. Positions are 16.16 fixed point, in
/// `i64` so that no texture size or scale overflows them.
fn fetch(out: &mut [u32], tex: &Texture, inv: &Affine, x: i32, y: i32) {
    let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
    let fixed = |v: f64| (v * FIXED as f64) as i64;
    let fdx = fixed(inv.m11);
    let mut fx = fixed(inv.m21 * cy + inv.m11 * cx + inv.dx);
    let fy = fixed(inv.m22 * cy + inv.m12 * cx + inv.dy);
    let (w, h) = (tex.w as i64, tex.h as i64);
    let py = (fy >> 16).clamp(0, h - 1) as usize;
    let row = &tex.px[py * w as usize..][..w as usize];
    for o in out.iter_mut() {
        *o = premultiply_round(row[(fx >> 16).clamp(0, w - 1) as usize]);
        fx += fdx;
    }
}

/// One antialiased span: `len` pixels from `x` on row `y`, coverage 0-255.
struct Span {
    x: i32,
    len: i32,
    y: i32,
    coverage: u32,
}

/// `QRasterizer::rasterizeLine` for a line whose ends share a y (in 26.6) and the given width
/// relative to its length: the rectangle it sweeps, as antialiased spans within `clip`
/// (x0, y0, x1, y1 exclusive).
fn rasterize_horizontal(
    a: (f64, f64),
    b: (f64, f64),
    width: f64,
    clip: (i32, i32, i32, i32),
) -> Vec<Span> {
    let mut spans = Vec::new();
    let same = |p1: f64, p2: f64| ((p2 - p1) * 64.0) as i32 == 0;
    if a == b || width.is_nan() || width <= 0.0 {
        return spans;
    }
    let (left_c, top_c, right_c, bottom_c) = (clip.0, clip.1, clip.2 - 1, clip.3 - 1);
    let (mut pa, mut pb) = (a, b);
    let offs = (
        (b.1 - a.1).abs() * width * 0.5,
        (b.0 - a.0).abs() * width * 0.5,
    );
    let cl = (
        left_c as f64 - offs.0,
        top_c as f64 - offs.1,
        (right_c + 1) as f64 + offs.0,
        (bottom_c + 1) as f64 + offs.1,
    );
    if clip_line(&mut pa, &mut pb, cl) {
        return spans;
    }
    let mut width = width;
    {
        let d0 = (a.0 - b.0, a.1 - b.1);
        let w0 = d0.0 * d0.0 + d0.1 * d0.1;
        let d = (pa.0 - pb.0, pa.1 - pb.1);
        let w = d.0 * d.0 + d.1 * d.1;
        if w == 0.0 {
            return spans;
        }
        width *= (w0 / w).sqrt();
    }
    if !same(pa.1, pb.1) {
        return spans; // only the horizontal case is reproduced
    }
    let x = (pa.0 + pb.0) * 0.5;
    let dx = (pb.0 - pa.0).abs() * 0.5;
    let y = pa.1;
    let dy = width * dx;
    pa = (x, y - dy);
    pb = (x, y + dy);
    width = 1.0 / width;
    if !same(pa.0, pb.0) {
        return spans;
    }
    if pa.1 > pb.1 {
        std::mem::swap(&mut pa, &mut pb);
    }
    let dy = pb.1 - pa.1;
    let half = 0.5 * width * dy;
    let bound = |lo: f64, v: f64, hi: f64| v.max(lo).min(hi);
    let left = bound(left_c as f64, pa.0 - half, (right_c + 1) as f64);
    let right = bound(left_c as f64, pa.0 + half, (right_c + 1) as f64);
    pa.1 = bound(top_c as f64, pa.1, (bottom_c + 1) as f64);
    pb.1 = bound(top_c as f64, pb.1, (bottom_c + 1) as f64);
    if same(left, right) || same(pa.1, pb.1) {
        return spans;
    }
    // QScFixed is 32-bit on the tablet.
    let to_fixed = |v: f64| -> i32 {
        let t = v * 65536.0;
        if t > i32::MAX as f64 {
            i32::MAX
        } else if t < i32::MIN as f64 {
            -i32::MAX
        } else {
            t as i32
        }
    };
    let (i_left, i_right) = (left as i32, right as i32);
    let left_w = ((i_left + 1) << 16) - to_fixed(left);
    let right_w = to_fixed(right) - (i_right << 16);
    let mut cov = [0i32; 3];
    let mut xs = [0i32; 3];
    let mut lens = [0i32; 3];
    let mut n = 1;
    if i_left == i_right {
        cov[0] = if left_w == FIXED {
            right_w * 255
        } else {
            (right_w + left_w - FIXED) * 255
        };
        xs[0] = i_left;
        lens[0] = 1;
    } else {
        cov[0] = left_w * 255;
        xs[0] = i_left;
        lens[0] = 1;
        if left_w == FIXED {
            lens[0] = i_right - i_left;
        } else if i_right - i_left > 1 {
            cov[1] = 255 << 16;
            xs[1] = i_left + 1;
            lens[1] = i_right - i_left - 1;
            n += 1;
        }
        if right_w != 0 {
            cov[n] = right_w * 255;
            xs[n] = i_right;
            lens[n] = 1;
            n += 1;
        }
    }
    let (top_fp, bottom_fp) = ((pa.1 as i32) << 16, (pb.1 as i32) << 16);
    let (ya, yb) = (to_fixed(pa.1), to_fixed(pb.1));
    let mut yfp = top_fp;
    while yfp <= bottom_fp {
        let row_h = (yfp + FIXED).min(yb) - yfp.max(ya);
        let y = yfp >> 16;
        if y > bottom_c {
            break;
        }
        for i in 0..n {
            let c = ((row_h as i64 * cov[i] as i64) >> 16) as i32 >> 16;
            if c != 0 && lens[i] != 0 {
                spans.push(Span {
                    x: xs[i],
                    len: lens[i],
                    y,
                    coverage: c as u32,
                });
            }
        }
        yfp += FIXED;
    }
    spans
}

/// `qClipLine`: false when something is left.
fn clip_line(p1: &mut (f64, f64), p2: &mut (f64, f64), c: (f64, f64, f64, f64)) -> bool {
    let (xmin, ymin, xmax, ymax) = c;
    let (x1, y1) = (&mut p1.0, &mut p1.1);
    let (x2, y2) = (&mut p2.0, &mut p2.1);
    if *x1 < xmin {
        if *x2 <= xmin {
            return true;
        }
        *y1 += (*y2 - *y1) / (*x2 - *x1) * (xmin - *x1);
        *x1 = xmin;
    } else if *x1 > xmax {
        if *x2 >= xmax {
            return true;
        }
        *y1 += (*y2 - *y1) / (*x2 - *x1) * (xmax - *x1);
        *x1 = xmax;
    }
    if *x2 < xmin {
        *y2 += (*y2 - *y1) / (*x2 - *x1) * (xmin - *x2);
        *x2 = xmin;
    } else if *x2 > xmax {
        *y2 += (*y2 - *y1) / (*x2 - *x1) * (xmax - *x2);
        *x2 = xmax;
    }
    if *y1 < ymin {
        if *y2 <= ymin {
            return true;
        }
        *x1 += (*x2 - *x1) / (*y2 - *y1) * (ymin - *y1);
        *y1 = ymin;
    } else if *y1 > ymax {
        if *y2 >= ymax {
            return true;
        }
        *x1 += (*x2 - *x1) / (*y2 - *y1) * (ymax - *y1);
        *y1 = ymax;
    }
    if *y2 < ymin {
        *x2 += (*x2 - *x1) / (*y2 - *y1) * (ymin - *y2);
        *y2 = ymin;
    } else if *y2 > ymax {
        *x2 += (*x2 - *x1) / (*y2 - *y1) * (ymax - *y2);
        *y2 = ymax;
    }
    false
}

/// `QPainter::drawImage(QRectF(-0.5, -0.5, 1, 1), image, QRectF(0, 0, w, h))` with
/// `SmoothPixmapTransform` and `Antialiasing` under `m`, onto the canvas's current view whose
/// device is `device` pixels square (the exporter's tile). False when the transform rotates or
/// shears (not reproduced).
pub(crate) fn draw_image(canvas: &mut Canvas, m: &Affine, tex: &Texture, device: i32) -> bool {
    if tex.w == 0 || tex.h == 0 {
        return true;
    }
    if m.kind() > Kind::Scale {
        return false;
    }
    let (rx, ry, rw, rh) = (-0.5, -0.5, 1.0, 1.0);
    let (sw, sh) = (tex.w as f64, tex.h as f64);
    let mut copy = *m;
    copy.translate(rx, ry);
    copy.scale(rw / sw, rh / sh);
    let inv = span_inverse(&copy);
    let uniform = {
        let (xs, ys) = (m.m11.abs(), m.m22.abs());
        m.kind() <= Kind::Translate || (xs - ys).abs() * 1e12 <= xs.min(ys)
    };
    let a = m.map(rx, ry + rh * 0.5);
    let b = m.map(rx + rw, ry + rh * 0.5);
    let width = if uniform {
        rh / rw
    } else {
        ((m.m22 * rh) / (m.m11 * rw)).abs()
    };
    let spans = rasterize_horizontal(a, b, width, (0, 0, device, device));
    let mut buf = vec![0u32; BUFFER_SIZE];
    let mut c = 0;
    while c < spans.len() {
        let (x0, y) = (spans[c].x, spans[c].y);
        let mut right = x0 + spans[c].len;
        let mut e = c + 1;
        while e < spans.len() && spans[e].y == y && spans[e].x == right {
            right += spans[e].len;
            e += 1;
        }
        let mut x = x0;
        while x < right {
            let l = (right - x).min(BUFFER_SIZE as i32);
            let src = &mut buf[..l as usize];
            fetch(src, tex, &inv, x, y);
            for (i, &s) in src.iter().enumerate() {
                let px = x + i as i32;
                let span = spans[c..e]
                    .iter()
                    .find(|sp| sp.x <= px && px < sp.x + sp.len)
                    .unwrap();
                if let Some(mut d) = canvas.get(px, y) {
                    source_over(&mut d, s, span.coverage);
                    canvas.put(px, y, d);
                }
            }
            x += l;
        }
        c = e;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn div255_rounds_to_nearest() {
        for t in 0..=255 * 255 {
            assert_eq!(div255(t), (t as f64 / 255.0).round() as u32, "{t}");
        }
    }

    #[test]
    fn transform_composition_follows_qt() {
        let mut m = Affine::translation(10.0, 20.0);
        m.scale(2.0, 3.0);
        assert_eq!(m.map(1.0, 1.0), (12.0, 23.0));
        m.translate(1.0, 1.0);
        assert_eq!(m.map(0.0, 0.0), (12.0, 23.0));
        let s = Affine {
            m11: 2.0,
            m22: 4.0,
            ..Affine::IDENTITY
        };
        let t = s.then(&Affine::translation(1.0, 1.0));
        assert_eq!(t.map(1.0, 1.0), (3.0, 5.0));
        assert_eq!(t.then(&t.inverted()).map(7.0, 9.0), (7.0, 9.0));
    }

    #[test]
    fn opaque_image_covers_its_rect() {
        let px = vec![0xff10_2030; 16];
        let tex = Texture {
            w: 4,
            h: 4,
            px: &px,
        };
        let mut canvas = Canvas::new(16, 16);
        // An 8 x 8 square centered at (8, 8): pixels 4..12 fully covered.
        let m = Affine {
            m11: 8.0,
            m22: 8.0,
            dx: 8.0,
            dy: 8.0,
            ..Affine::IDENTITY
        };
        assert!(draw_image(&mut canvas, &m, &tex, 16));
        for y in 0..16 {
            for x in 0..16 {
                let inside = (4..12).contains(&x) && (4..12).contains(&y);
                let p = canvas.px[(y * 16 + x) as usize];
                assert_eq!(p, if inside { 0xff10_2030 } else { 0 }, "({x}, {y})");
            }
        }
    }
}
