use super::raster::{Raster, Var, Vertex};
use std::f32::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    Bevel = 0,
    Round = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    Flat = 0,
    Square = 1,
    Round = 2,
}

/// Varyings for a stroke edge: `(length, half_width, side)` -> varying; side is +1 left, -1 right.
pub trait VaryingGen {
    fn varying(&self, length: f32, half_width: f32, side: f32) -> Var;
}

pub struct NoVarying;
impl VaryingGen for NoVarying {
    fn varying(&self, _: f32, _: f32, _: f32) -> Var {
        Var::default()
    }
}

/// `VaryingGenerator_WidthLength`: (length * k1, side * half_width * k2).
pub struct WidthLength {
    pub k1: f32,
    pub k2: f32,
}
impl VaryingGen for WidthLength {
    fn varying(&self, length: f32, half_width: f32, side: f32) -> Var {
        Var([length * self.k1, side * half_width * self.k2, 0.0, 0.0])
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Line {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Line {
    fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Line { x0, y0, x1, y1 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SegType {
    #[default]
    Invalid,
    Move,
    LineTo,
}

#[derive(Debug, Clone, Copy, Default)]
struct Segment {
    x: f32,
    y: f32,
    width: f32,
    left: Var,
    right: Var,
    ty: SegType,
}

pub struct Stroker<G: VaryingGen> {
    pub join: Join,
    pub cap: Cap,
    pub width: f32,
    pub length: f32,
    pub varying: G,
    last: Segment,
    last_left: Line,
    last_right: Line,
    first: Segment,
    first_left: Line,
    first_right: Line,
    /// Start cap already drawn (stroker byte +0xc4).
    pub start_capped: bool,
    /// Suppress the end cap (stroker byte +0xc5).
    pub no_end_cap: bool,
}

impl<G: VaryingGen> Stroker<G> {
    pub fn new(varying: G) -> Self {
        Stroker {
            join: Join::Round,
            cap: Cap::Round,
            width: 1.0,
            length: 0.0,
            varying,
            last: Segment::default(),
            last_left: Line::default(),
            last_right: Line::default(),
            first: Segment::default(),
            first_left: Line::default(),
            first_right: Line::default(),
            start_capped: false,
            no_end_cap: false,
        }
    }

    fn store(&mut self, x: f32, y: f32, ty: SegType, left: Var, right: Var) {
        self.last = Segment {
            x,
            y,
            width: self.width,
            left,
            right,
            ty,
        };
    }

    /// Add a point; the first point of a stroke acts as a moveTo.
    pub fn line_to<R: Raster>(&mut self, r: &mut R, x: f32, y: f32) {
        if self.last.ty == SegType::Invalid {
            let hw = self.width * 0.5;
            let l = self.varying.varying(self.length, hw, 1.0);
            let rt = self.varying.varying(self.length, hw, -1.0);
            self.store(x, y, SegType::Move, l, rt);
            self.first = self.last;
            return;
        }
        if self.last.x == x && self.last.y == y {
            return;
        }
        let (x0, y0) = (self.last.x, self.last.y);
        let len = ((y - y0) * (y - y0) + (x - x0) * (x - x0)).sqrt();
        let ndx = (y0 - y) / len;
        let ndy = (x - x0) / len;
        let chw = self.width * 0.5;
        let lhw = self.last.width * 0.5;
        let right = Line::new(x0 + ndx * lhw, y0 + ndy * lhw, x + ndx * chw, y + ndy * chw);
        let left = Line::new(x0 - ndx * lhw, y0 - ndy * lhw, x - ndx * chw, y - ndy * chw);
        if self.last.ty == SegType::LineTo {
            let (ll, lr, lv, rv) = (
                self.last_left,
                self.last_right,
                self.last.left,
                self.last.right,
            );
            self.do_join(r, ll, lr, left, right, lv, rv);
        }
        self.length += len;
        let lv = self.varying.varying(self.length, chw, 1.0);
        let rv = self.varying.varying(self.length, chw, -1.0);
        stroke(
            r,
            left,
            right,
            self.last.left,
            self.last.right,
            lv,
            rv,
            false,
        );
        if self.last.ty == SegType::Move {
            self.first_left = left;
            self.first_right = right;
        }
        self.last_left = left;
        self.last_right = right;
        self.store(x, y, SegType::LineTo, lv, rv);
    }

    #[allow(clippy::too_many_arguments)]
    fn do_join<R: Raster>(
        &mut self,
        r: &mut R,
        last_left: Line,
        last_right: Line,
        left: Line,
        right: Line,
        lv: Var,
        rv: Var,
    ) {
        match self.join {
            Join::Bevel => stroke(
                r,
                Line::new(last_left.x1, last_left.y1, left.x0, left.y0),
                Line::new(last_right.x1, last_right.y1, right.x0, right.y0),
                lv,
                rv,
                lv,
                rv,
                true,
            ),
            Join::Round => {
                let (sx, sy) = (self.last.x, self.last.y);
                let a_last = atan2f(last_left.y1 - sy, last_left.x1 - sx);
                let a_next = atan2f(left.y0 - sy, left.x0 - sx);
                let mut delta = a_next - a_last;
                if delta < -PI {
                    delta += 2.0 * PI;
                } else if delta > PI {
                    delta -= 2.0 * PI;
                }
                if delta.abs() < PI / 10.0 {
                    stroke(
                        r,
                        Line::new(left.x0, left.y0, last_left.x1, last_left.y1),
                        Line::new(right.x0, right.y0, last_right.x1, last_right.y1),
                        lv,
                        rv,
                        lv,
                        rv,
                        true,
                    );
                    return;
                }
                let radius = self.last.width * 0.5;
                let n = ((delta * radius) as f64 / std::f64::consts::PI)
                    .abs()
                    .ceil() as i32;
                let steps = n.min(30);
                if n == 0 {
                    return;
                }
                let (mut llx, mut lly, mut lrx, mut lry) =
                    (last_left.x1, last_left.y1, last_right.x1, last_right.y1);
                let mut t = a_last;
                let fsteps = steps as f32;
                for _ in 0..steps {
                    t += delta / fsteps;
                    let (s, c) = sincosf(t);
                    let (lx, ly) = (c * radius + sx, s * radius + sy);
                    let (rx, ry) = (sx - c * radius, sy - s * radius);
                    stroke(
                        r,
                        Line::new(llx, lly, lx, ly),
                        Line::new(lrx, lry, rx, ry),
                        lv,
                        rv,
                        lv,
                        rv,
                        true,
                    );
                    (llx, lly, lrx, lry) = (lx, ly, rx, ry);
                }
            }
        }
    }

    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn draw_cap<R: Raster>(&self, r: &mut R, left: Line, right: Line, seg: Segment, end: bool) {
        if !(seg.width > 0.0) || self.cap == Cap::Flat {
            return;
        }
        let (px, py, qx, qy, ax, ay, bx, by) = if end {
            (
                left.x1, left.y1, right.x1, right.y1, left.x0, left.y0, right.x0, right.y0,
            )
        } else {
            (
                right.x0, right.y0, left.x0, left.y0, right.x1, right.y1, left.x1, left.y1,
            )
        };
        match self.cap {
            Cap::Round => {
                let mut lt = atan2f(py - seg.y, px - seg.x);
                let radius = seg.width * 0.5;
                let mut rt = lt + PI;
                let n = ((radius as f64 * std::f64::consts::PI) as f32 as f64
                    / std::f64::consts::PI)
                    .abs()
                    .ceil() as i32;
                let steps = n.min(30);
                let iters = if n < 4 { 1 } else { steps >> 1 };
                let dt = (std::f64::consts::PI / steps as f64) as f32;
                let (mut lx, mut ly, mut rx, mut ry) = (px, py, qx, qy);
                for _ in 0..iters {
                    rt -= dt;
                    lt += dt;
                    let (rs, rc) = sincosf(rt);
                    let (ls, lc) = sincosf(lt);
                    let (nlx, nly) = (seg.x + lc * radius, seg.y + ls * radius);
                    let (nrx, nry) = (seg.x + rc * radius, seg.y + rs * radius);
                    stroke(
                        r,
                        Line::new(lx, ly, nlx, nly),
                        Line::new(rx, ry, nrx, nry),
                        seg.left,
                        seg.right,
                        seg.left,
                        seg.right,
                        false,
                    );
                    (lx, ly, rx, ry) = (nlx, nly, nrx, nry);
                }
            }
            Cap::Square => {
                let hw = self.width * 0.5;
                let (ux, uy) = (px - ax, py - ay);
                let (vx, vy) = (qx - bx, qy - by);
                let ul = (uy * uy + ux * ux).sqrt();
                let vl = (vy * vy + vx * vx).sqrt();
                let (nlx, nly) = (px + ux / ul * hw, py + uy / ul * hw);
                let (nrx, nry) = (qx + vx / vl * hw, qy + vy / vl * hw);
                stroke(
                    r,
                    Line::new(px, py, nlx, nly),
                    Line::new(qx, qy, nrx, nry),
                    seg.left,
                    seg.right,
                    seg.left,
                    seg.right,
                    false,
                );
            }
            Cap::Flat => {}
        }
    }

    /// Draw the start cap now (the ballpoint pipeline does this right after the first segment).
    pub fn start_cap<R: Raster>(&mut self, r: &mut R) {
        if !self.start_capped && self.last.ty == SegType::LineTo {
            self.draw_cap(r, self.first_left, self.first_right, self.first, false);
            self.start_capped = true;
        }
    }

    /// End the stroke: close it with a join if it returns to its start, else draw the caps.
    pub fn finish<R: Raster>(&mut self, r: &mut R) {
        if self.last.ty == SegType::LineTo {
            if self.last.x == self.first.x && self.last.y == self.first.y {
                let (fx, fy) = (self.first.x, self.first.y);
                self.line_to(r, fx, fy);
                let (ll, lr, fl, fr, lv, rv) = (
                    self.last_left,
                    self.last_right,
                    self.first_left,
                    self.first_right,
                    self.last.left,
                    self.last.right,
                );
                self.do_join(r, ll, lr, fl, fr, lv, rv);
            } else {
                self.start_cap(r);
                if !self.no_end_cap {
                    self.draw_cap(r, self.last_left, self.last_right, self.last, true);
                }
            }
        }
        self.reset();
    }

    pub fn reset(&mut self) {
        self.last = Segment::default();
        self.first = Segment::default();
        self.first.width = 1.0;
        self.last.width = 1.0;
        self.first_left = Line::default();
        self.first_right = Line::default();
        self.length = 0.0;
        self.start_capped = false;
    }
}

#[allow(clippy::too_many_arguments)]
fn stroke<R: Raster>(
    r: &mut R,
    left: Line,
    mut right: Line,
    vl0: Var,
    mut vr0: Var,
    vl1: Var,
    mut vr1: Var,
    check_direction: bool,
) {
    if check_direction
        && (right.x1 - right.x0) * (left.x1 - left.x0) + (right.y1 - right.y0) * (left.y1 - left.y0)
            < 0.0
    {
        right = Line::new(right.x1, right.y1, right.x0, right.y0);
        std::mem::swap(&mut vr0, &mut vr1);
    }
    let v = |x, y| Vertex { x, y };
    r.triangle(
        v(left.x0, left.y0),
        v(left.x1, left.y1),
        v(right.x0, right.y0),
        vl0,
        vl1,
        vr0,
    );
    r.triangle(
        v(right.x0, right.y0),
        v(left.x1, left.y1),
        v(right.x1, right.y1),
        vr0,
        vl1,
        vr1,
    );
}

fn atan2f(y: f32, x: f32) -> f32 {
    y.atan2(x)
}
fn sincosf(t: f32) -> (f32, f32) {
    t.sin_cos()
}
