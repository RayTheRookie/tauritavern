use crate::infrastructure::database::SqliteRepo;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct AppState {
    pub repo: SqliteRepo,
    pub data_dir: PathBuf,
    pub active_profile_id: Mutex<Option<String>>,
}

impl AppState {
    pub fn new(repo: SqliteRepo, data_dir: PathBuf) -> Self {
        Self {
            repo,
            data_dir,
            active_profile_id: Mutex::new(None),
        }
    }
}
