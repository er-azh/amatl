use serde_json::{Map, Value};

use super::{DEFAULT_PEN, Draw, Kind, Pen, Seg, TextRun};
use crate::format::Rect;
use crate::render::{DPI, Paper};
use crate::rmdoc::Orientation;
use crate::text::{Face, Fonts};

struct Ctx<'a> {
    template_w: f64,
    template_h: f64,
    scene: Rect,
    dpi: f64,
    constants: Vec<(String, f64)>,
    /// Bounding boxes of the enclosing groups.
    parents: Vec<Rect>,
    /// The width of the text item being parsed, for its position.
    text_width: Option<f64>,
    fonts: &'a mut Fonts,
}

impl Ctx<'_> {
    fn var(&self, name: &str) -> Option<f64> {
        let parent = self.parents.last();
        Some(match name {
            "templateWidth" => self.template_w,
            "templateHeight" => self.template_h,
            "parentX" => parent?.x,
            "parentY" => parent?.y,
            "parentWidth" => parent?.w,
            "parentHeight" => parent?.h,
            "paperOriginX" => self.scene.x + self.template_w * 0.5,
            "paperOriginY" => self.scene.y,
            "paperWidth" => self.scene.w,
            "paperHeight" => self.scene.h,
            "physicalDpi" => self.dpi,
            "pixelRatio" => self.dpi / DPI,
            "textWidth" => self.text_width?,
            _ => self.constants.iter().rev().find(|(n, _)| n == name)?.1,
        })
    }

    fn eval(&self, v: &Value) -> Option<f64> {
        match v {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => {
                let s: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
                let mut p = Parser {
                    s: &s,
                    i: 0,
                    ctx: self,
                };
                let v = p.top()?;
                (p.i == s.len()).then_some(v)
            }
            _ => None,
        }
    }
}

/// an arithmetic expression, optionally followed by conditions (`==`, `!=`, `<`, `>`; `&&`
/// binding tighter than `||`) that must then select between two arithmetic expressions with `? :`.
struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    ctx: &'a Ctx<'a>,
}

impl Parser<'_> {
    fn peek(&self) -> u8 {
        self.s.get(self.i).copied().unwrap_or(0)
    }

    fn top(&mut self) -> Option<f64> {
        let first = self.arith()?;
        if !matches!(self.peek(), b'=' | b'!' | b'<' | b'>') {
            return Some(first);
        }
        // An `||` chain of `&&` chains.
        let mut any = false;
        let mut all = self.cond(first)?;
        loop {
            while self.peek() == b'&' {
                self.expect2(b'&')?;
                let a = self.arith()?;
                all = self.cond(a)? && all;
            }
            any = any || all;
            if self.peek() != b'|' {
                break;
            }
            self.expect2(b'|')?;
            let a = self.arith()?;
            all = self.cond(a)?;
        }
        if self.peek() != b'?' {
            return None;
        }
        self.i += 1;
        let t = self.arith()?;
        if self.peek() != b':' {
            return None;
        }
        self.i += 1;
        let f = self.arith()?;
        Some(if any { t } else { f })
    }

    fn expect2(&mut self, c: u8) -> Option<()> {
        if self.s.get(self.i + 1) != Some(&c) {
            return None;
        }
        self.i += 2;
        Some(())
    }

    fn cond(&mut self, a: f64) -> Option<bool> {
        let op = self.peek();
        self.i += 1;
        let op2 = self.peek();
        Some(match (op, op2) {
            (b'=', b'=') => {
                self.i += 1;
                a == self.arith()?
            }
            (b'!', b'=') => {
                self.i += 1;
                a != self.arith()?
            }
            (b'<', _) => a < self.arith()?,
            (b'>', _) => a > self.arith()?,
            _ => return None,
        })
    }

    fn arith(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            match self.peek() {
                b'+' => {
                    self.i += 1;
                    v += self.term()?;
                }
                b'-' => {
                    self.i += 1;
                    v -= self.term()?;
                }
                _ => return Some(v),
            }
        }
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.unary()?;
        loop {
            match self.peek() {
                b'*' => {
                    self.i += 1;
                    v *= self.unary()?;
                }
                b'/' => {
                    self.i += 1;
                    v /= self.unary()?;
                }
                _ => return Some(v),
            }
        }
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek() {
            b'-' => {
                self.i += 1;
                Some(-self.unary()?)
            }
            b'+' => {
                self.i += 1;
                self.unary()
            }
            b'(' => {
                self.i += 1;
                let v = self.top()?;
                if self.peek() != b')' {
                    return None;
                }
                self.i += 1;
                Some(v)
            }
            c if c.is_ascii_digit() || c == b'.' => {
                let st = self.i;
                while self.peek().is_ascii_digit() || self.peek() == b'.' {
                    self.i += 1;
                }
                std::str::from_utf8(&self.s[st..self.i]).ok()?.parse().ok()
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let st = self.i;
                while self.peek().is_ascii_alphanumeric() || self.peek() == b'_' {
                    self.i += 1;
                }
                self.ctx.var(std::str::from_utf8(&self.s[st..self.i]).ok()?)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Repeat {
    /// This many tiles from the group's own on (one if not positive).
    Count(i32),
    /// Tiles covering the exposed rect.
    Infinite,
    /// Tiles from the group's own toward negative coordinates only (`left`, `up`).
    Back,
    /// Tiles from the group's own toward positive coordinates only (`right`, `down`).
    Forward,
}

/// xochitl keeps the modes as negative counts: -1 infinite, -2 up, -3 down, -4 left, -5 right
/// (a number given in the JSON means the same).
const INFINITE: i32 = -1;
const UP: i32 = -2;
const DOWN: i32 = -3;
const LEFT: i32 = -4;
const RIGHT: i32 = -5;

impl Repeat {
    /// The axis's mode from its code; `back` and `forward` are the axis's directional codes.
    fn of(code: i32, back: i32, forward: i32) -> Repeat {
        match code {
            INFINITE => Repeat::Infinite,
            c if c == back => Repeat::Back,
            c if c == forward => Repeat::Forward,
            n => Repeat::Count(n),
        }
    }
}

enum Item {
    Group {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        repeat: Option<(Repeat, Repeat)>,
        children: Vec<Item>,
    },
    Path {
        stroke_width: f32,
        stroke: u32,
        fill: u32,
        segs: Vec<Seg>,
    },
    Text {
        run: TextRun,
        x: f64,
        y: f64,
        color: u32,
        rotation: f64,
    },
}

fn color(v: Option<&Value>, default: u32) -> Option<u32> {
    match v {
        None => Some(default),
        Some(Value::Null) => Some(0),
        Some(Value::String(s)) => {
            let hex = s.strip_prefix('#')?;
            let v = i32::from_str_radix(hex, 16).ok()? as u32;
            match hex.len() {
                6 => Some(v | 0xff00_0000),
                8 => Some(v),
                _ => None,
            }
        }
        _ => None,
    }
}

fn flag(ctx: &Ctx, v: Option<&Value>) -> Option<bool> {
    match v {
        None => Some(false),
        Some(Value::Bool(b)) => Some(*b),
        Some(v) => Some(ctx.eval(v)?.abs() > 1e-5),
    }
}

fn axis(ctx: &Ctx, v: Option<&Value>, keywords: [(&str, i32); 3]) -> i32 {
    match v {
        Some(Value::String(s)) => {
            if let Some(&(_, m)) = keywords.iter().find(|(k, _)| k == s) {
                return m;
            }
            ctx.eval(&Value::String(s.clone())).map_or(0, |v| v as i32)
        }
        Some(Value::Number(n)) => n
            .as_f64()
            .filter(|v| v.fract() == 0.0)
            .map_or(0, |v| v as i32),
        _ => 0,
    }
}

fn path_data(ctx: &Ctx, data: &[Value]) -> Option<Vec<Seg>> {
    let mut segs = vec![];
    let mut i = 0;
    let num = |i: &mut usize| -> Option<f64> {
        let v = ctx.eval(data.get(*i)?);
        *i += 1;
        v
    };
    while i < data.len() {
        let cmd = data[i].as_str()?;
        i += 1;
        segs.push(match cmd {
            "M" => Seg::M(num(&mut i)?, num(&mut i)?),
            "L" => Seg::L(num(&mut i)?, num(&mut i)?),
            "C" => Seg::C(
                num(&mut i)?,
                num(&mut i)?,
                num(&mut i)?,
                num(&mut i)?,
                num(&mut i)?,
                num(&mut i)?,
            ),
            "Z" => Seg::Z,
            _ => return None,
        });
    }
    Some(segs)
}

fn item(ctx: &mut Ctx, o: &Map<String, Value>) -> Option<Item> {
    if let Some(v) = o.get("visible")
        && !flag(ctx, Some(v))?
    {
        return None;
    }
    match o.get("type")?.as_str()? {
        "group" => {
            let b = o.get("boundingBox")?.as_object()?;
            let x = ctx.eval(b.get("x")?)?;
            let y = ctx.eval(b.get("y")?)?;
            let w = ctx.eval(b.get("width")?)?;
            let h = ctx.eval(b.get("height")?)?;
            let repeat = o
                .get("repeat")
                .and_then(|r| r.as_object())
                .filter(|r| !r.is_empty())
                .map(|r| {
                    let columns = axis(
                        ctx,
                        r.get("columns"),
                        [("infinite", INFINITE), ("left", LEFT), ("right", RIGHT)],
                    );
                    let rows = axis(
                        ctx,
                        r.get("rows"),
                        [("infinite", INFINITE), ("up", UP), ("down", DOWN)],
                    );
                    (Repeat::of(columns, LEFT, RIGHT), Repeat::of(rows, UP, DOWN))
                });
            ctx.parents.push(Rect { x, y, w, h });
            let children = o
                .get("children")
                .and_then(|c| c.as_array())
                .map(|c| c.iter().filter_map(|c| item(ctx, c.as_object()?)).collect())
                .unwrap_or_default();
            ctx.parents.pop();
            Some(Item::Group {
                x,
                y,
                w,
                h,
                repeat,
                children,
            })
        }
        "path" => {
            let data = o.get("data")?.as_array().filter(|d| !d.is_empty())?;
            let segs = path_data(ctx, data)?;
            let stroke = color(o.get("strokeColor"), 0xff00_0000)?;
            let fill = color(o.get("fillColor"), 0)?;
            let stroke_width = o.get("strokeWidth").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            if stroke == 0 && fill == 0 {
                return None;
            }
            Some(Item::Path {
                stroke_width,
                stroke,
                fill,
                segs,
            })
        }
        "text" => {
            let text = o.get("text")?.as_str().filter(|t| !t.is_empty())?;
            let size = ctx.eval(o.get("fontSize")?)?;
            if size <= 0.0 {
                return None;
            }
            let color = color(o.get("color"), 0xff00_0000)?;
            let bold = flag(ctx, o.get("bold"))?;
            let italic = flag(ctx, o.get("italic"))?;
            let face = Face::styled(false, bold, italic);
            let run = crate::text::shape_run(ctx.fonts, face, size as u32, text)?;
            let pos = o.get("position")?.as_object()?;
            ctx.text_width = Some(run.ink_width() as f64);
            let (x, y) = (ctx.eval(pos.get("x")?), ctx.eval(pos.get("y")?));
            ctx.text_width = None;
            let rotation = o.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0);
            if rotation.abs() > 360.0 {
                return None;
            }
            Some(Item::Text {
                run,
                x: x?,
                y: y?,
                color,
                rotation,
            })
        }
        _ => None,
    }
}

struct Painter {
    m: [f64; 6],
    dirty: bool,
    draws: Vec<Draw>,
}

fn translate(m: &[f64; 6], x: f64, y: f64) -> [f64; 6] {
    [
        m[0],
        m[1],
        m[2],
        m[3],
        m[4] + x * m[0] + y * m[2],
        m[5] + x * m[1] + y * m[3],
    ]
}

impl Painter {
    fn set(&mut self, m: [f64; 6]) {
        self.m = m;
        self.dirty = true;
    }

    fn draw(&mut self, pen: Option<Pen>, kind: Kind) {
        let block = std::mem::take(&mut self.dirty);
        self.draws.push(Draw {
            m: self.m,
            block,
            pen,
            kind,
        });
    }
}

fn pen(color: u32, width: f64) -> Pen {
    Pen {
        color,
        width,
        cap: 2,
        join: 2,
        dash: vec![],
    }
}

fn paint(p: &mut Painter, it: &Item, rect: Rect) {
    match it {
        Item::Group {
            x,
            y,
            w,
            h,
            repeat,
            children,
        } => {
            let Rect {
                x: rx,
                y: ry,
                w: rw,
                h: rh,
            } = rect;
            let span = |r0: f64, rs: f64, o: f64, s: f64| {
                (
                    (((r0 - o) / s).floor()) as i32,
                    (((r0 + rs) - (s + o)) / s).ceil() as i32,
                )
            };
            let tiles = |m: Repeat, r0: f64, rs: f64, o: f64, s: f64| match m {
                Repeat::Count(n) => (0, (n - 1).max(0)),
                _ => {
                    let (a, b) = span(r0, rs, o, s);
                    (
                        if m == Repeat::Forward { a.max(0) } else { a },
                        if m == Repeat::Back { b.min(0) } else { b },
                    )
                }
            };
            let (c0, c1, r0, r1) = match *repeat {
                None => (0, 0, 0, 0),
                Some((cm, rm)) => {
                    let (c0, c1) = tiles(cm, rx, rw, *x, *w);
                    let (r0, r1) = tiles(rm, ry, rh, *y, *h);
                    (c0, c1, r0, r1)
                }
            };
            let saved = p.m;
            for r in r0..=r1 {
                for c in c0..=c1 {
                    let tx = x + c as f64 * w;
                    let ty = y + r as f64 * h;
                    p.set(translate(&saved, tx, ty));
                    let (ax, ay) = (rx - tx, ry - ty);
                    let (l, t) = (ax.max(0.0), ay.max(0.0));
                    let (rr, b) = ((ax + rw).min(*w), (ay + rh).min(*h));
                    let sub = if l < rr && t < b {
                        Rect {
                            x: l,
                            y: t,
                            w: rr - l,
                            h: b - t,
                        }
                    } else {
                        Rect::default()
                    };
                    for ch in children {
                        paint(p, ch, sub);
                    }
                }
            }
            p.set(saved);
        }
        Item::Path {
            stroke_width,
            stroke,
            fill,
            segs,
        } => {
            let pen = if *stroke_width == 0.0 && *stroke == 0xff00_0000 && *fill == 0 {
                Some(pen(0xffc0_c0c0, 0.5))
            } else if *stroke != 0 {
                Some(pen(
                    *stroke,
                    if *stroke_width == 0.0 {
                        0.5
                    } else {
                        *stroke_width as f64
                    },
                ))
            } else {
                None
            };
            let fill = (*fill != 0).then_some(*fill);
            p.draw(
                pen,
                Kind::Path {
                    fill,
                    segs: segs.clone(),
                },
            );
        }
        Item::Text {
            run,
            x,
            y,
            color,
            rotation,
        } => {
            let pen = Some(pen(*color, 1.0));
            let mut run = run.clone();
            run.color = *color;
            if *rotation != 0.0 {
                let saved = p.m;
                let m = translate(&saved, *x, *y);
                let (s, c) = rotation.to_radians().sin_cos();
                p.set([
                    c * m[0] + s * m[2],
                    c * m[1] + s * m[3],
                    -s * m[0] + c * m[2],
                    -s * m[1] + c * m[3],
                    m[4],
                    m[5],
                ]);
                run.x = 0.0;
                run.y = 0.0;
                p.draw(pen, Kind::Text(run));
                p.set(saved);
            } else {
                run.x = *x;
                run.y = *y;
                p.draw(pen, Kind::Text(run));
            }
        }
    }
}

pub fn render(json: &str, pw: f64, ph: f64, paper: Paper, fonts: &mut Fonts) -> Option<Vec<Draw>> {
    let root: Value = serde_json::from_str(json).ok()?;
    let root = root.as_object()?;
    if root.get("formatVersion")?.as_i64()? != 1 {
        return None;
    }
    let (pw_, ph_) = paper.page(Orientation::parse(
        root.get("orientation").and_then(|o| o.as_str()),
    ));
    let (tw, th) = (pw_ as f64, ph_ as f64);
    let mut ctx = Ctx {
        template_w: tw,
        template_h: th,
        scene: Rect {
            x: -(paper.h as f64) / 2.0,
            y: 0.0,
            w: paper.h as f64,
            h: paper.w as f64,
        },
        dpi: paper.dpi(),
        constants: vec![],
        parents: vec![],
        text_width: None,
        fonts,
    };
    for c in root
        .get("constants")
        .and_then(|c| c.as_array())
        .into_iter()
        .flatten()
    {
        for (k, v) in c.as_object()? {
            let v = ctx.eval(v)?;
            ctx.constants.push((k.clone(), v));
        }
    }
    let items: Vec<Item> = root
        .get("items")?
        .as_array()?
        .iter()
        .filter_map(|i| item(&mut ctx, i.as_object()?))
        .collect();
    let mut p = Painter {
        m: [1.0, 0.0, 0.0, 1.0, pw / 2.0 - tw / 2.0, 0.0],
        dirty: true,
        draws: vec![],
    };
    let rect = Rect {
        x: tw / 2.0 - pw / 2.0,
        y: 0.0,
        w: pw,
        h: ph,
    };
    for it in &items {
        paint(&mut p, it, rect);
    }
    p.set([1.0, 0.0, 0.0, 1.0, pw / 2.0, 0.0]);
    p.draw(Some(DEFAULT_PEN), Kind::Flush);
    Some(p.draws)
}
