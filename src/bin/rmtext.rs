//! rmtext [--fonts DIR] FILE.rm: print the root text layout: per line the baseline and each glyph id with its origin, in page
//! pixels (the coordinates `re/tools/textref.py` reports for xochitl's PDFs).
#[path = "shared/assets.rs"]
mod assets;

use amatl::format::Scene;
use amatl::render::Paper;
use amatl::text::{layout, to_px};

fn main() {
    const USAGE: &str = "usage: rmtext [--fonts DIR] FILE.rm";
    let (mut assets, args) = assets::parse(std::env::args().skip(1)).expect(USAGE);
    let [path] = &args[..] else { panic!("{USAGE}") };
    let sc = Scene::parse(&std::fs::read(path).unwrap()).unwrap_or_else(|e| panic!("{e}"));
    let Some(t) = &sc.text else { return };
    let (ox, oy) = (
        t.x as f32 + Paper::of(&sc).unwrap_or(Paper::RM2).w as f32 / 2.0,
        t.y as f32,
    );
    for l in layout(t, &mut assets.fonts).lines {
        println!("BT {} {}", l.format.face.name(), l.format.px);
        for g in &l.glyphs {
            println!(
                "  {:10.4} {:10.4}  {}",
                ox + to_px(g.x),
                oy + to_px(l.baseline),
                g.id
            );
        }
    }
}
