//! Layout of the root text as xochitl lays it out.
//!
//! - Document margin 4 px on every side; the text width includes it.
//! - Each block adds max(its top margin, previous bottom margin = 0) before its lines, except the
//!   first block.
//! - Per line: baseline = y + ascent; y += ceil(ascent + descent) + the block's line spacing
//!   (`LineDistanceHeight`, 0.2 x font height for plain paragraphs).
//! - Advances: FreeType's unhinted linear advance truncated to 26.6, plus the HarfBuzz (GPOS)
//!   adjustment rounded to 26.6.

mod fixed;
mod fonts;
mod layout;
mod shape;
mod style;

pub use fixed::{Fixed, to_px};
pub use fonts::{Face, Fonts};
pub use layout::{Glyph, Layout, Line, Rule, layout};
pub(crate) use shape::shape_run;
pub use style::Format;
