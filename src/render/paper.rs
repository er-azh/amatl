//! Paper sizes.
use crate::format::Scene;
use crate::rmdoc::Orientation;

/// Scene units (device pixels) per inch, on every device.
pub const DPI: f64 = 227.0;

/// A page's paper in scene units (227 dpi on every device), portrait. Scene x = 0 is the
/// horizontal page center.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paper {
    pub w: u32,
    pub h: u32,
}

impl Paper {
    /// reMarkable 1 and 2, and the size assumed when a page doesn't say.
    pub const RM2: Paper = Paper { w: 1404, h: 1872 };

    /// The scene's paper size (`SceneInfo`), if it stores one.
    pub fn of(sc: &Scene) -> Option<Paper> {
        let (a, b) = sc.info.paper_size?;
        (a > 0 && b > 0).then(|| Paper {
            w: a.min(b),
            h: a.max(b),
        })
    }

    /// The page's width and height in either orientation.
    pub fn page(self, orientation: Orientation) -> (u32, u32) {
        match orientation {
            Orientation::Portrait => (self.w, self.h),
            Orientation::Landscape => (self.h, self.w),
        }
    }

    /// Screen dpi of the device this paper belongs to (`physicalDpi` in JSON templates): the
    /// Paper Pro's 229 for its paper, the reMarkable 2's configured 226 otherwise.
    pub fn dpi(self) -> f64 {
        if self == (Paper { w: 1620, h: 2160 }) {
            229.0
        } else {
            226.0
        }
    }
}
