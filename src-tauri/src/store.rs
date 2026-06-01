//! Generic JSON-file-backed store shared by every domain.
//!
//! Each domain manages a `JsonStore<T>` (e.g. `JsonStore<Account>`); Tauri keys
//! managed state by type, so the distinct `T`s don't collide. The store keeps an
//! in-memory `Vec<T>` guarded by a `Mutex` and persists the whole list to a
//! pretty-printed JSON file on every mutation. No database — just JSON on disk.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::de::DeserializeOwned;
use serde::Serialize;

pub struct JsonStore<T> {
    path: PathBuf,
    items: Mutex<Vec<T>>,
}

impl<T> JsonStore<T>
where
    T: Clone + Serialize + DeserializeOwned,
{
    /// Load the list from `path`; if the file is missing or unreadable, persist
    /// `seed` and start from it.
    pub fn load_or_seed(path: PathBuf, seed: Vec<T>) -> Self {
        let items = read_json(&path).unwrap_or_else(|| {
            let _ = write_json(&path, &seed);
            seed
        });
        JsonStore {
            path,
            items: Mutex::new(items),
        }
    }

    pub fn snapshot(&self) -> Vec<T> {
        self.items.lock().expect("store mutex poisoned").clone()
    }

    /// Apply `f` to a clone of the current list, persist the result to disk, and
    /// store it as the new in-memory state. Returns the updated list.
    pub fn mutate<F>(&self, f: F) -> Vec<T>
    where
        F: FnOnce(Vec<T>) -> Vec<T>,
    {
        let mut guard = self.items.lock().expect("store mutex poisoned");
        let next = f(guard.clone());
        let _ = write_json(&self.path, &next);
        *guard = next.clone();
        next
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<Vec<T>> {
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn write_json<T: Serialize>(path: &Path, items: &[T]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(items).unwrap_or_else(|_| "[]".into());
    std::fs::write(path, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join("pstmacro_store_test")
            .join(name)
    }

    #[test]
    fn seeds_when_file_absent_then_persists() {
        let path = tmp("seed.json");
        let _ = std::fs::remove_file(&path);
        let store = JsonStore::load_or_seed(path.clone(), vec![1u32, 2, 3]);
        assert_eq!(store.snapshot(), vec![1, 2, 3]);
        // file now exists and round-trips
        let reloaded = JsonStore::<u32>::load_or_seed(path.clone(), vec![9]);
        assert_eq!(reloaded.snapshot(), vec![1, 2, 3]); // seed ignored, file wins
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mutate_persists_to_disk() {
        let path = tmp("mutate.json");
        let _ = std::fs::remove_file(&path);
        let store = JsonStore::load_or_seed(path.clone(), vec![1u32]);
        let next = store.mutate(|mut v| {
            v.push(2);
            v
        });
        assert_eq!(next, vec![1, 2]);
        // a fresh store reads the persisted state
        let reloaded = JsonStore::<u32>::load_or_seed(path.clone(), vec![]);
        assert_eq!(reloaded.snapshot(), vec![1, 2]);
        let _ = std::fs::remove_file(&path);
    }
}
