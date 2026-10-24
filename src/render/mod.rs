//! Stroke rasterization following xochitl's `strokev2` pipelines, at the settings the PDF
//! exporter uses (no antialiasing, 32-bit ARGB, device pixels).

mod ballpoint;
mod brush;
mod color;
mod geom;
mod paper;
mod pencil;
pub(crate) mod raster;
mod shader;
mod solid;
mod stroker;

pub(crate) use color::{premultiply, unpremultiply};
pub use geom::{Offsets, anchor_offsets, content_rect};
pub use paper::{DPI, Paper};

use crate::format::{ItemValue, Line, Node, Rect, Scene, Tool};
use geom::offset;

/// Premultiplied ARGB32 image; 0 is transparent.
pub struct Canvas {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
    /// Where the pipelines' pixel coordinates land: canvas = pixel + `off`, for pixels inside
    /// `clip` (x0, y0, x1, y1 in pixel coordinates).
    pub(crate) view: View,
}

#[derive(Clone, Copy)]
pub(crate) struct View {
    pub off: (i32, i32),
    pub clip: (i32, i32, i32, i32),
}

impl Canvas {
    pub fn new(w: u32, h: u32) -> Self {
        Canvas {
            w,
            h,
            px: vec![0; (w * h) as usize],
            view: View {
                off: (0, 0),
                clip: (0, 0, w as i32, h as i32),
            },
        }
    }

    /// Set pixel (x, y) of the current view.
    #[inline]
    pub(crate) fn put(&mut self, x: i32, y: i32, c: u32) {
        let (x0, y0, x1, y1) = self.view.clip;
        if x < x0 || y < y0 || x >= x1 || y >= y1 {
            return;
        }
        let (x, y) = (x + self.view.off.0, y + self.view.off.1);
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        self.px[(y as u32 * self.w + x as u32) as usize] = c;
    }

    /// Pixel (x, y) of the current view, if it is inside.
    #[inline]
    pub(crate) fn get(&self, x: i32, y: i32) -> Option<u32> {
        let (x0, y0, x1, y1) = self.view.clip;
        if x < x0 || y < y0 || x >= x1 || y >= y1 {
            return None;
        }
        let (x, y) = (x + self.view.off.0, y + self.view.off.1);
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return None;
        }
        Some(self.px[(y as u32 * self.w + x as u32) as usize])
    }

    pub fn write_png(&self, path: &str) -> std::io::Result<()> {
        let f = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut e = png::Encoder::new(f, self.w, self.h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().map_err(std::io::Error::other)?;
        let mut data = Vec::with_capacity(self.px.len() * 4);
        for &p in &self.px {
            data.extend_from_slice(&color::unpremultiply_rgba(p));
        }
        w.write_image_data(&data).map_err(std::io::Error::other)
    }
}

impl Tool {
    fn margin_factor(self, thickness_scale: f64) -> f32 {
        use Tool::*;
        match self {
            Paintbrush | Pencil => 50.0,
            Ballpoint | BallpointV2 | SolidPen | ReservedPen => 2.0,
            Marker | MarkerV2 => 20.0,
            Fineliner | SharpPencil | EraseSection | SelectionTool | FinelinerV2 => 3.0,
            Highlighter | HighlighterV2 => 30.0,
            Eraser | MaskedEraser => (thickness_scale * 10.0) as f32,
            ZoomTool | ShadingMarker => 10.0,
            PaintbrushV2 => 15.0,
            SharpPencilV2 => 4.0,
            PencilV2 => 18.0,
            Calligraphy => 6.0,
            _ => 1.0,
        }
    }

    /// Whether `PageVectorizer::processRange` passes the tool's lines through as vector strokes
    /// instead of tracing them (fineliners and highlighters).
    pub fn is_direct(self) -> bool {
        matches!(self, Tool::Fineliner | Tool::FinelinerV2) || self.is_highlighter()
    }

    /// The tools the exporter's class pass leaves out (highlighters and the shader).
    pub fn is_shading(self) -> bool {
        self.is_highlighter() || self == Tool::ShadingMarker
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Transform {
    s: f64,
    dx: f64,
    dy: f64,
}

impl Transform {
    pub fn map(&self, x: f32, y: f32) -> (f32, f32) {
        (
            (x as f64 * self.s + self.dx) as f32,
            (y as f64 * self.s + self.dy) as f32,
        )
    }
}

pub struct Renderer {
    pub canvas: Canvas,
    /// Device pixels per scene unit.
    pub(crate) scale: f32,
    /// Canvas pixel of the scene origin before [`Renderer::align_to`]: the page's top center,
    /// unless the canvas extends past the page.
    pub(crate) center: (i32, i32),
    /// Device position of scene origin.
    pub(crate) dx: f64,
    pub(crate) dy: f64,
    pub warnings: Vec<String>,
    /// When set, lines the exporter emits as stroked vectors (see [`Tool::is_direct`]) are collected
    /// here instead of being rasterized.
    /// Each with the scene offset of its group.
    pub(crate) direct: Option<Vec<(Line, (f32, f32))>>,
    pub offsets: Offsets,
    /// Leave out the shading tools (highlighters and the shading marker): the exporter's class pass.
    pub(crate) no_shading: bool,
    /// Offset of the group being drawn.
    origin: (f32, f32),
    /// Set by [`Renderer::align_to`]: the exporter's raster, drawn in 1024-pixel tiles.
    raster: Option<Raster>,
    /// Raster position of the tile being drawn.
    tile: (i32, i32),
}

#[derive(Clone, Copy)]
struct Raster {
    corner: (f64, f64),
    origin: (i32, i32),
    /// `ceil(scale * rect size) + 128`.
    size: (i32, i32),
}

impl Renderer {
    pub fn new(w: u32, h: u32) -> Self {
        Renderer {
            canvas: Canvas::new(w, h),
            center: (w as i32 / 2, 0),
            scale: 1.0,
            dx: w as f64 / 2.0,
            dy: 0.0,
            warnings: Vec::new(),
            direct: None,
            no_shading: false,
            offsets: Offsets::new(),
            origin: (0.0, 0.0),
            raster: None,
            tile: (0, 0),
        }
    }

    pub(crate) fn draw_line(&mut self, line: &Line) {
        if let Some(d) = self.direct.as_mut().filter(|_| line.tool.is_direct()) {
            d.push((line.clone(), self.origin));
            return;
        }
        let Some(ras) = self.raster else {
            return self.draw_tool(line);
        };
        let (w, h) = (self.canvas.w as i32, self.canvas.h as i32);
        let mut ty = 0;
        while ty < ras.size.1 {
            let mut tx = 0;
            while tx < ras.size.0 {
                self.tile = (tx, ty);
                let clip = (
                    0,
                    0,
                    (ras.size.0 - tx).min(1024),
                    (ras.size.1 - ty).min(1024),
                );
                self.canvas.view = View {
                    off: (ras.origin.0 + tx, ras.origin.1 + ty),
                    clip,
                };
                self.draw_tool(line);
                tx += 1024;
            }
            ty += 1024;
        }
        self.canvas.view = View {
            off: (0, 0),
            clip: (0, 0, w, h),
        };
    }

    fn draw_tool(&mut self, line: &Line) {
        if self.no_shading && line.tool.is_shading() {
            return;
        }
        use Tool::*;
        match line.tool {
            BallpointV2 => ballpoint::draw(self, line),
            PaintbrushV2 | MarkerV2 => brush::draw(self, line),
            ShadingMarker => shader::draw(self, line),
            SharpPencilV2 | PencilV2 => pencil::draw(self, line),
            Fineliner | Highlighter | FinelinerV2 | HighlighterV2 | Calligraphy | SolidPen
            | ReservedPen => solid::draw(self, line),
            t => {
                let tr = self.transform();
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for p in &line.points {
                    let (x, y) = tr.map(p.x, p.y);
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                }
                self.warnings.push(format!(
                    "tool {} not implemented ({x0:.0},{y0:.0})-({x1:.0},{y1:.0})",
                    t.id()
                ))
            }
        }
    }

    /// The device box strokes are clipped to: the 1024-pixel tile image, or the whole canvas.
    pub(crate) fn clip_box(&self) -> raster::ClipBox {
        match self.raster {
            Some(_) => [0.0, 1024.0, 0.0, 1024.0],
            None => [0.0, self.canvas.w as f32, 0.0, self.canvas.h as f32],
        }
    }

    /// Scene to device mapping for the line being drawn, evaluated in double precision like
    /// `QTransform::map`.
    pub(crate) fn transform(&self) -> Transform {
        let s = self.scale as f64;
        let (dx, dy) = match self.raster {
            Some(r) => (
                (64 - self.tile.0) as f64 + -r.corner.0 * s,
                (64 - self.tile.1) as f64 + -r.corner.1 * s,
            ),
            None => (self.dx, self.dy),
        };
        Transform {
            s,
            dx: dx + self.origin.0 as f64 * s,
            dy: dy + self.origin.1 as f64 * s,
        }
    }

    pub(crate) fn draw_group(&mut self, sc: &Scene, n: &Node) {
        if !n.visible.value {
            return;
        }
        let saved = self.origin;
        self.origin = offset(&self.offsets, n, saved);
        for c in &n.children {
            let Some(it) = sc.item(n.id, *c) else {
                continue;
            };
            match it.value.as_ref() {
                Some(ItemValue::Group(g)) => {
                    if let Some(child) = sc.nodes.get(g) {
                        self.draw_group(sc, child);
                    }
                }
                Some(ItemValue::Line(l)) => self.draw_line(l),
                _ => {}
            }
        }
        self.origin = saved;
    }

    /// Shift geometry the way the PDF exporter does. It rasterizes with the content rect's corner
    /// (scaled) at device pixel 64, but maps traced vectors back by the *truncated* corner
    /// so output = scene - fract(scale * rect) with pixels on the integer page grid. Returns that shift.
    pub fn align_to(&mut self, rect: Rect) -> (f32, f32) {
        let s = self.scale as f64;
        let (rx, ry) = (s * rect.x, s * rect.y);
        let (fx, fy) = (rx - (rx as i32) as f64, ry - (ry as i32) as f64);
        self.dx = self.center.0 as f64 - fx;
        self.dy = self.center.1 as f64 - fy;
        let origin = (
            self.center.0 + rx as i32 - 64,
            self.center.1 + ry as i32 - 64,
        );
        let size = (
            (s * rect.w).ceil() as i32 + 128,
            (s * rect.h).ceil() as i32 + 128,
        );
        self.raster = Some(Raster {
            corner: (rect.x, rect.y),
            origin,
            size,
        });
        (-fx as f32, -fy as f32)
    }

    /// The exporter's image set up by [`Renderer::align_to`]: canvas position and size.
    pub fn image(&self) -> Option<((i32, i32), (i32, i32))> {
        self.raster.map(|r| (r.origin, r.size))
    }

    pub fn draw_scene(&mut self, sc: &Scene) {
        if let Some(root) = sc.nodes.get(&crate::format::ROOT_ID) {
            self.draw_group(sc, root);
        }
    }
}
