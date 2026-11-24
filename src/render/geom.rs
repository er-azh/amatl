use std::collections::HashMap;

use crate::format::CrdtId;
use crate::format::{ImageItem, ItemValue, Line, Node, Rect, Scene};
use crate::render::qpaint::Affine;

fn rect_margin(l: &Line) -> f64 {
    let f = l.tool.margin_factor(l.thickness_scale);
    ((l.thickness_scale as f32) * f * 1.0 + 2.0) as f64
}

pub fn line_rect(l: &Line) -> Option<Rect> {
    if l.points.is_empty() {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in &l.points {
        x0 = x0.min(p.x);
        x1 = x1.max(p.x);
        y0 = y0.min(p.y);
        y1 = y1.max(p.y);
    }
    let h = rect_margin(l) * 0.5;
    Some(Rect {
        x: x0 as f64 - h,
        y: y0 as f64 - h,
        w: (x1 - x0) as f64 + h + h,
        h: (y1 - y0) as f64 + h + h,
    })
}

pub fn content_rect(sc: &Scene, offsets: &Offsets) -> Option<Rect> {
    fn walk(sc: &Scene, n: &Node, offsets: &Offsets, o: (f32, f32), acc: &mut Option<Rect>) {
        if !n.visible.value {
            return;
        }
        let o = offset(offsets, n, o);
        for c in &n.children {
            let Some(it) = sc.item(n.id, *c) else {
                continue;
            };
            match it.value.as_ref() {
                Some(ItemValue::Group(g)) => {
                    if let Some(child) = sc.nodes.get(g) {
                        walk(sc, child, offsets, o, acc);
                    }
                }
                Some(ItemValue::Line(l)) => {
                    if let Some(r) = line_rect(l) {
                        let r = r.translate(o.0 as f64, o.1 as f64);
                        *acc = Some(acc.map_or(r, |a| a.union(r)));
                    }
                }
                // The exporter's raster holds image items too (their bounding rect).
                Some(ItemValue::Image(i)) => {
                    if let Some(t) = image_transform(i) {
                        let pts = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)]
                            .map(|(x, y)| t.map(x, y));
                        let x0 = pts.iter().map(|p| p.0).fold(f64::MAX, f64::min);
                        let y0 = pts.iter().map(|p| p.1).fold(f64::MAX, f64::min);
                        let x1 = pts.iter().map(|p| p.0).fold(f64::MIN, f64::max);
                        let y1 = pts.iter().map(|p| p.1).fold(f64::MIN, f64::max);
                        let r = Rect {
                            x: x0,
                            y: y0,
                            w: x1 - x0,
                            h: y1 - y0,
                        }
                        .translate(o.0 as f64, o.1 as f64);
                        *acc = Some(acc.map_or(r, |a| a.union(r)));
                    }
                }
                _ => {}
            }
        }
    }
    let mut acc = None;
    if let Some(root) = sc.nodes.get(&crate::format::ROOT_ID) {
        walk(sc, root, offsets, (0.0, 0.0), &mut acc);
    }
    acc
}

/// A visible image item, the scene offset of its group, and how many lines the exporter passes
/// through as strokes ([`crate::format::Tool::is_direct`]) come before it in z order: the
/// exporter draws both in one list, before the traced regions.
pub type ImageAt<'a> = (&'a ImageItem, (f32, f32), usize);

/// The visible image items in z order (see [`ImageAt`]).
pub fn images<'a>(sc: &'a Scene, offsets: &Offsets) -> Vec<ImageAt<'a>> {
    fn walk<'a>(
        sc: &'a Scene,
        n: &Node,
        offsets: &Offsets,
        o: (f32, f32),
        direct: &mut usize,
        acc: &mut Vec<ImageAt<'a>>,
    ) {
        if !n.visible.value {
            return;
        }
        let o = offset(offsets, n, o);
        for c in &n.children {
            match sc.item(n.id, *c).and_then(|it| it.value.as_ref()) {
                Some(ItemValue::Group(g)) => {
                    if let Some(child) = sc.nodes.get(g) {
                        walk(sc, child, offsets, o, direct, acc);
                    }
                }
                Some(ItemValue::Line(l)) if l.tool.is_direct() => *direct += 1,
                Some(ItemValue::Image(i)) => acc.push((i, o, *direct)),
                _ => {}
            }
        }
    }
    let mut acc = Vec::new();
    if let Some(root) = sc.nodes.get(&crate::format::ROOT_ID) {
        walk(sc, root, offsets, (0.0, 0.0), &mut 0, &mut acc);
    }
    acc
}

/// The transform an image item draws its image with (`SceneImageItem::Bounds`): the image fills
/// the rect (-0.5, -0.5, 1, 1), mapped by `m11 m12` = the mesh's edge from the vertex textured
/// (0, 0) to the one textured (1, 0), `m21 m22` = from (1, 0) to (1, 1), and the translation =
/// the center between (0, 0) and (1, 1), all in floats. None when the mesh is not two triangles
/// over a textured rectangle (xochitl then rejects the item).
pub(crate) fn image_transform(item: &ImageItem) -> Option<Affine> {
    let vs = &item.vertices;
    if vs.len() != 4 || item.indices.len() != 6 {
        return None;
    }
    let near = |a: f32, b: f32| (a - b).abs() <= 0.01;
    let corner = |u: f32, v: f32| vs.iter().position(|p| near(p.u, u) && near(p.v, v));
    let (i00, i10, i11, i01) = (
        corner(0.0, 0.0)?,
        corner(1.0, 0.0)?,
        corner(1.0, 1.0)?,
        corner(0.0, 1.0)?,
    );
    // The two triangles must split the rectangle along a diagonal.
    let tri = |t: &[u32]| {
        let mut t = [t[0] as usize, t[1] as usize, t[2] as usize];
        t.sort_unstable();
        t
    };
    let sorted = |a: [usize; 3]| tri(&a.map(|i| i as u32));
    let given = [tri(&item.indices[..3]), tri(&item.indices[3..])];
    let splits = [
        [sorted([i00, i10, i11]), sorted([i00, i11, i01])],
        [sorted([i00, i10, i01]), sorted([i10, i11, i01])],
    ];
    if !splits
        .iter()
        .any(|s| s == &given || (s[0] == given[1] && s[1] == given[0]))
    {
        return None;
    }
    let (p00, p10, p11, p01) = (vs[i00], vs[i10], vs[i11], vs[i01]);
    let (ux, uy) = (p10.x - p00.x, p10.y - p00.y);
    let (vx, vy) = (p11.x - p10.x, p11.y - p10.y);
    let ex = (p01.x - p11.x) + ux;
    let ey = (p01.y - p11.y) + uy;
    if ey * ey + ex * ex > 0.01 {
        return None; // "Opposing sides are not equal and parallel"
    }
    let unit = |x: f32, y: f32| {
        // QVector2D::normalized
        let len = x.hypot(y);
        if (len - 1.0).abs() < 0.00001 && len - 1.0 >= -0.00001 {
            (x, y)
        } else if len > 0.00001 {
            (x / len, y / len)
        } else {
            (0.0, 0.0)
        }
    };
    let (a, b) = (unit(ux, uy), unit(vx, vy));
    if (b.1 * a.1 + b.0 * a.0).abs() > 0.01 {
        return None; // "The rectangle does not have right angles"
    }
    Some(Affine {
        m11: ux as f64,
        m12: uy as f64,
        m21: vx as f64,
        m22: vy as f64,
        dx: ((p00.x + p11.x) * 0.5) as f64,
        dy: ((p00.y + p11.y) * 0.5) as f64,
    })
}

/// Scene-unit translation of a group's contents, keyed by group node id (anchored groups sit at
/// their anchor's text position; see `crate::text`).
pub type Offsets = HashMap<CrdtId, (f32, f32)>;

/// Offsets of anchored groups: x from the group's anchor origin, y from the text baseline the
/// anchor refers to (`text.y` + the laid-out line's baseline).
pub fn anchor_offsets(sc: &Scene, layout: &crate::text::Layout) -> Offsets {
    let mut m = Offsets::new();
    let Some(t) = &sc.text else { return m };
    for n in sc.nodes.values() {
        if let (Some(a), Some(ox)) = (&n.anchor_id, &n.anchor_origin_x)
            && let Some(b) = layout.anchor_baseline(a.value)
        {
            m.insert(n.id, (ox.value, (t.y + b as f64 / 64.0) as f32));
        }
    }
    m
}

pub(super) fn offset(offsets: &Offsets, n: &Node, o: (f32, f32)) -> (f32, f32) {
    match offsets.get(&n.id) {
        Some(d) => (o.0 + d.0, o.1 + d.1),
        None => o,
    }
}
