//! Contour smoothing of the vectorizer: the slope trace and the direction merge
//! applied to each traced chain.

use super::Pt;

fn split(v: i32, a: i32, b: i32) -> i32 {
    if a == b {
        return v / 2;
    }
    let (a, b) = (a.abs(), b.abs());
    let av = v.abs();
    let t = (a + b - av).min(64);
    let m = v.wrapping_mul(t);
    let half = v / 2 + (if m < 0 { m + 0x7f } else { m } >> 7);
    if a <= b {
        return half;
    }
    (if half < 1 { -av } else { av }) - half
}

fn ratio(v: i32, a: i32, b: i32) -> i32 {
    let (a, b) = (a.abs(), b.abs());
    if a < b {
        v - v.wrapping_mul(b) / (a + b)
    } else {
        v.wrapping_mul(a) / (a + b)
    }
}

pub fn slope_trace(ring: &[Pt]) -> Vec<Pt> {
    if ring.len() <= 4 {
        return ring.to_vec();
    }
    let mut pts = ring.to_vec();
    pts.extend_from_slice(&ring[1..4]);
    slope_run(&pts, true)
}

pub fn slope_trace_open(chain: &[Pt]) -> Vec<Pt> {
    if chain.len() <= 4 {
        return chain.to_vec();
    }
    slope_run(chain, false)
}

fn slope_run(pts: &[Pt], closed: bool) -> Vec<Pt> {
    let n = pts.len();
    let mut out = Vec::with_capacity(n);
    out.push(pts[0]);
    let (mut p0, mut p1, mut p2) = (pts[1], pts[2], pts[3]);
    let (mut dy0, mut dy1, mut dy2) = (p0.y - pts[0].y, p1.y - p0.y, p2.y - p1.y);
    let (mut dx0, mut dx1, mut dx2) = (p0.x - pts[0].x, p1.x - p0.x, p2.x - p1.x);

    let mut r = 4;
    {
        let q = pts[r];
        let (ady0, adx0) = (dy0.abs(), dx0.abs());
        let shift = (dy0 == 0 && dx0 == 4)
            || (dy0 == 4 && dx0 == 0)
            || (ady0 == 8 && adx0 == 8 && (dy1 == 0 || dx1 == 0));
        let half = if shift || ady0 == 8 || adx0 != ady0 {
            None
        } else if dy1.abs() == 8 {
            (dx1 == 0).then_some((0, dy1))
        } else if dy1 == 0 && dx1.abs() == 8 {
            Some((dx1, 0))
        } else {
            None
        };
        if shift || half.is_some() {
            p0 = match half {
                Some((hx, hy)) => p0 + Pt::new(hx / 2, hy / 2),
                None => p1,
            };
            p1 = p2;
            (dy0, dy1, dy2) = (dy1, dy2, q.y - p2.y);
            (dx0, dx1, dx2) = (dx1, dx2, q.x - p2.x);
            p2 = q;
            r += 1;
        }
    }

    while r < pts.len() {
        let q = pts[r];
        r += 1;
        let a = dy1.abs();
        let emit_shift = |out: &mut Vec<Pt>, e: Pt, p0: &mut Pt, p1: &mut Pt, p2: &mut Pt| {
            out.push(e);
            *p0 = *p1;
            *p1 = *p2;
            *p2 = q;
        };
        let (ndy2, ndx2) = (q.y - p2.y, q.x - p2.x);
        if a != dx1.abs() {
            emit_shift(&mut out, p0, &mut p0, &mut p1, &mut p2);
            (dy0, dy1, dy2) = (dy1, dy2, ndy2);
            (dx0, dx1, dx2) = (dx1, dx2, ndx2);
            continue;
        }
        if a > 8 {
            let hit = if dy0.abs() == 8 {
                dx0 == 0 && dy2.abs() == 8 && dx2 == 0
            } else {
                dy0 == 0 && dx0.abs() == 8 && dy2 == 0 && dx2.abs() == 8
            };
            if hit {
                p0 += Pt::new(dx1 / 2, dy1 / 2);
                p1 = p2;
                p2 = q;
            } else {
                emit_shift(&mut out, p0, &mut p0, &mut p1, &mut p2);
            }
            (dy0, dy1, dy2) = (dy1, dy2, ndy2);
            (dx0, dx1, dx2) = (dx1, dx2, ndx2);
            continue;
        }
        if a == 4 {
            if dy1 == dy0 && dx0 + dx1 == 0 {
                emit_shift(
                    &mut out,
                    Pt::new(p0.x, p0.y - dy1),
                    &mut p0,
                    &mut p1,
                    &mut p2,
                );
                (dy0, dy1, dy2) = (dy1, dy2, ndy2);
                (dx0, dx1, dx2) = (dx1, dx2, ndx2);
                continue;
            }
            if dx0 == dx1 && -dy0 == dy1 {
                emit_shift(
                    &mut out,
                    Pt::new(p0.x - dx0, p0.y),
                    &mut p0,
                    &mut p1,
                    &mut p2,
                );
                (dy0, dy1, dy2) = (dy1, dy2, ndy2);
                (dx0, dx1, dx2) = (dx1, dx2, ndx2);
                continue;
            }
        }
        if (dx0 != 0 && dy0 != 0) || (dx2 != 0 && dy2 != 0) {
            emit_shift(&mut out, p0, &mut p0, &mut p1, &mut p2);
            (dy0, dy1, dy2) = (dy1, dy2, ndy2);
            (dx0, dx1, dx2) = (dx1, dx2, ndx2);
            continue;
        }
        if dx0 != 0 || dx2 != 0 {
            let h = dx1 / 2;
            if dy2 == 0 && dy0 == 0 {
                let t = if dx0 == dx2 {
                    dy1 / 2
                } else {
                    ratio(dy1, dx0 + h, dx2 + h)
                };
                p0 += Pt::new(h, t);
            } else {
                let ty = split(dy1, dx0 + h, dx2 + h);
                let tx = split(dx1, dy0 + dy1 / 2, dy2 + dy1 / 2);
                p0 += Pt::new(tx, ty);
            }
        } else {
            let h = dy1 / 2;
            let t = if dy2 == dy0 {
                dx1 / 2
            } else {
                ratio(dx1, dy0 + h, dy2 + h)
            };
            p0 += Pt::new(t, h);
        }
        p1 = p2;
        p2 = q;
        (dy0, dy1, dy2) = (dy1, dy2, ndy2);
        (dx0, dx1, dx2) = (dx1, dx2, ndx2);
    }
    if !closed {
        open_tail(&mut out, p0, p1, p2, [dy0, dy1, dy2], [dx0, dx1, dx2]);
    }
    out
}

enum Tail {
    /// Classify by the last step.
    Last,
    /// The last step is horizontal.
    Flat,
    /// The middle step decides whether p1 is pulled back.
    Middle,
    /// Emit p1 (with y = `y`), then p2.
    Emit(i32),
    /// Emit p2 only.
    End,
}

fn open_tail(out: &mut Vec<Pt>, p0: Pt, mut p1: Pt, p2: Pt, dy: [i32; 3], dx: [i32; 3]) {
    let [dy0, dy1, dy2] = dy;
    let [dx0, dx1, dx2] = dx;
    let mut next = if dy1.abs() != dx1.abs() {
        out.push(p1);
        Tail::Last
    } else if dy1.abs() > 8 {
        if (dy0.abs() != 8 || dy2.abs() != 8) && (dx0.abs() != 8 || dx2.abs() != 8) {
            out.push(p0);
        } else {
            let h = dx1 / 2;
            let t = if dx0 == dx2 {
                dy1 / 2
            } else {
                ratio(dy1, dx0 + h, dx2 + h)
            };
            p1 = p0 + Pt::new(h, t);
        }
        Tail::Last
    } else if (dx0 != 0 && dy0 != 0) || (dx2 != 0 && dy2 != 0) {
        out.push(p0);
        Tail::Last
    } else if dx0 != 0 || dx2 != 0 {
        let h = dx1 / 2;
        if dy0 != 0 || dy2 != 0 {
            let ty = split(dy1, dx0 + h, dx2 + h);
            let tx = split(dx1, dy0 + dy1 / 2, dy2 + dy1 / 2);
            p1 = p0 + Pt::new(tx, ty);
            Tail::Last
        } else {
            let t = if dx0 == dx2 {
                dy1 / 2
            } else {
                ratio(dy1, dx0 + h, dx2 + h)
            };
            p1 = p0 + Pt::new(h, t);
            Tail::Flat
        }
    } else {
        let h = dy1 / 2;
        let t = if dy0 == dy2 {
            dx1 / 2
        } else {
            ratio(dx1, dy0 + h, dy2 + h)
        };
        p1 = p0 + Pt::new(t, h);
        match dy2.abs() {
            0 => Tail::Middle,
            4 => Tail::End,
            8 => Tail::Emit(p1.y),
            _ => {
                if dx2.abs() == dy2.abs() {
                    Tail::Middle
                } else {
                    Tail::Emit(p1.y)
                }
            }
        }
    };
    loop {
        next = match next {
            Tail::Last => match dy2.abs() {
                0 => Tail::Flat,
                4 if dx2 == 0 => Tail::End,
                8 if dx2.abs() == 8 && (dy1 == 0 || dx1 == 0) => Tail::End,
                8 => Tail::Emit(p1.y),
                a => {
                    if a == dx2.abs() {
                        Tail::Middle
                    } else {
                        Tail::Emit(p1.y)
                    }
                }
            },
            Tail::Flat => match dx2.abs() {
                4 => Tail::End,
                0 => Tail::Middle,
                _ => Tail::Emit(p1.y),
            },
            Tail::Middle => {
                let back = if dy1.abs() == 8 && dx1 == 0 {
                    Some(0)
                } else if dy1 == 0 && dx1.abs() == 8 {
                    Some(dx1)
                } else {
                    None
                };
                match back {
                    Some(bx) => {
                        out.push(p1 - Pt::new(bx / 2, dy1 / 2));
                        Tail::End
                    }
                    None => Tail::Emit(p1.y),
                }
            }
            Tail::Emit(y) => {
                out.push(Pt::new(p1.x, y));
                Tail::End
            }
            Tail::End => {
                out.push(p2);
                return;
            }
        };
    }
}

/// Quantized direction
fn qdir(dx: i32, dy: i32, q: i32) -> (i32, i32) {
    let sg = |v: i32| if v < 0 { -q } else { q };
    if dy == 0 {
        return if dx == 0 { (0, 0) } else { (sg(dx), 0) };
    }
    match dx.abs().cmp(&dy.abs()) {
        std::cmp::Ordering::Less => (dx * q / dy, sg(dy)),
        std::cmp::Ordering::Greater => (sg(dx), dy * q / dx),
        std::cmp::Ordering::Equal => (sg(dx), sg(dy)),
    }
}

/// Direction merge after the slope trace: drops points whose step keeps the
/// quantized direction of the run so far.
pub fn merge(pts: &[Pt], closed: bool, q: i32) -> Vec<Pt> {
    let mut out = Vec::new();
    let Some(&first) = pts.first() else {
        return out;
    };
    let (mut anchor, mut cur) = (first, first);
    let mut d = Pt::default();
    for &p in &pts[1..] {
        let q1 = qdir(d.x, d.y, q);
        let s = p - cur;
        let q2 = qdir(s.x, s.y, q);
        if (q1.1 - q2.1).abs() <= 1 && (q1.0 - q2.0).abs() <= 1 {
            d = p - anchor;
        } else {
            out.push(cur);
            anchor = cur;
            d = s;
        }
        cur = p;
    }
    out.push(cur);
    if closed {
        out[0] = cur;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(s: &str) -> Vec<Pt> {
        s.split_whitespace()
            .map(|p| {
                let (x, y) = p.split_once(',').unwrap();
                Pt::new(x.parse().unwrap(), y.parse().unwrap())
            })
            .collect()
    }

    /// Pairs of lines: the input, then the emulated xochitl routine's output.
    fn check(data: &str, f: fn(&[Pt]) -> Vec<Pt>) {
        let lines: Vec<&str> = data.lines().collect();
        assert!(!lines.is_empty());
        for (i, case) in lines.chunks(2).enumerate() {
            assert_eq!(
                f(&points(case[0])),
                points(case[1]),
                "case {i}: {}",
                case[0]
            );
        }
    }

    #[test]
    fn closed_rings_match_xochitl() {
        check(include_str!("../../testdata/slope_closed.txt"), slope_trace);
    }

    #[test]
    fn open_chains_match_xochitl() {
        check(
            include_str!("../../testdata/slope_open.txt"),
            slope_trace_open,
        );
    }
}
