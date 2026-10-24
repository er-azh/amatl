//! Whole-document export: a notebook's pages, in `.content` order with their templates, through
//! the page pipeline (layout text, rasterize, trace, write PDF).
use std::path::PathBuf;

mod page;

pub use page::{Size, draw, page};

use crate::format::Scene;
use crate::render::Paper;
use crate::rmdoc::{Archive, pages};
use crate::text::Fonts;

/// Files copied from the device: the fonts xochitl embeds in exports and the templates directory
/// (`/usr/share/remarkable/templates`). Without them, text and file templates are left out.
#[derive(Default)]
pub struct Assets {
    pub fonts: Fonts,
    pub templates: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// A document this exporter doesn't render (PDF or EPUB backed); the caller may fall back.
    #[error("unsupported document type {0:?}")]
    Unsupported(String),
    #[error(transparent)]
    Document(#[from] crate::rmdoc::Error),
    #[error("page {id}: {source}")]
    Page { id: String, source: crate::Error },
    #[error("{id}.pdf: {source}")]
    Pdf {
        id: String,
        source: crate::pdf::overlay::Error,
    },
}

/// Export an `.rmdoc` (or any zip of a document's sync files) as PDF. Warnings (missing fonts or
/// templates, unparsed data) go to `warn`.
pub fn rmdoc(
    zip: &[u8],
    assets: &mut Assets,
    warn: &mut dyn FnMut(String),
) -> Result<Vec<u8>, ExportError> {
    let doc = Archive::open(zip)?;
    let (file_type, orientation, refs) = pages(doc.content()?, doc.pagedata())?;
    let scenes = refs
        .iter()
        .map(|p| match doc.page(&p.id) {
            Some(data) => Scene::parse(data).map_err(|source| ExportError::Page {
                id: p.id.clone(),
                source,
            }),
            None => Ok(Scene::default()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    // pages without a `.rm` file (or a paper size) take the document's.
    let paper = scenes.iter().find_map(Paper::of).unwrap_or(Paper::RM2);
    if file_type != "notebook" {
        // epubs are annotated over the pdf the tablet converts them to.
        let Some(original) = doc.pdf() else {
            return Err(ExportError::Unsupported(file_type));
        };
        let pdf_error = |source| ExportError::Pdf {
            id: doc.id().to_owned(),
            source,
        };
        let src = crate::pdf::overlay::Source::load(original).map_err(pdf_error)?;
        let mut out = Vec::new();
        for (p, sc) in refs.iter().zip(&scenes) {
            let paper = Paper::of(sc).unwrap_or(paper);
            let warn = &mut |w| warn(format!("page {}: {w}", p.id));
            out.push(
                match p.redir.and_then(|r| src.page_size(r).map(|s| (r, s))) {
                    Some((r, size)) => {
                        let (pg, content) = draw(sc, "", paper, Size::Fixed(size), assets, warn);
                        crate::pdf::overlay::Out::Overlay {
                            index: r,
                            page: pg,
                            content,
                        }
                    }
                    None => {
                        if let Some(r) = p.redir {
                            warn(format!("PDF has no page {r}; exported as a note page"));
                        }
                        crate::pdf::overlay::Out::Note(page(
                            sc,
                            &p.template,
                            orientation,
                            paper,
                            assets,
                            warn,
                        ))
                    }
                },
            );
        }
        return src.write(out).map_err(pdf_error);
    }
    let mut out = Vec::new();
    for (p, sc) in refs.iter().zip(&scenes) {
        let paper = Paper::of(sc).unwrap_or(paper);
        out.push(page(
            sc,
            &p.template,
            orientation,
            paper,
            assets,
            &mut |w| warn(format!("page {}: {w}", p.id)),
        ));
    }
    Ok(crate::pdf::write(&out))
}
