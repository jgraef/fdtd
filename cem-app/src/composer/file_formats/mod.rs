pub mod nec;
pub mod obj;
pub mod project_file;

use std::{
    collections::HashMap,
    ffi::OsStr,
    path::Path,
    sync::OnceLock,
};

use either::Either;
use strum::VariantArray;
use unicase::UniCase;

pub fn guess_file_format_from_path(path: impl AsRef<Path>) -> Option<FileFormat> {
    let path = path.as_ref();
    let file_extension = path.extension()?;
    FileFormatExtensions::global().get(file_extension).next()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, strum::VariantArray)]
#[non_exhaustive]
pub enum FileFormat {
    Cem,
    Nec,
}

impl FileFormat {
    pub fn iter() -> impl Iterator<Item = Self> {
        Self::VARIANTS.iter().copied()
    }

    pub fn file_extensions(&self) -> &'static [&'static str] {
        match self {
            Self::Cem => &["cem"],
            Self::Nec => &["nec"],
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Cem => "CEM Project File",
            Self::Nec => "NEC File",
        }
    }

    pub fn can_open(&self) -> bool {
        match self {
            Self::Cem => true,
            Self::Nec => true,
        }
    }

    pub fn can_save(&self) -> bool {
        matches!(self, Self::Cem)
    }

    pub fn canonical_file_extension(&self) -> &'static str {
        self.file_extensions()[0]
    }
}

#[derive(Clone, Debug)]
struct FileFormatExtensions {
    table: HashMap<UniCase<&'static str>, Vec<FileFormat>>,
}

impl FromIterator<FileFormat> for FileFormatExtensions {
    fn from_iter<T: IntoIterator<Item = FileFormat>>(iter: T) -> Self {
        let mut table: HashMap<UniCase<&'static str>, Vec<FileFormat>> = HashMap::new();

        for file_format in iter {
            for extension in file_format.file_extensions() {
                let entry = table.entry(UniCase::new(*extension)).or_default();
                entry.push(file_format);
            }
        }

        Self { table }
    }
}

impl FileFormatExtensions {
    pub fn global() -> &'static Self {
        static TABLE: OnceLock<FileFormatExtensions> = OnceLock::new();
        TABLE.get_or_init(|| {
            let mut table = FileFormat::VARIANTS.iter().copied().collect::<Self>();
            table.table.shrink_to_fit();
            table
        })
    }

    pub fn get<'a>(&'a self, file_extension: &'a OsStr) -> impl Iterator<Item = FileFormat> + 'a {
        if let Some(extension) = file_extension.to_str()
            && let Some(entry) = self.table.get(&UniCase::new(extension))
        {
            Either::Left(entry.iter().copied())
        }
        else {
            Either::Right([].into_iter())
        }
    }
}
