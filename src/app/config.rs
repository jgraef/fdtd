use serde::{
    Deserialize,
    Serialize,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub recently_opened_files_limit: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            recently_opened_files_limit: 10,
        }
    }
}
