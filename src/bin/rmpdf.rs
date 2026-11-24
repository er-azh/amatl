//! rmpdf [--fonts DIR] [--templates DIR] OUT.pdf [-t TEMPLATE] PAGE.rm...: export pages the
//! way xochitl's PDF exporter does.
//! `-t` sets the template (the `.content` page entry's name, e.g. "P Hexagon small") of the
//! pages after it; `-L` / `-P` make the following pages landscape / portrait (page size from each page's paper).
//! A page's images (captures) are read from the directory named after it, as in a document
//! (`PAGE/NAME` next to `PAGE.rm`).
#[path = "shared/assets.rs"]
mod assets;

use amatl::export::page;
use amatl::format::Scene;
use amatl::render::Paper;
use amatl::rmdoc::Orientation;

fn main() {
    const USAGE: &str =
        "usage: rmpdf [--fonts DIR] [--templates DIR] OUT.pdf [-t TEMPLATE] PAGE.rm...";
    let (mut assets, args) = assets::parse(std::env::args().skip(1)).expect(USAGE);
    let mut args = args.into_iter();
    let out = args.next().expect(USAGE);
    let mut pages = Vec::new();
    let mut template = String::new();
    let mut orientation = Orientation::Portrait;
    while let Some(src) = args.next() {
        if src == "-L" || src == "-P" {
            orientation = if src == "-L" {
                Orientation::Landscape
            } else {
                Orientation::Portrait
            };
            continue;
        }
        if src == "-t" {
            template = args.next().expect("-t needs a template name");
            continue;
        }
        let sc =
            Scene::parse(&std::fs::read(&src).unwrap()).unwrap_or_else(|e| panic!("{src}: {e}"));
        let dir = std::path::PathBuf::from(src.strip_suffix(".rm").unwrap_or(&src));
        let images = |f: &str| std::fs::read(dir.join(f)).ok();
        pages.push(page(
            &sc,
            &template,
            orientation,
            Paper::of(&sc).unwrap_or(Paper::RM2),
            &images,
            &mut assets,
            &mut |w| eprintln!("{src}: warning: {w}"),
        ));
    }
    std::fs::write(&out, amatl::pdf::write(&pages)).unwrap();
}
