//! rmvec [--fonts DIR] FILE.rm: rasterize and trace a page; prints the groups as JSON (device pixels).
#[path = "shared/assets.rs"]
mod assets;

use amatl::format::Scene;
use amatl::render::{Paper, Renderer, anchor_offsets, content_rect};

fn main() {
    const USAGE: &str = "usage: rmvec [--fonts DIR] FILE.rm";
    let (mut assets, args) = assets::parse(std::env::args().skip(1)).expect(USAGE);
    let [src] = &args[..] else { panic!("{USAGE}") };
    let sc = Scene::parse(&std::fs::read(src).unwrap()).unwrap_or_else(|e| panic!("{e}"));
    let paper = Paper::of(&sc).unwrap_or(Paper::RM2);
    let mut r = Renderer::new(paper.w, paper.h);
    r.offsets = anchor_offsets(
        &sc,
        &sc.text
            .as_ref()
            .map(|t| amatl::text::layout(t, &mut assets.fonts))
            .unwrap_or_default(),
    );
    let rect = content_rect(&sc, &r.offsets).expect("empty page");
    r.align_to(rect);
    r.draw_scene(&sc);
    let groups = amatl::trace::vectorize(
        &r.canvas.px,
        None,
        r.canvas.w as i32,
        r.canvas.h as i32,
        r.image().unwrap(),
    );
    let js: Vec<String> = groups
        .iter()
        .map(|g| {
            let sp: Vec<String> = g
                .subpaths
                .iter()
                .map(|s| {
                    format!(
                        "[{}]",
                        s.iter()
                            .map(|p| format!("[{},{}]", p.x as f64 / 8.0, p.y as f64 / 8.0))
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                })
                .collect();
            format!("{{\"color\":{},\"subpaths\":[{}]}}", g.color, sp.join(","))
        })
        .collect();
    println!("[{}]", js.join(",\n"));
}
