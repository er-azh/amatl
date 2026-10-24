use std::collections::BTreeMap;

use super::raster::{ClipBox, Raster, Var, Vertex, clip_triangle};
use super::stroker::{NoVarying, Stroker};
use super::{Canvas, Renderer, premultiply};
use crate::format::Line;

struct Coverage {
    tiles: BTreeMap<(u32, u32), [u8; 1024]>,
    value: u8,
    clip: ClipBox,
}

impl Coverage {
    fn put(&mut self, x: u32, y: u32) {
        let t = self.tiles.entry((y & !31, x & !31)).or_insert([0; 1024]);
        let c = &mut t[((y & 31) * 32 + (x & 31)) as usize];
        if *c < self.value {
            *c = self.value;
        }
    }

    fn row(&mut self, y: f32, left: f32, right: f32) {
        let (l, r) = (left as i32, right as i32);
        if l < r && (l as u32) < (r as u32) {
            let y = y as i32 as u32;
            for x in l as u32..r as u32 {
                self.put(x, y);
            }
        }
    }
}

impl Raster for Coverage {
    fn triangle(&mut self, a: Vertex, b: Vertex, c: Vertex, va: Var, vb: Var, vc: Var) {
        let clip = self.clip;
        clip_triangle(clip, a, b, c, va, vb, vc, &mut |a, b, c, _, _, _| {
            self.mono(a, b, c)
        });
    }
}

impl Coverage {
    fn mono(&mut self, a: Vertex, b: Vertex, c: Vertex) {
        let (mut a, mut b, mut c) = (a, b, c);
        if b.y < a.y {
            std::mem::swap(&mut a, &mut b);
        }
        if c.y < a.y {
            std::mem::swap(&mut a, &mut c);
        }
        if c.y < b.y {
            std::mem::swap(&mut b, &mut c);
        }
        let f = a.y.floor();
        let mut y = f + if a.y - f <= 0.5 { 0.5 } else { 1.5 };
        if a.y != b.y {
            let mut dxl = (b.x - a.x) / (b.y - a.y);
            let mut dxr = (c.x - a.x) / (c.y - a.y);
            let mut left = a.x + dxl * (y - a.y) + 0.5;
            let mut right = a.x + dxr * (y - a.y) + 0.5;
            if dxr < dxl {
                std::mem::swap(&mut left, &mut right);
                std::mem::swap(&mut dxl, &mut dxr);
            }
            while y < b.y {
                self.row(y, left, right);
                y += 1.0;
                left += dxl;
                right += dxr;
            }
        }
        if b.y != c.y {
            let mut dxl = (c.x - b.x) / (c.y - b.y);
            let mut dxr = (c.x - a.x) / (c.y - a.y);
            let mut left = c.x - dxl * (c.y - y) + 0.5;
            let mut right = c.x - dxr * (c.y - y) + 0.5;
            if right < left {
                std::mem::swap(&mut left, &mut right);
                std::mem::swap(&mut dxl, &mut dxr);
            }
            while y < c.y {
                self.row(y, left, right);
                y += 1.0;
                left += dxl;
                right += dxr;
            }
        }
    }
}

fn rnd(x: u32) -> u32 {
    x.wrapping_add(x.wrapping_add(0x101) >> 8) >> 8
}

fn blend(d: u32, c: u32, col: u32) -> u32 {
    let da = d >> 24;
    let (dr, dg, db) = ((d >> 16) & 0xff, (d >> 8) & 0xff, d & 0xff);
    let mut c = c;
    if let Some(luma) = (db * 0x1d + dr * 0x4c + dg * 0x96).checked_div(da) {
        c = rnd(c * luma);
    }
    if c == 0 || col < 0x100_0000 {
        return d;
    }
    let sa = (col >> 24) * c / 0xff;
    let inv = 0xff - sa;
    let ch = |dc: u32, cc: u32| {
        (dc + 0xff - da)
            .wrapping_mul(cc * c / 0xff)
            .wrapping_add(inv * dc)
    };
    let g = ch(dg, (col >> 8) & 0xff);
    let b = ch(db, col & 0xff);
    let r = ch(dr, (col >> 16) & 0xff);
    (g.wrapping_add(g.wrapping_add(0x101) >> 8) & 0xffff_ff00)
        | rnd(b)
        | (sa + da)
            .wrapping_sub(rnd(da * sa))
            .wrapping_mul(0x100_0000)
        | (rnd(r) << 16)
}

fn composite(canvas: &mut Canvas, cov: &Coverage, (w, h): (u32, u32), col: u32) {
    for (&(ty, tx), t) in &cov.tiles {
        if w == tx || h == ty {
            continue;
        }
        let tw = w.wrapping_sub(tx).min(32);
        let th = h.wrapping_sub(ty).min(32);
        for row in 0..th {
            for x in 0..tw {
                let (px, py) = (tx.wrapping_add(x) as i32, ty.wrapping_add(row) as i32);
                if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 {
                    continue;
                }
                if let Some(d) = canvas.get(px, py) {
                    canvas.put(px, py, blend(d, t[(row * 32 + x) as usize] as u32, col));
                }
            }
        }
    }
}

pub fn draw(r: &mut Renderer, line: &Line) {
    let scale = r.scale;
    let t = r.transform();
    let size = match r.raster {
        Some(_) => {
            let c = r.canvas.view.clip;
            (c.2 as u32, c.3 as u32)
        }
        None => (r.canvas.w, r.canvas.h),
    };
    let mut st = Stroker::new(NoVarying);
    st.length = line.starting_length * scale;
    let mut cov = Coverage {
        tiles: BTreeMap::new(),
        value: 0,
        clip: r.clip_box(),
    };
    for p in &line.points {
        st.width = p.width as f32 * 0.25 * scale;
        let s = p.speed as f32 * 0.25;
        let sf = if s < 2.0 {
            1.0
        } else if s <= 10.0 {
            let u = (s - 2.0) * 0.125;
            (1.0 - (3.0 - (u + u)) * u * u) * 0.2 + 0.8
        } else {
            0.8
        };
        let v = sf + p.pressure as f32 / 255.0 * 0.3;
        cov.value = if v < 1.0 {
            (v * 255.0) as u32 as u8
        } else {
            255
        };
        let (x, y) = t.map(p.x, p.y);
        st.line_to(&mut cov, x, y);
        st.start_cap(&mut cov);
    }
    st.finish(&mut cov);
    composite(
        &mut r.canvas,
        &cov,
        size,
        premultiply(line.color.to_argb(line.argb)),
    );
}
