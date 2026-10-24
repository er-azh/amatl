# amatl
A reimplementation of reMarkable's scene rendering pipeline in Rust. It can produce almost identical renderings of reMarkable's V6 `.rm` files down to the pixel and PDF draw command level.

The pipeline was originally based on xochitl (reMarkable's UI application) version `3.25.0.1270`, and was later updated to support everything up to version `3.29.0.153`, which is the latest stable version at the time of writing.

## Usage and required assets
Most users will probably want the [rmexport](src/bin/rmexport) binary, which handles the normal PDF export pipeline. To build the binaries, run:
```sh
cargo build --release
```

> [!IMPORTANT]
> To render text and templates properly, you need to extract reMarkable's fonts and default templates from your tablet. Without them, text and templates are skipped with a warning.

To extract fonts, connect your tablet via USB and run:
```sh
scp root@10.11.99.1:/usr/bin/xochitl /tmp/xochitl
./target/release/rmexport extract-fonts /tmp/xochitl ./fonts
```
> [!TIP]
> You can find your SSH password under Settings -> Help -> Copyrights and licenses.

You'll also need to copy your templates over if you use them. For the default ones, run:
```sh
scp -r root@10.11.99.1:/usr/share/remarkable/templates ./
```
You can then pass them to the binaries via env vars (`RM_FONTS` and `RM_TEMPLATES`), or via the `--fonts` and `--templates` flags. For example:
```sh
# you can use `-` for stdin/stdout
./target/release/rmexport --fonts ./fonts --templates ./templates Notebook.rmdoc Notebook.pdf
```
`rmdoc`s from both the USB web interface (http://10.11.99.1) and [rmfakecloud](https://github.com/ddvk/rmfakecloud)'s exporter work and are fully tested.

## Features and limitations
- All tools and text types as of `3.29.0.153` are tested and fully supported.
- All built-in and custom templates are supported, but SVG backgrounds in templates are not; the PNG fallback is used instead.
- Annotated (mixed) documents and EPUBs are fully supported, but note that rendering EPUBs requires an existing PDF render, which xochitl automatically generates when you open an EPUB.
- Rasterization is also supported. See [rmrender](src/bin/rmrender.rs).
- Scene format V5 and older are not tested or supported; only V6 is.
- Notebooks from devices other than the reMarkable 2 remain untested. They _should_ work, but I don't own a device other than the rM2 to test with. If you run into problems with them, feel free to open an issue and attach your notebook and its PDF export, and mention your xochitl version.

## Other binaries
These binaries also serve as examples of using this crate:
- `rmdump`: Dumps the raw contents of a scene file.
- `rmpdf`: Renders a single `.rm` page into a PDF.
- `rmrender`: Rasterizes a single `.rm` page into a PNG.
- `rmtext`: Prints the text data in a single `.rm` file.
- `rmvec`: Rasterizes and vectorizes a single `.rm` page and dumps the vectors as JSON.

## Alternatives
- [rmscene](https://github.com/ricklupton/rmscene) and [rmc](https://github.com/ricklupton/rmc): Support more output formats, but their rendering is simpler and might not support notebooks using newer features.
- [librm_lines](https://github.com/RedTTGMoss/librm_lines): Supports wasm, real-time rendering and the older V5 format with a pipeline much closer to xochitl's, but might not support things like specific built-in or custom templates.

## Using the crate
Export an rmdoc notebook to a PDF:
```rust
use amatl::export::{Assets, rmdoc};
use amatl::text::Fonts;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut assets = Assets {
        fonts: Fonts::new("fonts"),
        templates: Some("templates".into()),
    };
    let data = std::fs::read("Notebook.rmdoc")?;
    let pdf = rmdoc(&data, &mut assets, &mut |w| eprintln!("warning: {w}"))?;
    std::fs::write("Notebook.pdf", pdf)?;
    Ok(())
}
```
or just a single page:
```rust
use amatl::export::{Assets, page};
use amatl::format::Scene;
use amatl::render::Paper;
use amatl::rmdoc::Orientation;

let mut assets = Assets::default();
let scene = Scene::parse(&std::fs::read("page.rm")?)?;
let paper = Paper::of(&scene).unwrap_or(Paper::RM2);
let pg = page(&scene, "P Lines small", Orientation::Portrait, paper, &mut assets, &mut |w| eprintln!("{w}"));
std::fs::write("page.pdf", amatl::pdf::write(&[pg]))?;
```
The template name and orientation can be found in the document's `.content` file.
See `src/bin` for rasterizing, reading text or parsing scene files.
> [!WARNING]
> The API surface is extremely rough and might change in the future.

## Acknowledgements
[rmscene](https://github.com/ricklupton/rmscene) helped a lot in parsing the actual scene files. [Quill](https://github.com/CrimsonAS/quill) was a huge help in understanding reMarkable's stroker, and [libjpeg-turbo](https://github.com/libjpeg-turbo/libjpeg-turbo) was also used as the reference for the JPEG encoder in this repo.

## Legal and LLM notes
reMarkable is a trademark of reMarkable AS. This project heavily relies on reverse engineering reMarkable's xochitl; however, I do not ship any copyrighted assets. Users need to extract them from their own devices.
> [!NOTE]
> LLMs, especially Claude Opus 5.5, were heavily used to understand xochitl's code and port it, along with parts of libraries like libjpeg-turbo, to Rust. Major parts of the code here were written by LLMs.

## License
CC0-1.0. See [LICENSE](LICENSE).

The exception is the JPEG encoder in [src/pdf/jpeg.rs](src/pdf/jpeg.rs), which is a translation of
libjpeg-turbo and remains under libjpeg-turbo's IJG License. See [libjpeg-turbo.ijg](licenses/libjpeg-turbo.ijg).
