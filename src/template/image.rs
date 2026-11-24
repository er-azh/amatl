use super::{DEFAULT_PEN, Draw, Kind, Pen};

/// An image as `QPdfEngine::addImage` writes it: JPEG at quality 94, with the alpha channel as a
/// soft mask (some alpha strictly between 0 and 255) or a stencil mask (only 0 or 255).
#[derive(Debug)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub jpeg: Vec<u8>,
    pub mask: Option<Mask>,
}

#[derive(Debug)]
pub enum Mask {
    Soft(Vec<u8>),
    Stencil(Vec<u8>),
}

/// A PNG decoded as Qt does (8 bits a channel): size, resolution in dpi and straight RGBA pixels.
pub(crate) struct Png {
    pub w: usize,
    pub h: usize,
    pub dpi: (u16, u16),
    pub px: Vec<[u8; 4]>,
    /// Truecolor without transparency, which Qt loads as `QImage::Format_RGB32`.
    pub rgb32: bool,
}

impl Png {
    pub fn decode(data: &[u8]) -> Option<Png> {
        let mut dec = png::Decoder::new(std::io::Cursor::new(data));
        dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut r = dec.read_info().ok()?;
        let rgb32 = r.info().color_type == png::ColorType::Rgb && r.info().trns.is_none();
        // QImage's resolution: the pHYs chunk if in meters, else 96 dpi.
        let dpi = match r.info().pixel_dims {
            Some(png::PixelDimensions {
                xppu,
                yppu,
                unit: png::Unit::Meter,
            }) => {
                let d = |v: u32| (v as f64 * 0.0254).round() as u16;
                (d(xppu), d(yppu))
            }
            _ => (96, 96),
        };
        let mut buf = vec![0; r.output_buffer_size()?];
        let f = r.next_frame(&mut buf).ok()?;
        let (w, h) = (f.width as usize, f.height as usize);
        let n = f.color_type.samples();
        let px = buf[..w * h * n]
            .chunks_exact(n)
            .map(|p| match n {
                1 => [p[0], p[0], p[0], 255],
                2 => [p[0], p[0], p[0], p[1]],
                3 => [p[0], p[1], p[2], 255],
                _ => [p[0], p[1], p[2], p[3]],
            })
            .collect();
        Some(Png {
            w,
            h,
            dpi,
            px,
            rgb32,
        })
    }
}

impl Image {
    /// Decode a PNG as Qt does and encode it for the PDF (the JPEG gets the PNG's dpi).
    pub fn from_png(data: &[u8]) -> Option<Image> {
        let Png { w, h, dpi, px, .. } = Png::decode(data)?;
        let mut rgb = Vec::with_capacity(w * h * 3);
        let mut alpha = Vec::with_capacity(w * h);
        for p in px {
            rgb.extend_from_slice(&p[..3]);
            alpha.push(p[3]);
        }
        let jpeg = crate::pdf::jpeg::encode(&rgb, w, h, 94, dpi);
        let mask = if alpha.iter().any(|&a| a != 0 && a != 255) {
            Some(Mask::Soft(alpha))
        } else if alpha.iter().any(|&a| a != 255) {
            let bpl = w.div_ceil(8);
            let mut m = vec![0u8; bpl * h];
            for (i, &a) in alpha.iter().enumerate() {
                if a != 0 {
                    m[i / w * bpl + i % w / 8] |= 0x80 >> (i % w % 8);
                }
            }
            Some(Mask::Stencil(m))
        } else {
            None
        };
        Some(Image {
            w: w as u32,
            h: h as u32,
            jpeg,
            mask,
        })
    }
}

pub fn draws(image: std::sync::Arc<Image>, w: f64, h: f64) -> Vec<Draw> {
    let gray = Pen {
        color: 0xffc0_c0c0,
        width: 0.5,
        cap: 2,
        join: 2,
        dash: vec![],
    };
    let m = [1.0, 0.0, 0.0, 1.0, w / 2.0, 0.0];
    vec![
        Draw {
            m,
            block: true,
            pen: Some(gray.clone()),
            kind: Kind::Flush,
        },
        Draw {
            m,
            block: false,
            pen: Some(gray),
            kind: Kind::Image {
                image,
                rect: [-w / 2.0, 0.0, w, h],
            },
        },
        Draw {
            m,
            block: false,
            pen: Some(DEFAULT_PEN),
            kind: Kind::Flush,
        },
    ]
}
