//! `.rmdoc` documents: the zip of a document's sync files, its `.content` JSON (file type,
//! orientation, page order and templates) and format 1's `.pagedata`.
mod zip;

pub use zip::Error as ZipError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("zip: {0}")]
    Zip(#[from] zip::Error),
    #[error("no .content file")]
    NoContent,
    #[error(".content is not UTF-8")]
    ContentEncoding,
    #[error(".content: {0}")]
    Content(#[from] serde_json::Error),
    #[error(".content: page id is not a string")]
    PageId,
}

/// A document's files, as unpacked from its zip: `{id}.content`, `{id}.pagedata`, `{id}.pdf`
/// and the pages' `{id}/{page}.rm`.
pub struct Archive {
    id: String,
    files: Vec<(String, Vec<u8>)>,
}

impl Archive {
    /// The archive's files; the document id comes from its top-level `.content` file.
    pub fn open(zip: &[u8]) -> Result<Archive, Error> {
        let files = zip::entries(zip)?;
        let id = files
            .iter()
            .find_map(|(name, _)| name.strip_suffix(".content").filter(|n| !n.contains('/')))
            .ok_or(Error::NoContent)?
            .to_owned();
        Ok(Archive { id, files })
    }

    /// The document id (the name of its files).
    pub fn id(&self) -> &str {
        &self.id
    }

    fn file(&self, name: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, d)| &d[..])
    }

    /// The `.content` JSON.
    pub fn content(&self) -> Result<&str, Error> {
        std::str::from_utf8(
            self.file(&format!("{}.content", self.id))
                .unwrap_or_default(),
        )
        .map_err(|_| Error::ContentEncoding)
    }

    /// The `.pagedata` file, if present and text.
    pub fn pagedata(&self) -> Option<&str> {
        self.file(&format!("{}.pagedata", self.id))
            .and_then(|d| std::str::from_utf8(d).ok())
    }

    /// A page's `.rm` file.
    pub fn page(&self, page: &str) -> Option<&[u8]> {
        self.file(&format!("{}/{page}.rm", self.id))
    }

    /// The PDF a PDF or EPUB backed document annotates.
    pub fn pdf(&self) -> Option<&[u8]> {
        self.file(&format!("{}.pdf", self.id))
    }
}

/// One page of a document: its id (the `.rm` file's name), template name and, in a PDF or
/// EPUB backed document, the index of its background page in the PDF (none for inserted pages).
/// Which way up a page is: the document's `orientation` in `.content`, or a JSON template's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

impl Orientation {
    /// An `orientation` value: `"landscape"`, otherwise portrait.
    pub fn parse(s: Option<&str>) -> Orientation {
        match s {
            Some("landscape") => Orientation::Landscape,
            _ => Orientation::Portrait,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageRef {
    pub id: String,
    pub template: String,
    pub redir: Option<u32>,
}

/// The document's file type, orientation and visible pages in order, from its `.content` JSON
/// (`pagedata`: the `.pagedata` file, holding format 1's templates one per line).
pub fn pages(
    content: &str,
    pagedata: Option<&str>,
) -> Result<(String, Orientation, Vec<PageRef>), Error> {
    let v: serde_json::Value = serde_json::from_str(content)?;
    let file_type = v["fileType"].as_str().unwrap_or("notebook").to_owned();
    let orientation = Orientation::parse(v["orientation"].as_str());
    let mut pages = Vec::new();
    if let Some(list) = v["cPages"]["pages"].as_array() {
        // Format 2: pages ordered by their fractional index; deleted ones stay listed.
        let mut list: Vec<_> = list
            .iter()
            .filter(|p| p["deleted"]["value"].as_i64().unwrap_or(0) == 0)
            .filter_map(|p| {
                let id = p["id"].as_str()?.to_owned();
                let template = p["template"]["value"].as_str().unwrap_or("").to_owned();
                let redir = p["redir"]["value"].as_u64().map(|r| r as u32);
                Some((
                    p["idx"]["value"].as_str().unwrap_or("").to_owned(),
                    PageRef {
                        id,
                        template,
                        redir,
                    },
                ))
            })
            .collect();
        list.sort_by(|a, b| a.0.cmp(&b.0));
        pages = list.into_iter().map(|(_, p)| p).collect();
    } else if let Some(list) = v["pages"].as_array() {
        // Format 1: `redirectionPageMap` holds each page's PDF page, -1 for inserted pages.
        let mut templates = pagedata.unwrap_or("").lines();
        for (i, id) in list.iter().enumerate() {
            let id = id.as_str().ok_or(Error::PageId)?;
            let template = templates.next().unwrap_or("").trim().to_owned();
            let redir = match v["redirectionPageMap"].as_array() {
                Some(map) => map
                    .get(i)
                    .and_then(|r| r.as_i64())
                    .filter(|&r| r >= 0)
                    .map(|r| r as u32),
                None if file_type == "notebook" => None,
                None => Some(i as u32),
            };
            pages.push(PageRef {
                id: id.to_owned(),
                template,
                redir,
            });
        }
    }
    Ok((file_type, orientation, pages))
}
