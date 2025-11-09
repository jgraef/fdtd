pub mod nec;

use std::path::Path;

use strum::VariantArray;

pub fn guess_file_format_from_path(path: impl AsRef<Path>) -> Option<FileFormat> {
    let path = path.as_ref();
    let ext = path.extension()?.to_str()?;

    FileFormat::VARIANTS
        .iter()
        .find(|known| known.file_extensions().contains(&ext))
        .copied()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, strum::VariantArray)]
#[non_exhaustive]
pub enum FileFormat {
    Nec,
}

impl FileFormat {
    pub fn file_extensions(&self) -> &'static [&'static str] {
        match self {
            FileFormat::Nec => &["nec"],
        }
    }
}
