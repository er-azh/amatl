//! Loading the font files of the text styles.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

/// The reMarkable fonts that text and templates use, each in `{name}.ttf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    SansRegular,
    SansMedium,
    SansBold,
    SansItalic,
    SansBoldItalic,
    SerifRegular,
    SerifBold,
    SerifItalic,
    SerifBoldItalic,
}

impl Face {
    /// The regular sans or serif face, made bold and/or italic.
    pub fn styled(serif: bool, bold: bool, italic: bool) -> Face {
        use Face::*;
        match (serif, bold, italic) {
            (false, false, false) => SansRegular,
            (false, true, false) => SansBold,
            (false, false, true) => SansItalic,
            (false, true, true) => SansBoldItalic,
            (true, false, false) => SerifRegular,
            (true, true, false) => SerifBold,
            (true, false, true) => SerifItalic,
            (true, true, true) => SerifBoldItalic,
        }
    }

    pub fn is_serif(self) -> bool {
        use Face::*;
        matches!(
            self,
            SerifRegular | SerifBold | SerifItalic | SerifBoldItalic
        )
    }

    /// The PostScript name.
    pub fn name(self) -> &'static str {
        self.names().0
    }

    /// The file name.
    pub fn file(self) -> &'static str {
        self.names().1
    }

    fn names(self) -> (&'static str, &'static str) {
        use Face::*;
        match self {
            SansRegular => ("reMarkableSans-Regular", "reMarkableSans-Regular.ttf"),
            SansMedium => ("reMarkableSans-Medium", "reMarkableSans-Medium.ttf"),
            SansBold => ("reMarkableSans-Bold", "reMarkableSans-Bold.ttf"),
            SansItalic => (
                "reMarkableSans-RegularItalic",
                "reMarkableSans-RegularItalic.ttf",
            ),
            SansBoldItalic => ("reMarkableSans-BoldItalic", "reMarkableSans-BoldItalic.ttf"),
            SerifRegular => (
                "reMarkableSerifSmall-Regular",
                "reMarkableSerifSmall-Regular.ttf",
            ),
            SerifBold => ("reMarkableSerifSmall-Bold", "reMarkableSerifSmall-Bold.ttf"),
            SerifItalic => (
                "reMarkableSerifSmall-RegularItalic",
                "reMarkableSerifSmall-RegularItalic.ttf",
            ),
            SerifBoldItalic => (
                "reMarkableSerifSmall-BoldItalic",
                "reMarkableSerifSmall-BoldItalic.ttf",
            ),
        }
    }
}

/// The font files of the reMarkable text styles, looked up by file name in one directory.
/// Without a directory every font is missing.
#[derive(Default)]
pub struct Fonts {
    dir: Option<PathBuf>,
    cache: HashMap<Face, Option<std::sync::Arc<Vec<u8>>>>,
}

impl Fonts {
    pub fn new(dir: impl AsRef<Path>) -> Fonts {
        Fonts {
            dir: Some(dir.as_ref().to_owned()),
            cache: HashMap::new(),
        }
    }

    /// The face's font file, if it is there.
    pub fn data(&mut self, face: Face) -> Option<std::sync::Arc<Vec<u8>>> {
        let dir = self.dir.as_ref()?;
        self.cache
            .entry(face)
            .or_insert_with(|| {
                std::fs::read(dir.join(face.file()))
                    .ok()
                    .map(std::sync::Arc::new)
            })
            .clone()
    }
}
