use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimePaths {
    pub root: PathBuf,
    pub accounts_dir: PathBuf,
    pub cookies_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub accounts_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub password: String,
    #[serde(default)]
    pub label: String,
}
