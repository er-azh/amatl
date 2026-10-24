//! Region labeling of the exporter: region growing over the premultiplied raster in 1024-pixel
//!  tiles of 64-pixel blocks (row-major at each level, raster order inside a block).
//! A pixel joins its left or upper neighbor's region when the L1 distance between the pixel
//! and the region's mean with the pixel added is below [`THRESHOLD`]; when both qualify and
//! their means are close too, the two regions are merged.

pub const THRESHOLD: i64 = 10;
const FAR: i64 = i64::MAX;

#[derive(Clone)]
pub struct Rec {
    pub parent: u32,
    pub class: u8,
    /// Sums of R, G, B, A (premultiplied channel values).
    pub sum: [i64; 4],
    /// Number of non-transparent pixels.
    pub nz: i64,
    /// Pixels whose bottom-right corner has not been traced yet (the region is finished and
    /// emitted after a tile when this is zero).
    pub pending: i64,
    emitted: bool,
}

impl Rec {
    fn new(i: u32, class: u8) -> Self {
        Rec {
            parent: i,
            class,
            sum: [0; 4],
            nz: 0,
            pending: 0,
            emitted: false,
        }
    }

    /// Exported color (straight ARGB): mean alpha, channels unpremultiplied from the sums.
    pub fn argb(&self) -> u32 {
        let a = if self.nz == 0 {
            0
        } else {
            self.sum[3] / self.nz
        };
        if a == 0 || self.sum[3] == 0 {
            return 0;
        }
        let c = |k: usize| ((self.sum[k] * 255 / self.sum[3]) as u32).min(255);
        ((a as u32) << 24) | (c(0) << 16) | (c(1) << 8) | c(2)
    }
}

fn channels(p: u32) -> [i64; 4] {
    [
        ((p >> 16) & 0xff) as i64,
        ((p >> 8) & 0xff) as i64,
        (p & 0xff) as i64,
        (p >> 24) as i64,
    ]
}

/// L1 distance, except that a zero vector is only close to another zero vector.
fn dist(a: &[i64; 4], b: &[i64; 4]) -> i64 {
    let (za, zb) = (a.iter().all(|&v| v == 0), b.iter().all(|&v| v == 0));
    if za || zb {
        return if za && zb { 0 } else { FAR };
    }
    (0..4).map(|k| (a[k] - b[k]).abs()).sum()
}

/// Receives the boundary tracer's events in xochitl's order.
pub trait Sink {
    fn union(&mut self, lo: u32, hi: u32);
    fn corner(&mut self, x: i32, y: i32, tl: u32, bl: u32, tr: u32, br: u32);
    fn region(&mut self, r: u32, rec: &Rec);
}

struct State {
    recs: Vec<Rec>,
}

impl State {
    fn find(&self, mut i: u32) -> u32 {
        while self.recs[i as usize].parent != i {
            i = self.recs[i as usize].parent;
        }
        i
    }

    fn cand(&self, r: u32, p: &[i64; 4]) -> [i64; 4] {
        let rec = &self.recs[r as usize];
        if rec.sum.iter().all(|&v| v == 0) {
            return [0; 4];
        }
        let n = rec.nz + 1;
        [0, 1, 2, 3].map(|k| (rec.sum[k] + p[k]) / n)
    }

    fn union(&mut self, lo: u32, hi: u32) {
        let h = self.recs[hi as usize].clone();
        let l = &mut self.recs[lo as usize];
        for k in 0..4 {
            l.sum[k] += h.sum[k];
        }
        l.nz += h.nz;
        l.pending += h.pending;
        for r in self.recs.iter_mut() {
            if r.parent == hi {
                r.parent = lo;
            }
        }
    }
}

/// Label the raster `px` (premultiplied ARGB, `w` x `h`). `class` is the per-pixel merge class
/// (pixels of different classes never share a region); `None` means all zero. The exporter's
/// image starts at `origin` and is `size` pixels; its 1024-pixel tiles and 64-pixel blocks are
/// anchored there. After each tile, `sink` gets the pixel corners that are now complete, with the
/// roots of that moment, then the regions finished so far (highest index first).
pub fn label(
    px: &[u32],
    class: Option<&[u8]>,
    w: i32,
    h: i32,
    origin: (i32, i32),
    size: (i32, i32),
    sink: &mut impl Sink,
) {
    let mut st = State {
        recs: vec![Rec::new(0, 0)],
    };
    let mut label = vec![0u32; (w * h) as usize];
    let (ntx, nty) = ((size.0 + 1023) / 1024, (size.1 + 1023) / 1024);
    let bounds = |bx: i32, by: i32| {
        let (x0, y0) = (origin.0 + bx * 64, origin.1 + by * 64);
        let (x1, y1) = (
            (x0 + 64).min(origin.0 + size.0).min(w),
            (y0 + 64).min(origin.1 + size.1).min(h),
        );
        (x0.max(0), y0.max(0), x1, y1)
    };
    let span = |t: i32, n: i32, len: i32| {
        let lo = if t == 0 { 0 } else { t * 1024 - 64 };
        let hi = if t + 1 == n {
            len + 1
        } else {
            ((t + 1) * 1024).min(len + 1) - 64
        };
        (lo, hi)
    };
    for ty in 0..nty {
        for tx in 0..ntx {
            for by in ty * 16..(ty * 16 + 16).min((size.1 + 63) / 64) {
                for bx in tx * 16..(tx * 16 + 16).min((size.0 + 63) / 64) {
                    let r = bounds(bx, by);
                    if r.0 < r.2 && r.1 < r.3 {
                        label_block(&mut st, &mut label, px, class, w, r, sink);
                    }
                }
            }
            let ((x0, x1), (y0, y1)) = (span(tx, ntx, size.0), span(ty, nty, size.1));
            let clip = |t: i32, n: i32, lo: i32, hi: i32| {
                let t0 = if t == 0 { i32::MIN } else { t * 1024 };
                let t1 = if t + 1 == n {
                    i32::MAX
                } else {
                    t * 1024 + 1024
                };
                (lo.max(t0), hi.min(t1))
            };
            for cty in (y0 / 1024).min(nty - 1)..=((y1 - 1) / 1024).min(nty - 1) {
                for ctx in (x0 / 1024).min(ntx - 1)..=((x1 - 1) / 1024).min(ntx - 1) {
                    let (cx0, cx1) = clip(ctx, ntx, x0, x1);
                    let (cy0, cy1) = clip(cty, nty, y0, y1);
                    if cx0 >= cx1 || cy0 >= cy1 {
                        continue;
                    }
                    for by in cy0.div_euclid(64)..=(cy1 - 1).div_euclid(64) {
                        for bx in cx0.div_euclid(64)..=(cx1 - 1).div_euclid(64) {
                            for y in cy0.max(by * 64)..cy1.min(by * 64 + 64) {
                                for x in cx0.max(bx * 64)..cx1.min(bx * 64 + 64) {
                                    corner(&mut st, &label, w, h, origin.0 + x, origin.1 + y, sink);
                                }
                            }
                        }
                    }
                }
            }
            emit(&mut st, sink);
        }
    }
    for r in st.recs.iter_mut() {
        r.pending = 0;
    }
    emit(&mut st, sink);
}

fn corner(st: &mut State, label: &[u32], w: i32, h: i32, x: i32, y: i32, sink: &mut impl Sink) {
    let at = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= w || y >= h {
            0
        } else {
            st.find(label[(y * w + x) as usize])
        }
    };
    let (tl, bl, tr, br) = (at(x - 1, y - 1), at(x - 1, y), at(x, y - 1), at(x, y));
    if tl != 0 {
        st.recs[tl as usize].pending -= 1;
    }
    if !(tl == bl && bl == tr && tr == br) {
        sink.corner(x, y, tl, bl, tr, br);
    }
}

fn emit(st: &mut State, sink: &mut impl Sink) {
    for r in (1..st.recs.len()).rev() {
        let rec = &st.recs[r];
        if rec.parent == r as u32 && rec.pending == 0 && !rec.emitted {
            sink.region(r as u32, rec);
            st.recs[r].emitted = true;
        }
    }
}

fn label_block(
    st: &mut State,
    label: &mut [u32],
    px: &[u32],
    class: Option<&[u8]>,
    w: i32,
    (x0, y0, x1, y1): (i32, i32, i32, i32),
    sink: &mut impl Sink,
) {
    let start = st.recs.len() as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) as usize;
            let p = channels(px[i]);
            let zero = px[i] == 0;
            let cls = if zero { 0 } else { class.map_or(0, |c| c[i]) };
            let l = if x > 0 { st.find(label[i - 1]) } else { 0 };
            let u = if y > 0 {
                st.find(label[i - w as usize])
            } else {
                0
            };
            let (cl, cu) = (st.recs[l as usize].class, st.recs[u as usize].class);
            let (okl, oku) = (cls == cl, cls == cu);
            let ml = if okl { st.cand(l, &p) } else { [0; 4] };
            let mu = if oku { st.cand(u, &p) } else { [0; 4] };
            let dl = if okl { dist(&ml, &p) } else { FAR };
            let du = if oku { dist(&mu, &p) } else { FAR };
            let up_only = || {
                if l != u && oku && du < THRESHOLD {
                    Some(u)
                } else {
                    None
                }
            };
            let pick = if !okl || dl >= THRESHOLD {
                up_only()
            } else if l != u && cl == cu && du < THRESHOLD {
                if dist(&ml, &mu) >= THRESHOLD {
                    Some(if dl < du { l } else { u })
                } else {
                    let (lo, hi) = (l.min(u), l.max(u));
                    sink.union(lo, hi);
                    st.union(lo, hi);
                    Some(lo)
                }
            } else {
                Some(l)
            };
            let r = pick.unwrap_or_else(|| {
                let n = st.recs.len() as u32;
                st.recs.push(Rec::new(n, cls));
                n
            });
            let rec = &mut st.recs[r as usize];
            for (s, v) in rec.sum.iter_mut().zip(p) {
                *s += v;
            }
            if !zero {
                rec.nz += 1;
            }
            rec.pending += 1;
            label[i] = r;
        }
    }
    let n = st.recs.len() as u32;
    let roots: Vec<u32> = (start..n).map(|i| st.find(i)).collect();
    let mut map: Vec<u32> = (0..n).collect();
    let mut next = start;
    for i in start..n {
        if roots[(i - start) as usize] == i {
            map[i as usize] = next;
            st.recs[next as usize] = st.recs[i as usize].clone();
            st.recs[next as usize].parent = next;
            next += 1;
        }
    }
    st.recs.truncate(next as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) as usize;
            let l = label[i];
            label[i] = if l < start {
                st.find(l)
            } else {
                map[roots[(l - start) as usize] as usize]
            };
        }
    }
}
