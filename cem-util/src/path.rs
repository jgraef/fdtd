use std::{
    fmt::Display,
    path::Path,
};

pub use directories::{
    BaseDirs,
    ProjectDirs,
    UserDirs,
};

/// Format a path for display
pub fn format_path<P>(path: P) -> FormatPath<P>
where
    P: AsRef<Path>,
{
    FormatPath { path }
}

#[derive(Clone, Copy, Debug)]
pub struct FormatPath<P> {
    pub path: P,
}

impl<P> Display for FormatPath<P>
where
    P: AsRef<Path>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path = self.path.as_ref();

        if let Some(user_dirs) = UserDirs::new() {
            let home = user_dirs.home_dir();

            if let Ok(relative_path) = path.strip_prefix(home) {
                return write!(f, "~/{}", relative_path.to_string_lossy());
            }
        }

        write!(f, "{}", path.to_string_lossy())
    }
}
