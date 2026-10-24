use std::collections::HashMap;

use crate::format::CrdtId;
use crate::format::{ItemValue, Line, Node, Rect, Scene};

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
