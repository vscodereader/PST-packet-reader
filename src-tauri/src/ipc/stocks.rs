//! Stocks (종목토론방 크롤링 결과) domain — JSON-file-backed, served over Tauri
//! IPC. Read-only for the UI (the stock-crawl-modal lists them and the writer
//! picks targets by `code`), so the only command is `list_stocks`; the generic
//! [`JsonStore`] still persists the seed so the data lives outside the frontend.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::store::JsonStore;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Stock {
    pub code: String,
    pub name: String,
    pub market: String,
    pub posts: String,
    pub price: String,
    pub chg: f64,
}

fn stock(code: &str, name: &str, market: &str, posts: &str, price: &str, chg: f64) -> Stock {
    Stock {
        code: code.into(),
        name: name.into(),
        market: market.into(),
        posts: posts.into(),
        price: price.into(),
        chg,
    }
}

pub fn seed() -> Vec<Stock> {
    vec![
        stock("005930", "삼성전자", "KOSPI", "12,480", "78,400", 1.2),
        stock("000660", "SK하이닉스", "KOSPI", "8,210", "189,500", 2.8),
        stock("035720", "카카오", "KOSPI", "9,640", "41,250", -0.6),
        stock("035420", "NAVER", "KOSPI", "6,330", "172,800", 0.4),
        stock("086520", "에코프로", "KOSDAQ", "15,720", "98,700", -3.1),
        stock(
            "247540",
            "에코프로비엠",
            "KOSDAQ",
            "11,090",
            "172,300",
            -2.4,
        ),
        stock("373220", "LG에너지솔루션", "KOSPI", "7,450", "367,000", 1.7),
        stock("005490", "POSCO홀딩스", "KOSPI", "10,210", "412,500", 3.3),
        stock(
            "207940",
            "삼성바이오로직스",
            "KOSPI",
            "3,180",
            "789,000",
            0.9,
        ),
        stock("068270", "셀트리온", "KOSPI", "5,940", "182,400", -1.1),
        stock("323410", "카카오뱅크", "KOSPI", "4,720", "23,150", 0.2),
        stock("042700", "한미반도체", "KOSPI", "6,880", "118,900", 4.6),
    ]
}

#[tauri::command]
pub fn list_stocks(store: tauri::State<'_, JsonStore<Stock>>) -> Vec<Stock> {
    store.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_has_all_markets_and_codes() {
        let stocks = seed();
        assert_eq!(stocks.len(), 12);
        assert!(stocks
            .iter()
            .any(|s| s.code == "005930" && s.name == "삼성전자"));
        assert!(stocks.iter().any(|s| s.market == "KOSDAQ"));
    }

    #[test]
    fn seed_roundtrips_through_json_with_camelcase() {
        let stocks = seed();
        let json = serde_json::to_string(&stocks).unwrap();
        // `chg` stays a bare number, no field renaming surprises.
        assert!(json.contains("\"chg\":1.2"));
        let back: Vec<Stock> = serde_json::from_str(&json).unwrap();
        assert_eq!(stocks, back);
    }
}
