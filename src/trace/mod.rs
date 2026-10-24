//! xochitl's raster vectorizer, as used by the PDF exporter: each connected single-color region
//! of the rasterized page becomes one filled path. The region labeling is in `label`, the
//! boundary tracing in `boundary` and the smoothing in `smooth`.
//!
//! Coordinates are integers in 1/8 device pixels; pixel `x` spans `[8x, 8x + 8]`.

mod boundary;
mod label;
mod smooth;

/// Sub-pixel precision of traced coordinates.
pub(crate) const P: i32 = 8;
/// A point in 1/`P` pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Pt {
    pub x: i32,
    pub y: i32,
}

impl Pt {
    pub const fn new(x: i32, y: i32) -> Pt {
        Pt { x, y }
    }

    /// The point with x and y swapped.
    pub fn transpose(self) -> Pt {
        Pt::new(self.y, self.x)
    }
}

impl std::ops::Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
}

impl std::ops::AddAssign for Pt {
    fn add_assign(&mut self, o: Pt) {
        *self = *self + o;
    }
}

impl std::ops::Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
}

/// One filled path of the export: a connected single-color region with its holes.
pub struct Group {
    /// Straight ARGB.
    pub color: u32,
    /// From shading tools only (labeler class 1): written with the `Darken` blend mode.
    pub darken: bool,
    /// Outer contour first, then the holes; 1/8 device pixels, closed (first == last).
    pub subpaths: Vec<Vec<Pt>>,
}

/// Trace an ARGB32 premultiplied image (`w` x `h`, row-major) into filled regions, in
/// xochitl's output order. `image` is the exporter's image (origin and size) in these pixel
/// coordinates: the 64-pixel tracing blocks and the 1024-pixel tiles are anchored there.
/// `class` marks the pixels only shading tools drew; they never merge with other pixels.
pub fn vectorize(
    px: &[u32],
    class: Option<&[u8]>,
    w: i32,
    h: i32,
    image: ((i32, i32), (i32, i32)),
) -> Vec<Group> {
    struct Out {
        t: boundary::Tracer,
        cfg: boundary::Config,
        groups: Vec<Group>,
    }
    impl label::Sink for Out {
        fn union(&mut self, lo: u32, hi: u32) {
            self.t.union(lo, hi);
        }
        fn corner(&mut self, x: i32, y: i32, tl: u32, bl: u32, tr: u32, br: u32) {
            self.t.corner(x, y, tl, bl, tr, br);
        }
        // Regions without color are traced (their shared boundaries are consumed) but not
        // output.
        fn region(&mut self, r: u32, rec: &label::Rec) {
            let subpaths = self.t.region(r, &self.cfg);
            let color = rec.argb();
            if color != 0 {
                self.groups.push(Group {
                    color,
                    darken: rec.class != 0,
                    subpaths,
                });
            }
        }
    }
    let cfg = boundary::Config {
        q: 16,
        smooth: true,
    };
    let mut out = Out {
        t: boundary::Tracer::new(0),
        cfg,
        groups: Vec::new(),
    };
    label::label(px, class, w, h, image.0, image.1, &mut out);
    out.groups
}
