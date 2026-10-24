//! rmexport [--fonts DIR] [--templates DIR] IN.rmdoc OUT.pdf: export a notebook, or a PDF/EPUB with
//! its annotations, as xochitl does.
//! rmexport extract-fonts XOCHITL DIR: save the fonts compiled into the xochitl binary to DIR.
//! `-` reads the document from stdin or writes the PDF to stdout. The fonts and templates are
//! copied from the device (`RM_FONTS` and `RM_TEMPLATES` are the defaults).
//!
//! Exit status: 0 done, 1 bad document or I/O error, 2 usage, 3 a document type this exporter
//! doesn't render (an EPUB without the converted PDF); the caller should fall back.
#[path = "../shared/assets.rs"]
mod assets;
mod fontscan;

use std::io::{Read, Write};
use std::process::ExitCode;

use amatl::export::{ExportError, rmdoc};

const USAGE: &str = "usage: rmexport [--fonts DIR] [--templates DIR] IN.rmdoc|- OUT.pdf|-\n       rmexport extract-fonts XOCHITL DIR";

fn extract_fonts(bin: &str, dir: &str) -> ExitCode {
    let data = match std::fs::read(bin) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("rmexport: {bin}: {e}");
            return ExitCode::from(1);
        }
    };
    let fonts = fontscan::fonts(&data);
    if fonts.is_empty() {
        eprintln!("rmexport: no fonts found in {bin}");
        return ExitCode::from(1);
    }
    for (name, font) in fonts {
        let path = std::path::Path::new(dir).join(format!("{name}.ttf"));
        if let Err(e) = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, font)) {
            eprintln!("rmexport: {}: {e}", path.display());
            return ExitCode::from(1);
        }
        println!("{}", path.display());
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("extract-fonts") {
        let a: Vec<String> = std::env::args().skip(2).collect();
        let [bin, dir] = &a[..] else {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        };
        return extract_fonts(bin, dir);
    }
    let Some((mut assets, args)) = assets::parse(std::env::args().skip(1)) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let mut paths = Vec::new();
    for a in args {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ => paths.push(a),
        }
    }
    let [input, output] = &paths[..] else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let data = if input == "-" {
        let mut d = Vec::new();
        std::io::stdin().read_to_end(&mut d).map(|_| d)
    } else {
        std::fs::read(input)
    };
    let data = match data {
        Ok(d) => d,
        Err(e) => {
            eprintln!("rmexport: {input}: {e}");
            return ExitCode::from(1);
        }
    };
    let pdf = match rmdoc(&data, &mut assets, &mut |w| {
        eprintln!("rmexport: warning: {w}")
    }) {
        Ok(pdf) => pdf,
        Err(e) => {
            eprintln!("rmexport: {e}");
            return ExitCode::from(if matches!(e, ExportError::Unsupported(_)) {
                3
            } else {
                1
            });
        }
    };
    let written = if output == "-" {
        std::io::stdout().lock().write_all(&pdf)
    } else {
        std::fs::write(output, &pdf)
    };
    if let Err(e) = written {
        eprintln!("rmexport: {output}: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
