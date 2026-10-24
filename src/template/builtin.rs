//! The templates xochitl draws in code (lines, grids, dots, hexagons, calligraphy, ...).

use super::{DEFAULT_PEN, Draw, Kind, Pen, Seg};
use crate::format::Rect;

fn lines_to_draws(w: f64, groups: Vec<Lines>) -> Vec<Draw> {
    let mut out = vec![];
    for g in groups {
        let pen = Pen {
            color: 0xffc0_c0c0,
            width: 0.5,
            cap: 2,
            join: 2,
            dash: g.dash.map(|d| d.to_vec()).unwrap_or_default(),
        };
        for (i, l) in g.lines.iter().enumerate() {
            out.push(Draw {
                m: [1.0, 0.0, 0.0, 1.0, w / 2.0, 0.0],
                block: i == 0,
                pen: Some(pen.clone()),
                kind: Kind::Path {
                    fill: None,
                    segs: vec![Seg::M(l[0], l[1]), Seg::L(l[2], l[3])],
                },
            });
        }
    }
    out
}

struct Lines {
    dash: Option<[f64; 2]>,
    lines: Vec<[f64; 4]>,
}

struct Tiles {
    w: f64,
    h: f64,
    x: f64,
    y: f64,
}

pub(super) fn scene(paper: crate::render::Paper) -> (f64, f64) {
    (-(paper.h as f64) / 2.0, paper.h as f64)
}

pub(super) fn builtin(name: &str, w: f64, h: f64, scene: (f64, f64)) -> Option<Vec<Draw>> {
    let exposed = Rect {
        x: -w / 2.0,
        y: 0.0,
        w,
        h,
    };
    let groups = match name {
        "P Hexagon large" => hexagons(
            Tiles {
                w: 112.1,
                h: 194.4,
                x: -102.1,
                y: -147.4,
            },
            scene,
            exposed,
        ),
        "P Hexagon medium" => hexagons(
            Tiles {
                w: 87.8,
                h: 151.8,
                x: -30.0,
                y: -138.0,
            },
            scene,
            exposed,
        ),
        "P Hexagon small" => hexagons(
            Tiles {
                w: 43.9,
                h: 75.9,
                x: -8.0,
                y: -55.2,
            },
            scene,
            exposed,
        ),
        "P Calligraphy large" => calligraphy(
            4,
            Tiles {
                w: 72.0,
                h: 195.0,
                x: -50.5,
                y: 65.0,
            },
            scene,
            exposed,
        ),
        "P Calligraphy medium" => calligraphy(
            5,
            Tiles {
                w: 45.0,
                h: 157.5,
                x: -35.0,
                y: 99.0,
            },
            scene,
            exposed,
        ),
        "LS Calligraphy large" => calligraphy(
            6,
            Tiles {
                w: 72.0,
                h: 195.0,
                x: -105.0,
                y: 61.0,
            },
            scene,
            exposed,
        ),
        "LS Calligraphy medium" => calligraphy(
            7,
            Tiles {
                w: 45.0,
                h: 159.0,
                x: 0.0,
                y: 64.0,
            },
            scene,
            exposed,
        ),
        _ => return None,
    };
    let mut draws = vec![];
    let gray = Pen {
        color: 0xffc0_c0c0,
        width: 0.5,
        cap: 2,
        join: 2,
        dash: vec![],
    };
    let flush = |pen: &Pen, block| Draw {
        m: [1.0, 0.0, 0.0, 1.0, w / 2.0, 0.0],
        block,
        pen: Some(pen.clone()),
        kind: Kind::Flush,
    };
    let calligraphy = name.contains("Calligraphy");
    if calligraphy {
        draws.push(flush(&gray, true));
    }
    draws.extend(lines_to_draws(w, groups));
    if calligraphy {
        draws.push(flush(&gray, false));
    }
    draws.push(flush(&DEFAULT_PEN, false));
    Some(draws)
}

fn hexagons(
    t: Tiles,
    (left, width): (f64, f64),
    Rect {
        x: ex,
        y: ey,
        w: ew,
        h: eh,
    }: Rect,
) -> Vec<Lines> {
    let mut lines = Vec::new();
    let r0 = (((ey - t.y) / t.h) as i32).max(0);
    let r1 = ((((ey + eh - t.y) as i32) as f64) / t.h + 1.0) as i32;
    let c1 = ((((width - t.x) as i32) as f64) / t.w) as i32;
    for r in r0..=r1 {
        let y = t.y + r as f64 * t.h;
        for c in 0..=c1 {
            let x = t.x + left + c as f64 * t.w;
            if x < ex - t.w || x >= ex + ew {
                continue;
            }
            let (mx, rx) = (x + t.w * 0.5, x + t.w);
            let (y1, y2, y3) = (t.h / 6.0 + y, y + t.h * 0.5, (t.h + t.h) / 3.0 + y);
            lines.extend([
                [x, y, mx, y1],
                [mx, y1, mx, y2],
                [mx, y1, rx, y],
                [x, y3, mx, y2],
                [mx, y2, rx, y3],
                [rx, y3, rx, y + t.h],
            ]);
        }
    }
    vec![Lines { dash: None, lines }]
}

#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn calligraphy(
    kind: u32,
    t: Tiles,
    (left, width): (f64, f64),
    Rect {
        x: ex,
        y: ey,
        w: ew,
        h: eh,
    }: Rect,
) -> Vec<Lines> {
    let large = kind & !2 == 4;
    let (inset, unit, slant) = match kind {
        5 => (0.0, 40.2, 12.0),
        7 => (12.0, 40.6, 12.0),
        _ => (5.0, 48.0, 0.0),
    };
    let right = left + width;
    let x0 = if large { ex } else { left } + inset;
    let c1 = ((((width - t.x) as i32) as f64) / t.w) as i32;
    let r0 = (((ey - t.y) / t.h) as i32).max(0);
    let r1 = ((((ey + eh - t.y) as i32) as f64) / t.h + 1.0) as i32;
    let mut groups = Vec::new();
    for r in r0..=r1 {
        let y = t.y + r as f64 * t.h;
        let base = unit + unit + y;
        let low = base + unit;
        groups.push(Lines {
            dash: Some([6.5, 8.5]),
            lines: vec![
                [x0, y, right, y],
                [x0, y + unit, right, y + unit],
                [x0, low, right, low],
            ],
        });
        let mut lines = vec![[ex, base, right, base]];
        if large {
            for c in 0..=c1 {
                let x = left + t.x + c as f64 * t.w;
                if !(x < ex - 30.0) && x < ex + ew {
                    lines.push([x, low, x + t.w + slant, y]);
                }
            }
        } else {
            let mut drift = 0.0;
            for c in 0..=c1 {
                if c > 0 && c % 2 == 0 {
                    drift += 1.0;
                }
                let x = (left + t.x + c as f64 * t.w) - drift;
                lines.push([x, low, x + t.w + slant, y]);
            }
        }
        groups.push(Lines { dash: None, lines });
    }
    groups
}
