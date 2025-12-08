use std::{
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

use cem_util::path::ProjectDirs;
use color_eyre::eyre::Context;
use serde::{
    Serialize,
    de::DeserializeOwned,
};

use crate::Error;

#[derive(Clone, Debug)]
pub struct AppFiles {
    project_dirs: Arc<ProjectDirs>,
}

impl AppFiles {
    pub fn new(project_dirs: ProjectDirs) -> Self {
        Self {
            project_dirs: Arc::new(project_dirs),
        }
    }

    pub fn create_directories(&self) -> Result<(), Error> {
        std::fs::create_dir_all(self.state_dir_with_fallback())?;
        std::fs::create_dir_all(self.project_dirs.config_local_dir())?;
        std::fs::create_dir_all(self.screenshots_dir())?;
        Ok(())
    }

    pub fn open() -> Result<Self, Error> {
        let app_files = Self::default();
        app_files.create_directories()?;
        Ok(app_files)
    }

    /// Path to state directory.
    ///
    /// This tries to use the system's canonical state directory. If this is
    /// undefined, it will use the data-local directory.
    pub fn state_dir_with_fallback(&self) -> &Path {
        self.project_dirs
            .state_dir()
            .unwrap_or_else(|| self.project_dirs.data_local_dir())
    }

    pub fn screenshots_dir(&self) -> PathBuf {
        self.project_dirs.data_local_dir().join("screenshots")
    }

    /// Returns path to file for egui's persistence.
    pub fn egui_persist_path(&self) -> PathBuf {
        self.state_dir_with_fallback().join("ui_state")
    }

    /// Read config file, or create one if it doesn't exist yet.
    ///
    /// # TODO
    ///
    /// - What format shall we use? TOML is nice and all, but JSON5 also seems
    ///   fine.
    pub fn read_config_or_create<T>(&self) -> Result<T, Error>
    where
        T: Serialize + DeserializeOwned + Default,
    {
        let path = self.project_dirs.config_local_dir().join("config.toml");

        let config = if !path.exists() {
            tracing::info!(path = %path.display(), "Creating config file");
            let config = T::default();
            let toml = toml::to_string_pretty(&config)?;
            std::fs::write(&path, &toml)
                .with_context(|| format!("Could not write config file: {}", path.display()))?;
            config
        }
        else {
            tracing::info!(path = %path.display(), "Reading config file");
            let toml = std::fs::read(&path)
                .with_context(|| format!("Could not read config file: {}", path.display()))?;

            toml::from_slice(&toml)
                .with_context(|| format!("Invalid config file: {}", path.display()))?
        };

        Ok(config)
    }

    pub fn mipmap_cache_path(&self) -> PathBuf {
        self.project_dirs.cache_dir().join("mipmaps")
    }
}

impl Default for AppFiles {
    fn default() -> Self {
        Self::new(
            ProjectDirs::from("", "switch", std::env!("CARGO_PKG_NAME"))
                .expect("Failed to create ProjectDirs"),
        )
    }
}
