//! 엑셀(.xlsx) 입출력 — Rust에서 워크북 생성(rust_xlsxwriter)/파싱(calamine).
use rust_xlsxwriter::Workbook;

use crate::ipc::accounts::Account;

fn platform_str(p: &crate::ipc::accounts::PlatformId) -> &'static str {
    use crate::ipc::accounts::PlatformId::*;
    match p {
        Forum => "forum",
        Naver => "naver",
        Band => "band",
        Instagram => "instagram",
        Threads => "threads",
    }
}

fn status_str(s: &crate::ipc::accounts::AccountStatus) -> &'static str {
    use crate::ipc::accounts::AccountStatus::*;
    match s {
        New => "new",
        Active => "active",
        Error => "error",
    }
}

/// 계정 목록을 "계정" 시트로 기록. pw 포함(백업/재가져오기 대칭).
pub fn write_accounts_xlsx(path: &str, accounts: &[Account]) -> Result<(), String> {
    let mut wb = Workbook::new();
    let sheet = wb
        .add_worksheet()
        .set_name("계정")
        .map_err(|e| e.to_string())?;
    let headers = ["loginId", "pw", "platform", "status", "tags", "last"];
    for (c, h) in headers.iter().enumerate() {
        sheet
            .write_string(0, c as u16, *h)
            .map_err(|e| e.to_string())?;
    }
    for (r, a) in accounts.iter().enumerate() {
        let row = (r + 1) as u32;
        sheet
            .write_string(row, 0, &a.login_id)
            .map_err(|e| e.to_string())?;
        sheet
            .write_string(row, 1, &a.pw)
            .map_err(|e| e.to_string())?;
        sheet
            .write_string(row, 2, platform_str(&a.platform))
            .map_err(|e| e.to_string())?;
        sheet
            .write_string(row, 3, status_str(&a.status))
            .map_err(|e| e.to_string())?;
        sheet
            .write_string(row, 4, &a.tags.join(","))
            .map_err(|e| e.to_string())?;
        sheet
            .write_string(row, 5, &a.last)
            .map_err(|e| e.to_string())?;
    }
    wb.save(path).map_err(|e| e.to_string())
}

use crate::ipc::activity::ActivityItem;
use crate::ipc::log_batches::LogBatch;

fn activity_type_str(t: &crate::ipc::activity::ActivityType) -> &'static str {
    use crate::ipc::activity::ActivityType::*;
    match t {
        Success => "성공",
        Error => "실패",
        Info => "정보",
    }
}

fn item_status_str(s: &crate::ipc::log_batches::BatchItemStatus) -> &'static str {
    use crate::ipc::log_batches::BatchItemStatus::*;
    match s {
        Success => "성공",
        Fail => "실패",
        Running => "처리중",
        Waiting => "대기",
    }
}

/// 알림 피드를 두 시트("게시 배치", "시스템 활동")로 기록.
pub fn write_activity_xlsx(
    path: &str,
    batches: &[LogBatch],
    activity: &[ActivityItem],
) -> Result<(), String> {
    let mut wb = Workbook::new();

    // 시트 ① 게시 배치 (flatten)
    let s1 = wb
        .add_worksheet()
        .set_name("게시 배치")
        .map_err(|e| e.to_string())?;
    let h1 = [
        "시각(ms)", "제목", "플랫폼", "대상", "코드", "계정", "상태", "메시지",
    ];
    for (c, h) in h1.iter().enumerate() {
        s1.write_string(0, c as u16, *h)
            .map_err(|e| e.to_string())?;
    }
    let mut row = 1u32;
    for b in batches {
        for it in &b.items {
            s1.write_number(row, 0, b.at as f64)
                .map_err(|e| e.to_string())?;
            s1.write_string(row, 1, &b.title)
                .map_err(|e| e.to_string())?;
            s1.write_string(row, 2, platform_str(&it.platform))
                .map_err(|e| e.to_string())?;
            s1.write_string(row, 3, &it.target)
                .map_err(|e| e.to_string())?;
            s1.write_string(row, 4, it.code.as_deref().unwrap_or(""))
                .map_err(|e| e.to_string())?;
            s1.write_string(row, 5, &it.login_id)
                .map_err(|e| e.to_string())?;
            s1.write_string(row, 6, item_status_str(&it.status))
                .map_err(|e| e.to_string())?;
            s1.write_string(row, 7, &it.msg)
                .map_err(|e| e.to_string())?;
            row += 1;
        }
    }

    // 시트 ② 시스템 활동
    let s2 = wb
        .add_worksheet()
        .set_name("시스템 활동")
        .map_err(|e| e.to_string())?;
    let h2 = ["시각(ms)", "유형", "내용"];
    for (c, h) in h2.iter().enumerate() {
        s2.write_string(0, c as u16, *h)
            .map_err(|e| e.to_string())?;
    }
    for (r, a) in activity.iter().enumerate() {
        let rr = (r + 1) as u32;
        s2.write_number(rr, 0, a.at as f64)
            .map_err(|e| e.to_string())?;
        s2.write_string(rr, 1, activity_type_str(&a.r#type))
            .map_err(|e| e.to_string())?;
        s2.write_string(rr, 2, &a.text)
            .map_err(|e| e.to_string())?;
    }

    wb.save(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::accounts::{Account, AccountStatus, PlatformId};

    fn acct(login: &str) -> Account {
        Account {
            id: login.into(),
            platform: PlatformId::Forum,
            login_id: login.into(),
            pw: "pw123".into(),
            status: AccountStatus::Active,
            last: "—".into(),
            tags: vec!["반도체".into(), "대형주".into()],
        }
    }

    #[test]
    fn accounts_roundtrip_through_xlsx() {
        let dir = std::env::temp_dir().join("pstmacro_xlsx_acct");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("acc.xlsx");
        write_accounts_xlsx(path.to_str().unwrap(), &[acct("invest_king7")]).unwrap();

        use calamine::{open_workbook, Reader, Xlsx};
        let mut wb: Xlsx<_> = open_workbook(&path).unwrap();
        let range = wb.worksheet_range("계정").unwrap();
        let rows: Vec<_> = range.rows().collect();
        // header row
        assert_eq!(rows[0][0].to_string(), "loginId");
        assert_eq!(rows[0][1].to_string(), "pw");
        assert_eq!(rows[0][2].to_string(), "platform");
        assert_eq!(rows[0][3].to_string(), "status");
        assert_eq!(rows[0][4].to_string(), "tags");
        assert_eq!(rows[0][5].to_string(), "last");
        // data row
        assert_eq!(rows[1][0].to_string(), "invest_king7");
        assert_eq!(rows[1][1].to_string(), "pw123"); // pw 포함
        assert_eq!(rows[1][2].to_string(), "forum"); // PlatformId::Forum → "forum"
        assert_eq!(rows[1][3].to_string(), "active"); // AccountStatus::Active → "active"
        assert!(rows[1][4].to_string().contains("반도체")); // tags
        assert_eq!(rows[1][5].to_string(), "—"); // last
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn activity_log_roundtrip_two_sheets() {
        use crate::ipc::accounts::PlatformId;
        use crate::ipc::activity::{ActivityItem, ActivityType};
        use crate::ipc::log_batches::{BatchItem, BatchItemStatus, LogBatch};
        use crate::ipc::posts::ModeValue;
        let dir = std::env::temp_dir().join("pstmacro_xlsx_act");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("act.xlsx");
        let batch = LogBatch {
            id: "lb1".into(),
            title: "실적 정리".into(),
            kind: ModeValue::Post,
            at: 1_700_000_000_000,
            state: None,
            items: vec![BatchItem {
                platform: PlatformId::Forum,
                target: "삼성전자".into(),
                code: Some("005930".into()),
                board: None,
                login_id: "invest_king7".into(),
                status: BatchItemStatus::Success,
                msg: "게시 완료".into(),
                trace: None,
            }],
        };
        let act = ActivityItem {
            id: "ac1".into(),
            r#type: ActivityType::Info,
            text: "종목 12개 크롤링".into(),
            at: 1_700_000_000_000,
        };
        write_activity_xlsx(path.to_str().unwrap(), &[batch], &[act]).unwrap();

        use calamine::{open_workbook, Reader, Xlsx};
        let mut wb: Xlsx<_> = open_workbook(&path).unwrap();

        // 게시 배치 시트 셀 값 검증
        let batch_sheet = wb.worksheet_range("게시 배치").unwrap();
        let brows: Vec<_> = batch_sheet.rows().collect();
        assert_eq!(brows[0][1].to_string(), "제목"); // header col 1
        assert_eq!(brows[0][3].to_string(), "대상"); // header col 3
        assert_eq!(brows[1][1].to_string(), "실적 정리"); // title
        assert_eq!(brows[1][2].to_string(), "forum"); // platform (Forum)
        assert_eq!(brows[1][3].to_string(), "삼성전자"); // 대상
        assert_eq!(brows[1][4].to_string(), "005930"); // 코드
        assert_eq!(brows[1][5].to_string(), "invest_king7"); // 계정
        assert_eq!(brows[1][6].to_string(), "성공"); // 상태 (item_status_str Success)
        assert_eq!(brows[1][7].to_string(), "게시 완료"); // 메시지
        // timestamp column is written via write_number → calamine reads it back as Data::Float
        assert!(
            matches!(brows[1][0], calamine::Data::Float(_)),
            "expected Data::Float for timestamp, got {:?}",
            brows[1][0]
        );

        // 시스템 활동 시트 검증
        let sys = wb.worksheet_range("시스템 활동").unwrap();
        let rows: Vec<_> = sys.rows().collect();
        assert_eq!(rows[1][2].to_string(), "종목 12개 크롤링");
        // system activity timestamp also written via write_number → Data::Float
        assert!(
            matches!(rows[1][0], calamine::Data::Float(_)),
            "expected Data::Float for activity timestamp, got {:?}",
            rows[1][0]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
