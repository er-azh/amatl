//! The bins' asset options: `--fonts DIR` and `--templates DIR`, defaulting to `RM_FONTS` and
//! `RM_TEMPLATES`.
use std::path::PathBuf;

use amatl::export::Assets;
use amatl::text::Fonts;

/// The assets the arguments (or the environment) name, and the remaining arguments; `None` if
/// a flag lacks its directory.
pub fn parse(args: impl Iterator<Item = String>) -> Option<(Assets, Vec<String>)> {
    let mut fonts: Option<PathBuf> = std::env::var_os("RM_FONTS").map(Into::into);
    let mut templates: Option<PathBuf> = std::env::var_os("RM_TEMPLATES").map(Into::into);
    let mut rest = Vec::new();
    let mut args = args;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--fonts" => fonts = Some(args.next()?.into()),
            "--templates" => templates = Some(args.next()?.into()),
            _ => rest.push(a),
        }
    }
    let fonts = fonts.map_or_else(Fonts::default, Fonts::new);
    Some((Assets { fonts, templates }, rest))
}
