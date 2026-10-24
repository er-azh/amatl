//! Page templates (`SceneTemplateRenderer`) as the QPainter draw calls xochitl's exporter makes:
//! the built-in patterns it draws in code, and JSON `.template` files (`json`).

mod builtin;
mod image;
pub mod json;

pub use image::{Image, Mask};

/// A pen as Qt's PDF engine writes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pen {
    /// Non-premultiplied ARGB.
    pub color: u32,
    pub width: f64,
    /// PDF line cap / join codes.
    pub cap: u8,
    pub join: u8,
    /// Dash pattern as written to the PDF (already scaled by the pen width); empty for solid.
    pub dash: Vec<f64>,
}

/// QPainter's default pen, which restoring the painter at the end writes.
pub const DEFAULT_PEN: Pen = Pen {
    color: 0xff00_0000,
    width: 1.0,
    cap: 2,
    join: 2,
    dash: vec![],
};

#[derive(Clone, Copy, Debug)]
pub enum Seg {
    M(f64, f64),
    L(f64, f64),
    C(f64, f64, f64, f64, f64, f64),
    Z,
}

impl Seg {
    /// The segment moved by (dx, dy).
    pub fn offset(self, dx: f64, dy: f64) -> Seg {
        match self {
            Seg::M(x, y) => Seg::M(x + dx, y + dy),
            Seg::L(x, y) => Seg::L(x + dx, y + dy),
            Seg::C(x1, y1, x2, y2, x, y) => {
                Seg::C(x1 + dx, y1 + dy, x2 + dx, y2 + dy, x + dx, y + dy)
            }
            Seg::Z => Seg::Z,
        }
    }
}

/// A run of template text, positioned glyph by glyph as Qt's `drawText` writes it.
#[derive(Clone, Debug)]
pub struct TextRun {
    /// PostScript name of the font.
    pub font: &'static str,
    pub data: std::sync::Arc<Vec<u8>>,
    pub px: u32,
    /// Baseline origin in the draw's frame.
    pub x: f64,
    pub y: f64,
    pub color: u32,
    /// Glyph id, x relative to the origin, and the characters it stands for.
    pub glyphs: Vec<(u16, f64, String)>,
    /// Ink extent (left, right) relative to the origin.
    pub ink: (f64, f64),
}

impl TextRun {
    /// `QFontMetrics::boundingRect(text).width()`, approximated from the outlines.
    pub fn ink_width(&self) -> i32 {
        (self.ink.1.ceil() - self.ink.0.floor()) as i32
    }
}

#[derive(Clone, Debug)]
pub enum Kind {
    Path {
        fill: Option<u32>,
        segs: Vec<Seg>,
    },
    Text(TextRun),
    /// `QPainter::drawImage(rect, image)`: the image stretched over `rect` (x, y, w, h).
    Image {
        image: std::sync::Arc<Image>,
        rect: [f64; 4],
    },
    /// A state change Qt's engine writes without drawing (its pen always written).
    Flush,
}

/// One QPainter draw call of the template.
#[derive(Clone, Debug)]
pub struct Draw {
    /// The painter's transform (a b c d e f) into device pixels.
    pub m: [f64; 6],
    /// The transform was set since the previous draw (Qt's engine then restarts its `q` block).
    pub block: bool,
    pub pen: Option<Pen>,
    pub kind: Kind,
}

/// The template `name` over a `w` x `h` px page of `paper`: a built-in one, else
/// `NAME.template` or `NAME.png` in `dir`, if given.
pub fn load(
    name: &str,
    w: f64,
    h: f64,
    paper: crate::render::Paper,
    dir: Option<&std::path::Path>,
    fonts: &mut crate::text::Fonts,
) -> Option<Vec<Draw>> {
    if let Some(d) = builtin::builtin(name, w, h, builtin::scene(paper)) {
        return Some(d);
    }
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return None;
    }
    let dir = dir?;
    if let Ok(json) = std::fs::read_to_string(dir.join(format!("{name}.template"))) {
        return json::render(&json, w, h, paper, fonts);
    }
    let png = std::fs::read(dir.join(format!("{name}.png"))).ok()?;
    Some(image::draws(
        std::sync::Arc::new(Image::from_png(&png)?),
        w,
        h,
    ))
}
