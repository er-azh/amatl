//! Typed decoding of the v6 blocks into a [`Scene`].
//!
//! Every block and item value is decoded tag by tag; bytes that are not understood are kept in
//! `extra` fields and reported through `Scene::warnings`, never silently dropped.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use crate::Error;
use crate::format::crdt::{CrdtId, Item, Lww, toposort};
use crate::format::scene::{
    GlyphRange, ItemValue, Line, Node, ParagraphStyle, PenColor, Point, Rect, RootText, Scene,
    SceneInfo, TextValue, Tool,
};
use crate::format::tagged::{BlockInfo, Reader, TagType, blocks};

/// Block types, the last byte of a block header. Other types (0x06, text items, among them)
/// are reported as unknown.
mod block {
    pub const MIGRATION_INFO: u8 = 0x00;
    pub const SCENE_TREE: u8 = 0x01;
    pub const TREE_NODE: u8 = 0x02;
    pub const GLYPH_ITEM: u8 = 0x03;
    pub const GROUP_ITEM: u8 = 0x04;
    pub const LINE_ITEM: u8 = 0x05;
    pub const ROOT_TEXT: u8 = 0x07;
    pub const TOMBSTONE: u8 = 0x08;
    pub const AUTHOR_IDS: u8 = 0x09;
    pub const PAGE_INFO: u8 = 0x0a;
    pub const SCENE_INFO: u8 = 0x0d;
}

/// Item value kinds, the first byte of an item block's value.
mod kind {
    pub const GLYPH: u8 = 1;
    pub const GROUP: u8 = 2;
    pub const LINE: u8 = 3;
}

fn lww<'a, T>(
    r: &mut Reader<'a>,
    i: u64,
    f: impl FnOnce(&mut Reader<'a>) -> Result<T, Error>,
) -> Result<Lww<T>, Error> {
    let mut s = r.tag_sub(i)?;
    let timestamp = s.tag_id(1)?;
    let value = f(&mut s)?;
    finish_sub(&s)?;
    Ok(Lww { timestamp, value })
}

fn finish_sub(r: &Reader) -> Result<(), Error> {
    if r.is_empty() {
        Ok(())
    } else {
        Err(Error::Format {
            offset: r.pos(),
            msg: format!("{} unread bytes in sub-block", r.remaining()),
        })
    }
}

/// String: varuint length, u8 is_ascii, bytes (UTF-8).
fn string(r: &mut Reader) -> Result<String, Error> {
    let n = r.varuint()? as usize;
    let _is_ascii = r.u8()?;
    let at = r.pos();
    let b = r.bytes(n)?;
    String::from_utf8(b.to_vec()).map_err(|_| Error::Format {
        offset: at,
        msg: "invalid utf-8".into(),
    })
}

fn tag_string(r: &mut Reader, i: u64) -> Result<String, Error> {
    let mut s = r.tag_sub(i)?;
    let v = string(&mut s)?;
    finish_sub(&s)?;
    Ok(v)
}

fn read_item<'a, V>(
    r: &mut Reader<'a>,
    value: impl FnOnce(&mut Reader<'a>) -> Result<V, Error>,
) -> Result<(CrdtId, Item<V>), Error> {
    let parent = r.tag_id(1)?;
    let id = r.tag_id(2)?;
    let left = r.tag_id(3)?;
    let right = r.tag_id(4)?;
    let deleted_length = r.tag_u32(5)?;
    let value = if r.has_tag(6, TagType::Length4) {
        let mut s = r.tag_sub(6)?;
        let v = value(&mut s)?;
        finish_sub(&s)?;
        Some(v)
    } else {
        None
    };
    Ok((
        parent,
        Item {
            id,
            left,
            right,
            deleted_length,
            value,
        },
    ))
}

fn item_value(r: &mut Reader, block_type: u8) -> Result<ItemValue, Error> {
    let kind = r.u8()?;
    Ok(match (block_type, kind) {
        (block::GROUP_ITEM, kind::GROUP) => ItemValue::Group(r.tag_id(2)?),
        (block::LINE_ITEM, kind::LINE) => ItemValue::Line(line(r)?),
        (block::GLYPH_ITEM, kind::GLYPH) => ItemValue::Glyph(glyph(r)?),
        _ => ItemValue::Unknown(kind, r.rest().to_vec()),
    })
}

fn line(r: &mut Reader) -> Result<Line, Error> {
    let tool = Tool::from_id(r.tag_u32(1)?);
    let color = PenColor::from_id(r.tag_u32(2)?);
    let thickness_scale = r.tag_f64(3)?;
    let starting_length = r.tag_f32(4)?;
    let mut pts = r.tag_sub(5)?;
    if pts.remaining() % 14 != 0 {
        return Err(Error::Format {
            offset: pts.pos(),
            msg: format!("point data of {} bytes", pts.remaining()),
        });
    }
    let mut points = Vec::with_capacity(pts.remaining() / 14);
    while !pts.is_empty() {
        points.push(Point {
            x: pts.f32()?,
            y: pts.f32()?,
            speed: pts.u16()?,
            width: pts.u16()?,
            direction: pts.u8()?,
            pressure: pts.u8()?,
        });
    }
    let id6 = r.opt(6, TagType::Id, Reader::tag_id)?;
    let id7 = r.opt(7, TagType::Id, Reader::tag_id)?;
    let argb = r.opt(8, TagType::Byte4, Reader::tag_u32)?;
    let extra = r.rest().to_vec();
    Ok(Line {
        tool,
        color,
        thickness_scale,
        starting_length,
        points,
        id6,
        id7,
        argb,
        extra,
    })
}

fn glyph(r: &mut Reader) -> Result<GlyphRange, Error> {
    let start = r.opt(2, TagType::Byte4, Reader::tag_u32)?;
    let length = r.tag_u32(3)?;
    let color = PenColor::from_id(r.tag_u32(4)?);
    let text = tag_string(r, 5)?;
    let mut s = r.tag_sub(6)?;
    let n = s.varuint()?;
    let mut rects = Vec::new();
    for _ in 0..n {
        rects.push(Rect {
            x: s.f64()?,
            y: s.f64()?,
            w: s.f64()?,
            h: s.f64()?,
        });
    }
    finish_sub(&s)?;
    // Highlighter color chosen as an ARGB code (lines keep theirs in tag 8).
    let argb = r.opt(10, TagType::Byte4, Reader::tag_u32)?;
    let extra = r.rest().to_vec();
    Ok(GlyphRange {
        start,
        length,
        color,
        text,
        rects,
        argb,
        extra,
    })
}

fn root_text(r: &mut Reader) -> Result<RootText, Error> {
    let _block_id = r.tag_id(1)?;
    let mut body = r.tag_sub(2)?;

    // [1][1]: character items
    let mut s1 = body.tag_sub(1)?;
    let mut s = s1.tag_sub(1)?;
    let n = s.varuint()?;
    let mut items = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let mut it = s.tag_sub(0)?;
        let id = it.tag_id(2)?;
        let left = it.tag_id(3)?;
        let right = it.tag_id(4)?;
        let deleted_length = it.tag_u32(5)?;
        let value = if it.has_tag(6, TagType::Length4) {
            let mut v = it.tag_sub(6)?;
            let s = string(&mut v)?;
            // an optional [2] u32 after the string is a format code instead of text
            let val = match v.opt(2, TagType::Byte4, Reader::tag_u32)? {
                Some(code) => TextValue::Format(code),
                None => TextValue::Str(s),
            };
            finish_sub(&v)?;
            Some(val)
        } else {
            None
        };
        finish_sub(&it)?;
        items.push(Item {
            id,
            left,
            right,
            deleted_length,
            value,
        });
    }
    finish_sub(&s)?;
    finish_sub(&s1)?;

    let mut s2 = body.tag_sub(2)?;
    let mut s = s2.tag_sub(1)?;
    let n = s.varuint()?;
    let mut styles = BTreeMap::new();
    for _ in 0..n {
        let key = s.crdt_id()?;
        let timestamp = s.tag_id(1)?;
        let mut v = s.tag_sub(2)?;
        let value = ParagraphStyle {
            code: v.tag_u8(1)?,
            kind: v.opt(2, TagType::Byte1, Reader::tag_u8)?,
            bits: v.opt(3, TagType::Byte4, Reader::tag_u32)?,
        };
        finish_sub(&v)?;
        styles.insert(key, Lww { timestamp, value });
    }
    finish_sub(&s)?;
    finish_sub(&s2)?;
    finish_sub(&body)?;

    let mut pos = r.tag_sub(3)?;
    let x = pos.f64()?;
    let y = pos.f64()?;
    finish_sub(&pos)?;
    let width = r.tag_f32(4)?;
    let extra = r.rest().to_vec();
    Ok(RootText {
        items,
        styles,
        x,
        y,
        width,
        extra,
    })
}

fn tree_node(r: &mut Reader) -> Result<Node, Error> {
    let id = r.tag_id(1)?;
    let label = lww(r, 2, |s| tag_string(s, 2))?;
    let visible = lww(r, 3, |s| s.tag_bool(2))?;
    let anchor_id = r.opt(7, TagType::Length4, |r, i| lww(r, i, |s| s.tag_id(2)))?;
    let anchor_type = r.opt(8, TagType::Length4, |r, i| lww(r, i, |s| s.tag_u8(2)))?;
    let anchor_threshold = r.opt(9, TagType::Length4, |r, i| lww(r, i, |s| s.tag_f32(2)))?;
    let anchor_origin_x = r.opt(10, TagType::Length4, |r, i| lww(r, i, |s| s.tag_f32(2)))?;
    let extra = r.rest().to_vec();
    Ok(Node {
        id,
        label,
        visible,
        anchor_id,
        anchor_type,
        anchor_threshold,
        anchor_origin_x,
        extra,
        children: Vec::new(),
    })
}

fn scene_info(r: &mut Reader) -> Result<SceneInfo, Error> {
    let current_layer = Some(lww(r, 1, |s| s.tag_id(2))?);
    let background_visible = r.opt(2, TagType::Length4, |r, i| lww(r, i, |s| s.tag_bool(2)))?;
    let root_document_visible = r.opt(3, TagType::Length4, |r, i| lww(r, i, |s| s.tag_bool(2)))?;
    let paper_size = r.opt(5, TagType::Length4, |r, i| {
        let mut s = r.tag_sub(i)?;
        let v = (s.u32()?, s.u32()?);
        finish_sub(&s)?;
        Ok(v)
    })?;
    let extra = r.rest().to_vec();
    Ok(SceneInfo {
        current_layer,
        background_visible,
        root_document_visible,
        paper_size,
        extra,
    })
}

fn author_ids(r: &mut Reader) -> Result<Vec<(String, u16)>, Error> {
    let n = r.varuint()?;
    let mut out = Vec::new();
    for _ in 0..n {
        let mut s = r.tag_sub(0)?;
        let len = s.varuint()? as usize;
        let uuid = s.bytes(len)?;
        let id = s.u16()?;
        finish_sub(&s)?;
        let hex = uuid.iter().fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        });
        out.push((hex, id));
    }
    Ok(out)
}

impl Scene {
    pub fn parse(data: &[u8]) -> Result<Scene, Error> {
        let mut sc = Scene::default();
        let mut tree_parents: HashMap<CrdtId, CrdtId> = HashMap::new();
        for (info, mut r) in blocks(data)? {
            sc.block(info, &mut r, &mut tree_parents)?;
            if !r.is_empty() {
                let rest = r.rest();
                sc.warnings.push(format!(
                    "block type {:#x} at {:#x}: {} trailing bytes {:02x?}",
                    info.block_type,
                    info.offset,
                    rest.len(),
                    &rest[..rest.len().min(32)]
                ));
            }
        }
        sc.finish();
        Ok(sc)
    }

    fn block(
        &mut self,
        info: BlockInfo,
        r: &mut Reader,
        tree_parents: &mut HashMap<CrdtId, CrdtId>,
    ) -> Result<(), Error> {
        let warn_extra = |w: &mut Vec<String>, what: &str, extra: &[u8]| {
            if !extra.is_empty() {
                w.push(format!(
                    "{what} at {:#x}: unknown data {:02x?}",
                    info.offset,
                    &extra[..extra.len().min(32)]
                ));
            }
        };
        match info.block_type {
            block::MIGRATION_INFO => {
                r.rest();
            }
            block::SCENE_TREE => {
                let id = r.tag_id(1)?;
                let _ts = r.tag_id(2)?;
                let _is_update = r.tag_bool(3)?;
                let mut s = r.tag_sub(4)?;
                let parent = s.tag_id(1)?;
                finish_sub(&s)?;
                tree_parents.insert(id, parent);
            }
            block::TREE_NODE => {
                let n = tree_node(r)?;
                warn_extra(&mut self.warnings, "tree node", &n.extra);
                self.nodes.insert(n.id, n);
            }
            block::GLYPH_ITEM..=block::LINE_ITEM => {
                let t = info.block_type;
                let (parent, item) = read_item(r, |s| item_value(s, t))?;
                match &item.value {
                    Some(ItemValue::Line(l)) => warn_extra(&mut self.warnings, "line", &l.extra),
                    Some(ItemValue::Glyph(g)) => {
                        warn_extra(&mut self.warnings, "glyph range", &g.extra)
                    }
                    Some(ItemValue::Unknown(k, _)) => self.warnings.push(format!(
                        "item kind {k} in block {t:#x} at {:#x}",
                        info.offset
                    )),
                    _ => {}
                }
                self.items.entry(parent).or_default().push(item);
            }
            block::ROOT_TEXT => {
                let t = root_text(r)?;
                warn_extra(&mut self.warnings, "root text", &t.extra);
                self.text = Some(t);
            }
            block::TOMBSTONE => {
                let (parent, item) =
                    read_item(r, |s| Ok(ItemValue::Unknown(s.u8()?, s.rest().to_vec())))?;
                self.items.entry(parent).or_default().push(Item {
                    value: None,
                    ..item
                });
            }
            block::AUTHOR_IDS => self.author_ids = author_ids(r)?,
            block::PAGE_INFO => {
                let mut v = Vec::new();
                let mut i = 1;
                while r.has_tag(i, TagType::Byte4) {
                    v.push(r.tag_u32(i)?);
                    i += 1;
                }
                self.page_info = v;
            }
            block::SCENE_INFO => {
                self.info = scene_info(r)?;
                warn_extra(&mut self.warnings, "scene info", &self.info.extra.clone());
            }
            t => {
                self.warnings
                    .push(format!("unknown block type {t:#x} at {:#x}", info.offset));
                r.rest();
            }
        }
        Ok(())
    }

    fn finish(&mut self) {
        let keys: Vec<CrdtId> = self.items.keys().copied().collect();
        for parent in keys {
            let items = self.items.get_mut(&parent).unwrap();
            let order = toposort(items);
            let mut by_id: HashMap<CrdtId, Item<ItemValue>> =
                items.drain(..).map(|i| (i.id, i)).collect();
            *items = order
                .into_iter()
                .filter_map(|id| by_id.remove(&id))
                .collect();
        }
        for (parent, items) in &self.items {
            let children = items
                .iter()
                .filter(|i| i.value.is_some() && i.deleted_length == 0)
                .map(|i| i.id)
                .collect();
            match self.nodes.get_mut(parent) {
                Some(n) => n.children = children,
                None => self
                    .warnings
                    .push(format!("items for unknown group {parent}")),
            }
        }
    }
}
