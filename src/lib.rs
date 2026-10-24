//! Reader and PDF exporter for reMarkable v6 ("scene") .rm files, following xochitl's pipeline:
//! parse scene -> rasterize strokes -> trace bitmaps into polygons -> write PDF.

pub mod export;
pub mod format;
pub mod pdf;
pub mod render;
pub mod rmdoc;
pub(crate) mod template;
pub mod text;
pub mod trace;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("format error at {offset:#x}: {msg}")]
    Format { offset: usize, msg: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
