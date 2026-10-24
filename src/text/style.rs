//! Paragraph styles: the block and character format of each.

use super::fixed::Fixed;
use super::fonts::Face;
use crate::format::Block;

/// Indent of list paragraphs (bullet, checkbox, numbered) per level.
const INDENT: Fixed = 48 * 64;

/// Character and block format of one paragraph style.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Format {
    pub face: Face,
    pub px: u32,
    pub top_margin: Fixed,
    /// Extra line distance as a fraction of the font height.
    pub line_spacing: f64,
    pub indent: Fixed,
}

/// Inline character format flags, switched by the format codes in the text: 1 / 2 start / end
/// bold, 3 / 4 start / end italic.
pub const BOLD: u8 = 1;

pub const ITALIC: u8 = 2;

impl Format {
    /// The paragraph's face with inline bold / italic.
    pub fn font(&self, flags: u8) -> Face {
        if flags == 0 {
            return self.face;
        }
        Face::styled(self.face.is_serif(), flags & BOLD != 0, flags & ITALIC != 0)
    }
}

/// The block formats. The top margins are the ones the layout ends up with,
/// measured on samples; the formats set larger ones (pixel size times
/// 2.75, 1.75 and 1.25 for title, subheading2, subheading).
pub fn format(block: Block) -> Format {
    let plain = Format {
        face: Face::SansRegular,
        px: 32,
        top_margin: 24 * 64,
        line_spacing: 0.2,
        indent: 0,
    };
    let serif = |px, top_margin| Format {
        face: Face::SerifRegular,
        px,
        top_margin,
        line_spacing: 0.0,
        indent: 0,
    };
    match block {
        Block::Title => serif(64, 88 * 64),
        Block::Subheading2 => serif(46, 56 * 64),
        Block::Subheading => Format {
            face: Face::SansMedium,
            px: 36,
            top_margin: 40 * 64,
            line_spacing: 0.0,
            indent: 0,
        },
        Block::List { indented, .. } => Format {
            indent: if indented { 2 * INDENT } else { INDENT },
            ..plain
        },
        Block::Basic | Block::Body | Block::Image => plain,
    }
}
