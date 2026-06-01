//! Band destinations (밴드) domain — JSON-file-backed, served over Tauri IPC.
//! The user's connected bands, used as publish targets. Read-only for the UI;
//! the only command is `list_bands`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::store::JsonStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Band {
    pub name: String,
}

fn band(name: &str) -> Band {
    Band { name: name.into() }
}

pub fn seed() -> Vec<Band> {
    vec![
        band("가치투자모임 BAND"),
        band("단타클럽 BAND"),
        band("주식스터디 BAND"),
    ]
}

#[tauri::command]
pub fn list_bands(store: tauri::State<'_, JsonStore<Band>>) -> Vec<Band> {
    store.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_has_three_bands() {
        assert_eq!(seed().len(), 3);
    }

    #[test]
    fn seed_roundtrips_through_json() {
        let bands = seed();
        let back: Vec<Band> =
            serde_json::from_str(&serde_json::to_string(&bands).unwrap()).unwrap();
        assert_eq!(bands, back);
    }
}
