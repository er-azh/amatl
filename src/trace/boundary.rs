//! exporttracer: contours as pairwise region boundaries.
//!
//! Every pixel corner where labels differ feeds each pair of distinct labels A < B through a
//! 256 entry emitter table indexed by 2 bits per corner (A, B or a third label). The emitters
//! link half-pixel segments into the chains of that pair's `Boundary`. A third label makes the
//! segment end at the pixel corner, so boundaries are open chains meeting at triple junctions.
//! Each region then smooths its boundaries' chains into pieces, stitches pieces end to end and
//! emits them as subpaths. The first region to process a boundary stores the reversed pieces
//! for the region on the other side.
//!
//! All of this runs in xochitl's transposed frame (points are (y, x)); [`Tracer::region`]
//! swaps the output back. Chain direction, start points and the smoothing depend on it.

use super::Pt;
use super::smooth::{merge, slope_trace, slope_trace_open};

/// Corner neighborhood midpoints in 1/8 px, relative to the corner.
const T: Pt = Pt::new(0, -4);
const B: Pt = Pt::new(0, 4);
const L: Pt = Pt::new(-4, 0);
const R: Pt = Pt::new(4, 0);

enum Emit {
    /// Link a segment.
    Seg(Pt, Pt),
    /// Segment from an edge midpoint into the corner (a triple junction).
    In(Pt),
    /// Segment from the corner out to an edge midpoint.
    Out(Pt),
    /// Saddle: deferred until all segments exist; four candidate segments `from[i] -> to[i]`.
    Saddle([Pt; 4], [Pt; 4]),
}

/// Code bits: TL 6-7, BL 4-5, TR 2-3, BR 0-1; per corner
/// 0 = A, 1 = B, 2 = another label.
fn emitter(code: u8) -> Option<Emit> {
    use Emit::*;
    Some(match code {
        0x01 | 0x81 => Seg(R, B),
        0x04 | 0x24 => Seg(T, R),
        0x05 => Seg(T, B),
        0x10 | 0x18 => Seg(B, L),
        0x11 => Seg(R, L),
        0x15 | 0x16 => Seg(T, L),
        0x40 | 0x42 => Seg(L, T),
        0x44 => Seg(L, R),
        0x45 | 0x49 => Seg(L, B),
        0x50 => Seg(B, T),
        0x51 | 0x61 => Seg(R, T),
        0x54 | 0x94 => Seg(B, R),
        0x06 | 0x25 | 0x26 => In(T),
        0x21 | 0x91 | 0xa1 => In(R),
        0x46 | 0x48 | 0x4a => In(L),
        0x58 | 0x90 | 0x98 => In(B),
        0x09 | 0x85 | 0x89 => Out(B),
        0x12 | 0x19 | 0x1a => Out(L),
        0x52 | 0x60 | 0x62 => Out(T),
        0x64 | 0x84 | 0xa4 => Out(R),
        0x14 => Saddle([T, B, T, B], [L, R, R, L]),
        0x41 => Saddle([L, R, L, R], [B, T, T, B]),
        _ => return None,
    })
}

fn coll(p: Pt, q: Pt, r: Pt) -> bool {
    let (dx1, dy1, dx2, dy2) = (q.x - p.x, q.y - p.y, r.x - q.x, r.y - q.y);
    if dy1 == 0 && dy2 == 0 || dx1 == 0 && dx2 == 0 {
        return true;
    }
    if dx1 == 0 || dx2 == 0 || dy1 == 0 || dy2 == 0 {
        return false;
    }
    (dy1 ^ dy2) >= 0 && (dx1 ^ dx2) >= 0
}

#[derive(Clone, Debug)]
pub struct Piece {
    s: Pt,
    e: Pt,
    pub pts: Vec<Pt>,
}

impl Piece {
    fn reverse(&mut self) {
        std::mem::swap(&mut self.s, &mut self.e);
        self.pts.reverse();
    }
}

#[derive(Default)]
struct Boundary {
    a: u32,
    b: u32,
    chains: Vec<Vec<Pt>>,
    items: Vec<([Pt; 4], [Pt; 4])>,
    done: Option<Vec<Piece>>,
    dead: bool,
}

impl Boundary {
    fn back(&self, p: Pt) -> Option<usize> {
        self.chains.iter().position(|c| *c.last().unwrap() == p)
    }

    fn front(&self, p: Pt) -> Option<usize> {
        self.chains.iter().position(|c| c[0] == p)
    }

    fn push_back(&mut self, i: usize, p: Pt) {
        let c = &mut self.chains[i];
        let n = c.len();
        if n >= 2 && coll(c[n - 2], c[n - 1], p) {
            c[n - 1] = p;
        } else {
            c.push(p);
        }
    }

    fn push_front(&mut self, i: usize, p: Pt) {
        let c = &mut self.chains[i];
        if c.len() >= 2 && coll(p, c[0], c[1]) {
            c[0] = p;
        } else {
            c.insert(0, p);
        }
    }

    fn close(&mut self, i: usize) {
        let c = &mut self.chains[i];
        let n = c.len();
        if coll(c[n - 2], c[n - 1], c[0]) {
            c.pop();
        }
        let n = c.len();
        if coll(c[n - 1], c[0], c[1]) {
            c[0] = c[n - 1];
        } else {
            let f = c[0];
            c.push(f);
        }
    }

    fn join(&mut self, i: usize, j: usize) {
        let cb = std::mem::take(&mut self.chains[j]);
        let ca = &mut self.chains[i];
        let n = ca.len();
        if coll(ca[n - 2], ca[n - 1], cb[0]) {
            ca.pop();
        }
        let n = ca.len();
        let k = if coll(ca[n - 1], cb[0], cb[1]) { 1 } else { 0 };
        ca.extend_from_slice(&cb[k..]);
        let merged = std::mem::take(&mut self.chains[i]);
        let (keep, drop) = (i.min(j), i.max(j));
        self.chains[keep] = merged;
        self.chains.remove(drop);
    }

    fn add(&mut self, a: Pt, b: Pt) {
        match (self.back(a), self.front(b)) {
            (None, None) => self.chains.push(vec![a, b]),
            (Some(i), None) => self.push_back(i, b),
            (None, Some(j)) => self.push_front(j, a),
            (Some(i), Some(j)) if i == j => self.close(i),
            (Some(i), Some(j)) => self.join(i, j),
        }
    }

    fn link(&mut self, i: usize, j: usize) {
        if i == j {
            self.close(i);
        } else {
            self.join(i, j);
        }
    }

    fn resolve(&mut self, p: [Pt; 4], q: [Pt; 4]) -> bool {
        if let Some(j) = self.front(q[0]) {
            match self.back(p[0]) {
                None => self.push_front(j, p[0]),
                Some(i) => self.link(i, j),
            }
            self.add(p[1], q[1]);
            return true;
        }
        if let Some(i) = self.back(p[1]) {
            match self.front(q[1]) {
                None => self.push_back(i, q[1]),
                Some(j) => self.link(i, j),
            }
            self.add(p[0], q[0]);
            return true;
        }
        match self.back(p[2]) {
            None => {
                if let Some(j) = self.front(q[2]) {
                    self.push_front(j, p[2]);
                    self.add(p[3], q[3]);
                    return true;
                }
            }
            Some(i) => {
                if let Some(j) = self.front(q[2]) {
                    self.link(i, j);
                    self.add(p[3], q[3]);
                    return true;
                }
            }
        }
        let Some(i) = self.back(p[3]) else {
            return false;
        };
        match self.front(q[3]) {
            None => self.push_back(i, q[3]),
            Some(j) => self.link(i, j),
        }
        self.add(p[2], q[2]);
        true
    }

    fn resolve_all(&mut self) {
        let mut items = std::mem::take(&mut self.items);
        if items.is_empty() {
            return;
        }
        let (mut i, mut progress) = (0, false);
        loop {
            let (p, q) = items[i];
            if self.resolve(p, q) {
                items.remove(i);
                progress = !items.is_empty();
            } else {
                i += 1;
            }
            if i == items.len() {
                if !progress || items.is_empty() {
                    break;
                }
                progress = false;
                i = 0;
            }
        }
        self.items = items;
    }
}

fn join_pieces(v: &mut Vec<Piece>) {
    let mut out: Vec<Piece> = Vec::with_capacity(v.len());
    let mut rest: Vec<Option<Piece>> = std::mem::take(v).into_iter().map(Some).collect();
    for i in 0..rest.len() {
        let cur = rest[i].take().unwrap();
        let mut merged = false;
        for o in rest[i + 1..].iter_mut() {
            let o = o.as_mut().unwrap();
            if o.s == cur.e {
                let mut pts = cur.pts.clone();
                pts.extend_from_slice(&o.pts[1..]);
                *o = Piece {
                    s: cur.s,
                    e: o.e,
                    pts,
                };
                merged = true;
                break;
            }
            if cur.s == o.e {
                let mut pts = o.pts[..o.pts.len() - 1].to_vec();
                pts.extend_from_slice(&cur.pts);
                *o = Piece {
                    s: o.s,
                    e: cur.e,
                    pts,
                };
                merged = true;
                break;
            }
        }
        if !merged {
            out.push(cur);
        }
    }
    *v = out;
}

pub struct Config {
    /// Direction merge quantum (0 = collinear only).
    pub q: i32,
    pub smooth: bool,
}

pub struct Tracer {
    bounds: Vec<Boundary>,
    /// Per label: boundary indices in registration order.
    lists: Vec<Vec<usize>>,
}

impl Tracer {
    pub fn new(labels: usize) -> Self {
        Tracer {
            bounds: Vec::new(),
            lists: vec![Vec::new(); labels],
        }
    }

    fn list(&mut self, l: u32) -> &mut Vec<usize> {
        if self.lists.len() <= l as usize {
            self.lists.resize(l as usize + 1, Vec::new());
        }
        &mut self.lists[l as usize]
    }

    pub fn union(&mut self, lo: u32, hi: u32) {
        let mut moved = std::mem::take(self.list(hi));
        moved.retain(|&i| {
            let bd = &mut self.bounds[i];
            if bd.dead {
                return true;
            }
            if bd.a == hi {
                bd.a = lo;
            } else if bd.b == hi {
                bd.b = lo;
            }
            if bd.a == bd.b {
                return false;
            }
            if bd.b < bd.a {
                std::mem::swap(&mut bd.a, &mut bd.b);
                for c in &mut bd.chains {
                    c.reverse();
                }
                for (p, q) in &mut bd.items {
                    std::mem::swap(p, q);
                }
            }
            true
        });
        for i in moved {
            let (a, b, dead) = (self.bounds[i].a, self.bounds[i].b, self.bounds[i].dead);
            self.list(lo);
            let found = if dead {
                None
            } else {
                self.lists[lo as usize].iter().copied().find(|&j| {
                    !self.bounds[j].dead && self.bounds[j].a == a && self.bounds[j].b == b
                })
            };
            match found {
                None => self.list(lo).push(i),
                Some(j) => {
                    let other = if a != lo { a } else { b };
                    self.list(other).retain(|&k| k != i);
                    let chains = std::mem::take(&mut self.bounds[i].chains);
                    let items = std::mem::take(&mut self.bounds[i].items);
                    self.bounds[j].chains.extend(chains);
                    self.bounds[j].items.extend(items);
                }
            }
        }
    }

    pub fn corner(&mut self, x: i32, y: i32, tl: u32, bl: u32, tr: u32, br: u32) {
        let (x, y, bl, tr) = (y, x, tr, bl);
        let mut ls = [tl, bl, tr, br];
        ls.sort_unstable();
        let mut uniq: Vec<u32> = ls.to_vec();
        uniq.dedup();
        if uniq.len() < 2 {
            return;
        }
        let at = Pt::new(x * 8, y * 8);
        self.list(uniq[uniq.len() - 1]);
        for (ia, &a) in uniq.iter().enumerate() {
            for &b in &uniq[ia + 1..] {
                let v = |l: u32| {
                    if l == a {
                        0
                    } else if l == b {
                        1
                    } else {
                        2
                    }
                };
                let code = (v(tl) << 6) | (v(bl) << 4) | (v(tr) << 2) | v(br);
                let Some(em) = emitter(code) else { continue };
                let bi = match self.lists[b as usize].iter().find(|&&i| {
                    !self.bounds[i].dead && self.bounds[i].a == a && self.bounds[i].b == b
                }) {
                    Some(&i) => i,
                    None => {
                        self.bounds.push(Boundary {
                            a,
                            b,
                            ..Default::default()
                        });
                        let i = self.bounds.len() - 1;
                        self.lists[b as usize].push(i);
                        self.lists[a as usize].push(i);
                        i
                    }
                };
                let bd = &mut self.bounds[bi];
                let off = |p: Pt| at + p;
                match em {
                    Emit::Seg(p, q) => bd.add(off(p), off(q)),
                    Emit::In(p) => match bd.back(off(p)) {
                        Some(i) => bd.push_back(i, at),
                        None => bd.chains.push(vec![off(p), at]),
                    },
                    Emit::Out(p) => match bd.front(off(p)) {
                        Some(i) => bd.push_front(i, at),
                        None => bd.chains.push(vec![at, off(p)]),
                    },
                    Emit::Saddle(p, q) => bd.items.push((p.map(off), q.map(off))),
                }
            }
        }
    }

    pub fn region(&mut self, r: u32, cfg: &Config) -> Vec<Vec<Pt>> {
        let ids = self.list(r).clone();
        for &i in &ids {
            self.bounds[i].resolve_all();
        }
        let mut list: Vec<Piece> = Vec::new();
        for &i in &ids {
            let bd = &mut self.bounds[i];
            if let Some(stored) = bd.done.take() {
                list.extend(stored);
                continue;
            }
            let mut pieces = Vec::new();
            for chain in &bd.chains {
                let mut c = chain.clone();
                if bd.a == r {
                    c.reverse();
                }
                let (s, e) = (c[0], *c.last().unwrap());
                let closed = s == e;
                let pts = if c.len() > 4 {
                    let t = if !cfg.smooth {
                        c.clone()
                    } else if closed {
                        slope_trace(&c)
                    } else {
                        slope_trace_open(&c)
                    };
                    merge(&t, closed, cfg.q)
                } else {
                    c.clone()
                };
                pieces.push(Piece { s, e, pts });
            }
            join_pieces(&mut pieces);
            list.extend(pieces.iter().cloned());
            for p in &mut pieces {
                p.reverse();
            }
            bd.done = Some(pieces);
            bd.dead = true;
        }
        join_pieces(&mut list);
        list.into_iter()
            .map(|p| p.pts.iter().map(|p| p.transpose()).collect())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trace every region of a label image (0 outside), highest label first, as
    /// "label: x,y ...|x,y ..." lines.
    fn trace(w: i32, h: i32, lab: &[u32]) -> Vec<String> {
        let n = *lab.iter().max().unwrap() as usize + 1;
        let at = |x: i32, y: i32| {
            if x < 0 || y < 0 || x >= w || y >= h {
                0
            } else {
                lab[(y * w + x) as usize]
            }
        };
        let mut t = Tracer::new(n);
        for y in 0..=h {
            for x in 0..=w {
                t.corner(x, y, at(x - 1, y - 1), at(x - 1, y), at(x, y - 1), at(x, y));
            }
        }
        let cfg = Config {
            q: 16,
            smooth: true,
        };
        (1..n as u32)
            .rev()
            .map(|r| {
                let sps: Vec<String> = t
                    .region(r, &cfg)
                    .iter()
                    .map(|sp| {
                        sp.iter()
                            .map(|p| format!("{},{}", p.x, p.y))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .collect();
                format!("{r}: {}", sps.join("|"))
            })
            .collect()
    }

    /// Cases of "w h", h rows of labels, the emulated tracer's regions, then "--".
    #[test]
    fn regions_match_xochitl() {
        let data = include_str!("../../testdata/boundary.txt");
        let mut cases = 0;
        let mut lines = data.lines();
        while let Some(size) = lines.next() {
            let (w, h) = size.split_once(' ').unwrap();
            let (w, h): (i32, i32) = (w.parse().unwrap(), h.parse().unwrap());
            let lab: Vec<u32> = (0..h)
                .flat_map(|_| lines.next().unwrap().split(' ').map(|v| v.parse().unwrap()))
                .collect();
            let want: Vec<&str> = lines.by_ref().take_while(|&l| l != "--").collect();
            assert_eq!(trace(w, h, &lab), want, "case {cases} ({w} x {h})");
            cases += 1;
        }
        assert!(cases > 0);
    }
}
