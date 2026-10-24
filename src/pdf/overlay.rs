use std::collections::HashSet;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use super::{PT_PER_PX, Page};
use crate::format::Rect;

/// One page of the output.
pub enum Out {
    /// Page `index` (0-based) of the original PDF with `page` (drawn at [`Source::page_size`])
    /// over it; `content` is the scene's content rect (x from the center, y, w, h) in pixels.
    Overlay {
        index: u32,
        page: Page,
        content: Option<Rect>,
    },
    /// An inserted page.
    Note(Page),
}

/// The document's original PDF.
pub struct Source {
    doc: Document,
    pages: Vec<ObjectId>,
}

/// A page's boxes ([x0, y0, x1, y1], normalized) and rotation in quarter turns clockwise.
struct Geo {
    media: [f64; 4],
    crop: [f64; 4],
    rot: u8,
}

impl Geo {
    /// Width and height as displayed, in points (PDFium's FPDF_GetPageWidth/Height).
    fn displayed(&self) -> (f64, f64) {
        let (w, h) = (self.crop[2] - self.crop[0], self.crop[3] - self.crop[1]);
        if self.rot % 2 == 1 { (h, w) } else { (w, h) }
    }

    /// Displayed point (`u` right, `v` down from the top left, in points) to PDF user space.
    fn map(&self, u: f64, v: f64) -> (f64, f64) {
        let [x0, y0, x1, y1] = self.crop;
        match self.rot {
            0 => (x0 + u, y1 - v),
            1 => (x0 + v, y0 + u),
            2 => (x1 - u, y0 + v),
            _ => (x1 - v, y1 - u),
        }
    }
}

const MARGIN: f64 = 5.0 / 25.4 * 72.0;
const NAME: &[u8] = b"RmAnnotations";

fn number(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

fn rect_object(r: [f64; 4]) -> Object {
    Object::Array(r.iter().map(|&v| Object::Real(v as f32)).collect())
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Pdf(#[from] lopdf::Error),
    /// Our own PDF of the drawn pages failed to load back.
    #[error("annotations: {0}")]
    Annotations(lopdf::Error),
    /// Missing page info: a page we couldn't read before rewriting the tree.
    #[error("page {0}")]
    Page(&'static str),
}

impl Source {
    pub fn load(data: &[u8]) -> Result<Source, Error> {
        let mut doc = Document::load_mem(data)?;
        if doc.was_encrypted() {
            // Loaded with the empty password and decrypted; written out unencrypted.
            doc.trailer.remove(b"Encrypt");
        }
        let pages = doc.get_pages().into_values().collect();
        Ok(Source { doc, pages })
    }

    /// A page attribute, inherited through the page tree, dereferenced.
    fn inherited(&self, page: ObjectId, key: &[u8]) -> Option<Object> {
        let mut id = page;
        for _ in 0..64 {
            let d = self.doc.get_dictionary(id).ok()?;
            if let Ok(v) = d.get(key) {
                return self.doc.dereference(v).ok().map(|(_, o)| o.clone());
            }
            id = d.get(b"Parent").ok()?.as_reference().ok()?;
        }
        None
    }

    fn rect(&self, page: ObjectId, key: &[u8]) -> Option<[f64; 4]> {
        let a = self.inherited(page, key)?;
        let v: Vec<f64> = a
            .as_array()
            .ok()?
            .iter()
            .filter_map(|o| number(self.doc.dereference(o).ok()?.1))
            .collect();
        let [a, b, c, d] = v[..] else { return None };
        Some([a.min(c), b.min(d), a.max(c), b.max(d)])
    }

    fn geo(&self, index: u32) -> Option<Geo> {
        let id = *self.pages.get(index as usize)?;
        let media = self
            .rect(id, b"MediaBox")
            .unwrap_or([0.0, 0.0, 612.0, 792.0]);
        let crop = match self.rect(id, b"CropBox") {
            Some(c) => [
                c[0].max(media[0]),
                c[1].max(media[1]),
                c[2].min(media[2]),
                c[3].min(media[3]),
            ],
            None => media,
        };
        let crop = if crop[0] < crop[2] && crop[1] < crop[3] {
            crop
        } else {
            media
        };
        let rot = self
            .inherited(id, b"Rotate")
            .and_then(|r| r.as_i64().ok())
            .unwrap_or(0);
        Some(Geo {
            media,
            crop,
            rot: (rot.rem_euclid(360) / 90) as u8,
        })
    }

    /// The size in device pixels to draw page `index`'s annotations at, or `None` if the PDF
    /// has no such page.
    pub fn page_size(&self, index: u32) -> Option<(u32, u32)> {
        let (w, h) = self.geo(index)?.displayed();
        Some((
            ((w / PT_PER_PX as f64).round() as u32).max(1),
            ((h / PT_PER_PX as f64).ceil() as u32).max(1),
        ))
    }

    /// The output PDF: `out`'s pages in order over the original's (a page used twice is
    /// copied; pages not used are dropped).
    pub fn write(self, out: Vec<Out>) -> Result<Vec<u8>, Error> {
        let mut drawn = Vec::new();
        let mut kinds = Vec::new();
        for o in out {
            match o {
                Out::Overlay {
                    index,
                    page,
                    content,
                } => {
                    let empty = page.strokes.is_empty()
                        && page.groups.is_empty()
                        && page.text.is_empty()
                        && page.highlights.is_empty()
                        && page.shapes.is_empty()
                        && page.rules.is_empty();
                    kinds.push(Some((index, content, empty)));
                    drawn.push(page);
                }
                Out::Note(page) => {
                    kinds.push(None);
                    drawn.push(page);
                }
            }
        }
        let mut ours =
            Document::load_mem(&crate::pdf::write(&drawn)).map_err(Error::Annotations)?;
        // Page geometry and attributes are read before the page tree is changed.
        let geos: Vec<Option<Geo>> = kinds
            .iter()
            .map(|k| k.and_then(|(i, _, _)| self.geo(i)))
            .collect();
        let attrs: Vec<Option<(Object, Option<Object>)>> = kinds
            .iter()
            .map(|k| {
                k.map(|(i, _, _)| {
                    let id = self.pages[i as usize];
                    let res = self
                        .inherited(id, b"Resources")
                        .filter(|r| r.as_dict().is_ok())
                        .unwrap_or(Object::Dictionary(Dictionary::new()));
                    let contents = self
                        .doc
                        .get_dictionary(id)
                        .ok()
                        .and_then(|d| d.get(b"Contents").ok().cloned());
                    (res, contents)
                })
            })
            .collect();
        let Source { mut doc, pages } = self;
        ours.renumber_objects_with(doc.max_id + 1);
        let our_pages: Vec<ObjectId> = ours.get_pages().into_values().collect();
        doc.max_id = doc.max_id.max(ours.max_id);
        let our_contents: Vec<Vec<u8>> = our_pages
            .iter()
            .map(|&p| ours.get_page_content(p).unwrap_or_default())
            .collect();
        doc.objects.extend(std::mem::take(&mut ours.objects));

        let tree = doc.new_object_id();
        let mut used = HashSet::new();
        let mut kids = Vec::new();
        for (n, kind) in kinds.iter().enumerate() {
            let ours_id = our_pages[n];
            let Some((index, content, empty)) = *kind else {
                doc.get_dictionary_mut(ours_id)?.set("Parent", tree);
                kids.push(ours_id);
                continue;
            };
            let orig = pages[index as usize];
            let geo = geos[n].as_ref().ok_or(Error::Page("geometry"))?;
            let (res, contents) = attrs[n].clone().ok_or(Error::Page("attributes"))?;
            let mut dict = doc.get_dictionary(orig)?.clone();
            let id = if used.insert(orig) {
                orig
            } else {
                doc.add_object(Dictionary::new())
            };
            dict.set("Parent", tree);
            dict.set("Rotate", (geo.rot as i64) * 90);
            let (mut media, mut crop) = (geo.media, geo.crop);
            if !empty {
                let ours_dict = doc.get_dictionary(ours_id)?;
                let ours_res = ours_dict
                    .get(b"Resources")
                    .cloned()
                    .unwrap_or(Object::Dictionary(Dictionary::new()));
                let hov = ours_dict
                    .get(b"MediaBox")
                    .ok()
                    .and_then(|m| m.as_array().ok())
                    .and_then(|a| a.get(3).and_then(number))
                    .unwrap_or(0.0);
                let (w, _) = geo.displayed();
                let dx = (w - drawn[n].w as f64 * PT_PER_PX as f64) / 2.0;
                let [x0, y0, x1, y1] = geo.crop;
                let m = match geo.rot {
                    0 => [1.0, 0.0, 0.0, 1.0, x0 + dx, y1 - hov],
                    1 => [0.0, 1.0, -1.0, 0.0, x0 + hov, y0 + dx],
                    2 => [-1.0, 0.0, 0.0, -1.0, x1 - dx, y0 + hov],
                    _ => [0.0, -1.0, 1.0, 0.0, x1 - hov, y1 - dx],
                };
                let mut form = Dictionary::new();
                form.set("Type", Object::Name(b"XObject".to_vec()));
                form.set("Subtype", Object::Name(b"Form".to_vec()));
                form.set("BBox", rect_object([-1e5, -1e5, 1e5, 1e5]));
                form.set(
                    "Matrix",
                    Object::Array(m.iter().map(|&v| Object::Real(v as f32)).collect()),
                );
                form.set("Resources", ours_res);
                let form = doc.add_object(Stream::new(form, our_contents[n].clone()));

                let mut res = res.as_dict()?.clone();
                let mut xobjects = match res.get(b"XObject") {
                    Ok(x) => doc
                        .dereference(x)
                        .ok()
                        .and_then(|(_, o)| o.as_dict().ok().cloned())
                        .unwrap_or_default(),
                    Err(_) => Dictionary::new(),
                };
                xobjects.set(NAME, form);
                res.set("XObject", xobjects);
                dict.set("Resources", res);

                let mut streams = vec![Object::Reference(
                    doc.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec())),
                )];
                match contents {
                    Some(Object::Array(a)) => streams.extend(a),
                    Some(c @ Object::Reference(_)) => streams.push(c),
                    _ => {}
                }
                let tail = format!("\nQ\nq /{} Do Q\n", std::str::from_utf8(NAME).unwrap());
                streams.push(Object::Reference(
                    doc.add_object(Stream::new(Dictionary::new(), tail.into_bytes())),
                ));
                dict.set("Contents", streams);

                // Content past the crop box widens it to there plus 5 mm (on that side).
                if let Some(Rect { x, y, w: cw, h: ch }) = content {
                    let k = PT_PER_PX as f64;
                    let (w, h) = geo.displayed();
                    let (u0, u1) = (x * k + w / 2.0, (x + cw) * k + w / 2.0);
                    let (v0, v1) = (y * k, (y + ch) * k);
                    if u0 < 0.0 || v0 < 0.0 || u1 > w || v1 > h {
                        let l = if u0 < 0.0 { u0 - MARGIN } else { 0.0 };
                        let r = if u1 > w { u1 + MARGIN } else { w };
                        let t = if v0 < 0.0 { v0 - MARGIN } else { 0.0 };
                        let b = if v1 > h { v1 + MARGIN } else { h };
                        let (a, c) = (geo.map(l, t), geo.map(r, b));
                        crop = [a.0.min(c.0), a.1.min(c.1), a.0.max(c.0), a.1.max(c.1)];
                        media = [
                            media[0].min(crop[0]),
                            media[1].min(crop[1]),
                            media[2].max(crop[2]),
                            media[3].max(crop[3]),
                        ];
                    }
                }
            }
            dict.set("MediaBox", rect_object(media));
            dict.set("CropBox", rect_object(crop));
            *doc.get_object_mut(id)? = Object::Dictionary(dict);
            kids.push(id);
        }
        let mut node = Dictionary::new();
        node.set("Type", Object::Name(b"Pages".to_vec()));
        node.set("Count", kids.len() as i64);
        node.set(
            "Kids",
            kids.into_iter().map(Object::Reference).collect::<Vec<_>>(),
        );
        doc.objects.insert(tree, Object::Dictionary(node));
        doc.catalog_mut()?.set("Pages", tree);
        doc.prune_objects();
        doc.compress();
        let mut buf = Vec::new();
        doc.save_to(&mut buf).expect("writing to memory");
        Ok(buf)
    }
}
