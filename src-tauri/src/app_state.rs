use crate::infrastructure::database::SqliteRepo;
use std::path::PathBuf;

pub struct AppState {
    pub repo: SqliteRepo,
    pub data_dir: PathBuf,
}

impl AppState {
    pub fn new(repo: SqliteRepo, data_dir: PathBuf) -> Self {
        Self { repo, data_dir }
    }
}
