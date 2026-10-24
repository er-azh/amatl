//! rmrender [--fonts DIR] FILE.rm OUT.png: rasterize a page's strokes at device resolution (227 dpi).
#[path = "shared/assets.rs"]
mod assets;

use amatl::format::Scene;
use amatl::render::{Paper, Renderer, anchor_offsets, content_rect};

fn main() {
    const USAGE: &str = "usage: rmrender [--fonts DIR] FILE.rm OUT.png";
    let (mut assets, args) = assets::parse(std::env::args().skip(1)).expect(USAGE);
    let [src, out] = &args[..] else {
        panic!("{USAGE}")
    };
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
    if let Some(rect) = content_rect(&sc, &r.offsets) {
        let (fx, fy) = r.align_to(rect);
        println!(
            "content rect ({:?}, {:?}, {:?}, {:?}); geometry shifted by ({fx}, {fy})",
            rect.x, rect.y, rect.w, rect.h
        );
    }
    r.draw_scene(&sc);
    r.canvas.write_png(out).unwrap();
    for w in &r.warnings {
        eprintln!("warning: {w}");
    }
}
