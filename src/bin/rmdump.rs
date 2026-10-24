//! rmdump [--points] FILE.rm: print the decoded scene tree (`--points`: with each line's points).
use amatl::format::{ItemValue, Node, Scene, TextValue};

fn print_node(sc: &Scene, n: &Node, depth: usize, points: bool) {
    let pad = "  ".repeat(depth);
    let anchor = n
        .anchor_id
        .as_ref()
        .map(|a| {
            format!(
                " anchor={} type={:?} thr={:?} ox={:?}",
                a.value,
                n.anchor_type.as_ref().map(|v| v.value),
                n.anchor_threshold.as_ref().map(|v| v.value),
                n.anchor_origin_x.as_ref().map(|v| v.value)
            )
        })
        .unwrap_or_default();
    println!(
        "{pad}group {} {:?} visible={}{anchor}",
        n.id, n.label.value, n.visible.value
    );
    for c in &n.children {
        let Some(it) = sc.item(n.id, *c) else {
            continue;
        };
        match it.value.as_ref().unwrap() {
            ItemValue::Group(g) => match sc.nodes.get(g) {
                Some(child) => print_node(sc, child, depth + 1, points),
                None => println!("{pad}  missing group {g}"),
            },
            ItemValue::Line(l) => {
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for p in &l.points {
                    x0 = x0.min(p.x);
                    y0 = y0.min(p.y);
                    x1 = x1.max(p.x);
                    y1 = y1.max(p.y);
                }
                println!(
                    "{pad}  line {} tool={:?} color={:?} argb={:?} thick={} n={} bbox=({x0:.1},{y0:.1})-({x1:.1},{y1:.1})",
                    it.id,
                    l.tool,
                    l.color,
                    l.argb.map(|a| format!("{a:08x}")),
                    l.thickness_scale,
                    l.points.len()
                );
                if points {
                    for p in &l.points {
                        println!(
                            "{pad}    {} {} speed={} width={} dir={} pressure={}",
                            p.x, p.y, p.speed, p.width, p.direction, p.pressure
                        );
                    }
                }
            }
            ItemValue::Glyph(g) => println!("{pad}  glyph {} {:?}", it.id, g.text),
            ItemValue::Unknown(k, v) => println!("{pad}  unknown kind {k} ({} bytes)", v.len()),
        }
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let points = args.first().is_some_and(|a| a == "--points");
    if points {
        args.remove(0);
    }
    let [path] = &args[..] else {
        panic!("usage: rmdump [--points] FILE.rm")
    };
    let data = std::fs::read(path).unwrap();
    let sc = Scene::parse(&data).unwrap_or_else(|e| panic!("{e}"));
    println!(
        "authors {:?}\npage info {:?}\ninfo {:?}",
        sc.author_ids, sc.page_info, sc.info
    );
    if let Some(root) = sc.nodes.get(&amatl::format::ROOT_ID) {
        print_node(&sc, root, 0, points);
    }
    if let Some(t) = &sc.text {
        println!("text at ({}, {}) width {}", t.x, t.y, t.width);
        for it in &t.items {
            if let Some(v) = &it.value {
                match v {
                    TextValue::Str(s) => println!("  {} {:?}", it.id, s),
                    TextValue::Format(c) => println!("  {} format {c}", it.id),
                }
            }
        }
        for (k, v) in &t.styles {
            println!("  style @{k}: {:?}", v.value);
        }
    }
    for w in &sc.warnings {
        println!("WARNING {w}");
    }
}
