//! The decoded scene: a tree of groups (layers are the children of the root group) holding CRDT
//! sequences of items, plus the root text. Decoding is in `parse`.

use std::collections::{BTreeMap, HashMap};

use crate::format::crdt::{CrdtId, Item, Lww, ROOT_ID};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub speed: u16,
    pub width: u16,
    pub direction: u8,
    pub pressure: u8,
}

/// Pen tool of a line item, named as xochitl's `Line::Tool` enum (Qt metadata of `Line`). Ids
/// outside it are kept as `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    Paintbrush,
    Pencil,
    Ballpoint,
    Marker,
    Fineliner,
    Highlighter,
    Eraser,
    /// Mechanical pencil.
    SharpPencil,
    /// Area eraser.
    EraseSection,
    ClearPage,
    ZoomTool,
    SelectionTool,
    PaintbrushV2,
    /// Mechanical pencil.
    SharpPencilV2,
    PencilV2,
    BallpointV2,
    MarkerV2,
    FinelinerV2,
    HighlighterV2,
    SolidPen,
    ReservedPen,
    Calligraphy,
    MaskedEraser,
    /// The shader.
    ShadingMarker,
    Undefined,
    Other(u32),
}

impl Tool {
    pub fn from_id(id: u32) -> Tool {
        match id {
            0 => Tool::Paintbrush,
            1 => Tool::Pencil,
            2 => Tool::Ballpoint,
            3 => Tool::Marker,
            4 => Tool::Fineliner,
            5 => Tool::Highlighter,
            6 => Tool::Eraser,
            7 => Tool::SharpPencil,
            8 => Tool::EraseSection,
            9 => Tool::ClearPage,
            10 => Tool::ZoomTool,
            11 => Tool::SelectionTool,
            12 => Tool::PaintbrushV2,
            13 => Tool::SharpPencilV2,
            14 => Tool::PencilV2,
            15 => Tool::BallpointV2,
            16 => Tool::MarkerV2,
            17 => Tool::FinelinerV2,
            18 => Tool::HighlighterV2,
            19 => Tool::SolidPen,
            20 => Tool::ReservedPen,
            21 => Tool::Calligraphy,
            22 => Tool::MaskedEraser,
            23 => Tool::ShadingMarker,
            24 => Tool::Undefined,
            id => Tool::Other(id),
        }
    }

    /// The id stored in the file.
    pub fn id(self) -> u32 {
        match self {
            Tool::Paintbrush => 0,
            Tool::Pencil => 1,
            Tool::Ballpoint => 2,
            Tool::Marker => 3,
            Tool::Fineliner => 4,
            Tool::Highlighter => 5,
            Tool::Eraser => 6,
            Tool::SharpPencil => 7,
            Tool::EraseSection => 8,
            Tool::ClearPage => 9,
            Tool::ZoomTool => 10,
            Tool::SelectionTool => 11,
            Tool::PaintbrushV2 => 12,
            Tool::SharpPencilV2 => 13,
            Tool::PencilV2 => 14,
            Tool::BallpointV2 => 15,
            Tool::MarkerV2 => 16,
            Tool::FinelinerV2 => 17,
            Tool::HighlighterV2 => 18,
            Tool::SolidPen => 19,
            Tool::ReservedPen => 20,
            Tool::Calligraphy => 21,
            Tool::MaskedEraser => 22,
            Tool::ShadingMarker => 23,
            Tool::Undefined => 24,
            Tool::Other(id) => id,
        }
    }

    pub fn is_highlighter(self) -> bool {
        matches!(self, Tool::Highlighter | Tool::HighlighterV2)
    }
}

/// Color of a line or glyph range, named as xochitl's `Line::Color` enum (Qt metadata of `Line`).
/// Values outside it are kept as `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PenColor {
    Black,
    Gray,
    White,
    HighlighterYellow,
    HighlighterGreen,
    HighlighterPink,
    Blue,
    Red,
    HighlighterGray,
    /// The color is the explicit ARGB value stored with the item.
    ArgbCode,
    Green,
    Cyan,
    Magenta,
    Yellow,
    Other(u32),
}

impl PenColor {
    pub fn from_id(id: u32) -> PenColor {
        match id {
            0 => PenColor::Black,
            1 => PenColor::Gray,
            2 => PenColor::White,
            3 => PenColor::HighlighterYellow,
            4 => PenColor::HighlighterGreen,
            5 => PenColor::HighlighterPink,
            6 => PenColor::Blue,
            7 => PenColor::Red,
            8 => PenColor::HighlighterGray,
            9 => PenColor::ArgbCode,
            10 => PenColor::Green,
            11 => PenColor::Cyan,
            12 => PenColor::Magenta,
            13 => PenColor::Yellow,
            id => PenColor::Other(id),
        }
    }

    /// The value stored in the file.
    pub fn id(self) -> u32 {
        match self {
            PenColor::Black => 0,
            PenColor::Gray => 1,
            PenColor::White => 2,
            PenColor::HighlighterYellow => 3,
            PenColor::HighlighterGreen => 4,
            PenColor::HighlighterPink => 5,
            PenColor::Blue => 6,
            PenColor::Red => 7,
            PenColor::HighlighterGray => 8,
            PenColor::ArgbCode => 9,
            PenColor::Green => 10,
            PenColor::Cyan => 11,
            PenColor::Magenta => 12,
            PenColor::Yellow => 13,
            PenColor::Other(id) => id,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub tool: Tool,
    pub color: PenColor,
    pub thickness_scale: f64,
    pub starting_length: f32,
    pub points: Vec<Point>,
    pub id6: Option<CrdtId>,
    pub id7: Option<CrdtId>,
    /// Explicit ARGB color, present when `color` is `ArgbCode`.
    pub argb: Option<u32>,
    pub extra: Vec<u8>,
}

/// A rectangle: top left corner and size.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    /// The smallest rect holding both.
    pub fn union(self, o: Rect) -> Rect {
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = self.right().max(o.right());
        let y1 = self.bottom().max(o.bottom());
        Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }

    pub fn translate(self, dx: f64, dy: f64) -> Rect {
        Rect {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }
}

/// Highlight on the text of a PDF/EPUB background page.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphRange {
    pub start: Option<u32>,
    pub length: u32,
    pub color: PenColor,
    pub text: String,
    pub rects: Vec<Rect>,
    pub argb: Option<u32>,
    pub extra: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemValue {
    /// Reference to a child group node.
    Group(CrdtId),
    Line(Line),
    Glyph(GlyphRange),
    /// Item kind this reader does not decode yet: (kind byte, raw value).
    Unknown(u8, Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: CrdtId,
    pub label: Lww<String>,
    pub visible: Lww<bool>,
    pub anchor_id: Option<Lww<CrdtId>>,
    pub anchor_type: Option<Lww<u8>>,
    pub anchor_threshold: Option<Lww<f32>>,
    pub anchor_origin_x: Option<Lww<f32>>,
    pub extra: Vec<u8>,
    /// Children in sequence (z) order, deleted items removed. Filled by `Scene::finish`.
    pub children: Vec<CrdtId>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextValue {
    Str(String),
    /// A formatting code (u32) embedded in the character sequence.
    Format(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RootText {
    pub items: Vec<Item<TextValue>>,
    /// Paragraph styles keyed by the id of the character that starts the paragraph
    /// (`END_MARKER` = first paragraph).
    pub styles: BTreeMap<CrdtId, Lww<ParagraphStyle>>,
    pub x: f64,
    pub y: f64,
    pub width: f32,
    pub extra: Vec<u8>,
}

/// A paragraph's style as stored: the legacy code (`style::*`), and since 3.29 the
/// `scene::ParagraphStyle` type and bits it stands for (needed for the styles without a legacy code, such as
/// Subheading2).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ParagraphStyle {
    pub code: u8,
    /// Field 2, the type: 1 basic, 2 text, 3 list, 4 image.
    pub kind: Option<u8>,
    /// Field 3: for text the heading level (0 body, 1 title, 2 subheading, 3 subheading2); for lists
    /// bit 0 indented, bits 4-5 kind (0 bullet, 1 checkbox, 2 numbered), bit 8 checked.
    pub bits: Option<u32>,
}

impl ParagraphStyle {
    /// The block style: from the type and bits, or from the legacy code when the file has none
    pub fn block(&self) -> Block {
        match (self.kind, self.bits) {
            (Some(kind), Some(bits)) => Block::from_type(kind, bits),
            _ => Block::from_code(self.code),
        }
    }
}

/// Paragraph style codes of the root text (the legacy encoding).
pub mod style {
    pub const BASIC: u8 = 0;
    pub const PLAIN: u8 = 1;
    pub const HEADING: u8 = 2;
    pub const BOLD: u8 = 3;
    pub const BULLET: u8 = 4;
    pub const BULLET2: u8 = 5;
    pub const CHECKBOX: u8 = 6;
    pub const CHECKBOX_CHECKED: u8 = 7;
    pub const CHECKBOX2: u8 = 8;
    pub const CHECKBOX2_CHECKED: u8 = 9;
    pub const NUMBERED: u8 = 10;
    pub const NUMBERED2: u8 = 11;
    pub const IMAGE: u8 = 12;
}

/// A paragraph's block style (`scene::ParagraphStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    Basic,
    Body,
    Title,
    Subheading,
    Subheading2,
    /// `indented`: the second level.
    List {
        kind: List,
        indented: bool,
    },
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum List {
    Bullet,
    Checkbox,
    Checked,
    Numbered,
}

impl Block {
    /// From the style's type (1 basic, 2 text, 3 list, 4 image) and bits.
    fn from_type(kind: u8, bits: u32) -> Block {
        match (kind, bits) {
            (2, 0) => Block::Body,
            (2, 1) => Block::Title,
            (2, 2) => Block::Subheading,
            (2, 3) => Block::Subheading2,
            (3, bits) => Block::List {
                kind: match ((bits >> 4) & 3, (bits >> 8) & 1) {
                    (0, _) => List::Bullet,
                    (1, 0) => List::Checkbox,
                    (1, _) => List::Checked,
                    _ => List::Numbered,
                },
                indented: bits & 1 != 0,
            },
            (4, _) => Block::Image,
            _ => Block::Basic,
        }
    }

    /// From a legacy style code.
    fn from_code(code: u8) -> Block {
        let list = |kind, indented| Block::List { kind, indented };
        match code {
            style::PLAIN => Block::Body,
            style::HEADING => Block::Title,
            style::BOLD => Block::Subheading,
            style::BULLET => list(List::Bullet, false),
            style::BULLET2 => list(List::Bullet, true),
            style::CHECKBOX => list(List::Checkbox, false),
            style::CHECKBOX_CHECKED => list(List::Checked, false),
            style::CHECKBOX2 => list(List::Checkbox, true),
            style::CHECKBOX2_CHECKED => list(List::Checked, true),
            style::NUMBERED => list(List::Numbered, false),
            style::NUMBERED2 => list(List::Numbered, true),
            style::IMAGE => Block::Image,
            _ => Block::Basic,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SceneInfo {
    pub current_layer: Option<Lww<CrdtId>>,
    pub background_visible: Option<Lww<bool>>,
    pub root_document_visible: Option<Lww<bool>>,
    pub paper_size: Option<(u32, u32)>,
    pub extra: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub author_ids: Vec<(String, u16)>,
    pub info: SceneInfo,
    pub page_info: Vec<u32>,
    pub nodes: HashMap<CrdtId, Node>,
    /// Items of each group node, keyed by parent id, in file order.
    pub items: HashMap<CrdtId, Vec<Item<ItemValue>>>,
    pub text: Option<RootText>,
    pub warnings: Vec<String>,
}

impl Scene {
    pub fn item(&self, parent: CrdtId, id: CrdtId) -> Option<&Item<ItemValue>> {
        self.items.get(&parent)?.iter().find(|i| i.id == id)
    }

    /// The layer groups (children of the root) in z order.
    pub fn layers(&self) -> Vec<&Node> {
        let Some(root) = self.nodes.get(&ROOT_ID) else {
            return Vec::new();
        };
        root.children
            .iter()
            .filter_map(|c| match self.item(ROOT_ID, *c)?.value.as_ref()? {
                ItemValue::Group(g) => self.nodes.get(g),
                _ => None,
            })
            .collect()
    }
}
