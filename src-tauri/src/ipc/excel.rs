//! 엑셀(.xlsx) 입출력 — Rust에서 워크북 생성(rust_xlsxwriter)/파싱(calamine).
use calamine::{open_workbook, Data, Reader, Xlsx};
use rust_xlsxwriter::Workbook;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ipc::accounts::{Account, AccountStatus, PlatformId};
use crate::ipc::posts::{LibraryPost, ModeValue, PostStatus};

/// 엑셀 가져오기 결과 요약.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub imported: u32,
    pub skipped: u32,
    pub errors: Vec<String>,
}

fn platform_str(p: &PlatformId) -> &'static str {
    use PlatformId::*;
    match p {
        Forum => "forum",
        Naver => "naver",
        Band => "band",
        Instagram => "instagram",
        Threads => "threads",
    }
}

fn status_str(s: &AccountStatus) -> &'static str {
    use AccountStatus::*;
    match s {
        New => "new",
        Active => "active",
        Error => "error",
    }
}

fn parse_platform(s: &str) -> Option<PlatformId> {
    match s.trim().to_lowercase().as_str() {
        "forum" => Some(PlatformId::Forum),
        "naver" => Some(PlatformId::Naver),
        "band" => Some(PlatformId::Band),
        "instagram" => Some(PlatformId::Instagram),
        "threads" => Some(PlatformId::Threads),
        _ => None,
    }
}

fn header_index(headers: &[String], name: &str) -> Option<usize> {
    headers
        .iter()
        .position(|h| h.trim().eq_ignore_ascii_case(name))
}

fn parse_kind(s: &str) -> ModeValue {
    match s.trim().to_lowercase().as_str() {
        "comment" => ModeValue::Comment,
        "both" => ModeValue::Both,
        _ => ModeValue::Post,
    }
}

/// 첫 시트를 읽어 (loginId, pw, platform[, tags]) 행을 계정으로 변환·병합.
/// 중복 loginId는 기존 행 업데이트. 반환: (병합된 목록, 요약).
pub fn import_accounts(
    path: &str,
    mut existing: Vec<Account>,
) -> Result<(Vec<Account>, ImportSummary), String> {
    let mut wb: Xlsx<_> = open_workbook(path).map_err(|e: calamine::XlsxError| e.to_string())?;
    let range = wb
        .worksheet_range_at(0)
        .ok_or("시트를 찾을 수 없습니다")?
        .map_err(|e| e.to_string())?;
    let mut rows = range.rows();
    let headers: Vec<String> = rows
        .next()
        .map(|r| r.iter().map(|c| c.to_string()).collect())
        .unwrap_or_default();
    let (Some(i_login), Some(i_pw), Some(i_plat)) = (
        header_index(&headers, "loginId"),
        header_index(&headers, "pw"),
        header_index(&headers, "platform"),
    ) else {
        return Err("필수 컬럼(loginId/pw/platform)이 없습니다".into());
    };
    let i_tags = header_index(&headers, "tags");

    let mut summary = ImportSummary {
        imported: 0,
        skipped: 0,
        errors: vec![],
    };
    let cell = |r: &[Data], i: usize| r.get(i).map(|c| c.to_string()).unwrap_or_default();

    for (n, r) in rows.enumerate() {
        let login = cell(r, i_login).trim().to_owned();
        let pw = cell(r, i_pw).trim().to_owned();
        let plat = parse_platform(&cell(r, i_plat));
        if login.is_empty() || pw.is_empty() || plat.is_none() {
            summary.skipped += 1;
            summary
                .errors
                .push(format!("{}행: loginId/pw/platform 누락 또는 오류", n + 2));
            continue;
        }
        let tags: Vec<String> = i_tags
            .map(|i| {
                cell(r, i)
                    .split(',')
                    .map(|t| t.trim().to_owned())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let acct = Account {
            id: login.clone(),
            platform: plat.unwrap(),
            login_id: login.clone(),
            pw,
            status: AccountStatus::New,
            last: "—".into(),
            tags,
        };
        match existing.iter_mut().find(|a| a.login_id == login) {
            Some(a) => *a = acct,   // 중복 → 업데이트
            None => existing.push(acct), // 신규 → 추가
        }
        summary.imported += 1;
    }
    Ok((existing, summary))
}

/// `taken`에 없는 제목이면 그대로, 있으면 " (1)", " (2)" … 접미사.
pub fn unique_title(title: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == title) {
        return title.to_owned();
    }
    let mut n = 1;
    loop {
        let cand = format!("{title} ({n})");
        if !taken.iter().any(|t| t == &cand) {
            return cand;
        }
        n += 1;
    }
}

/// 첫 시트를 읽어 (title, body[, kind]) 행을 게시글로 변환·추가.
/// 제목 중복 시 "(1)", "(2)" 접미사를 붙인다. 반환: (병합된 목록, 요약).
pub fn import_posts(
    path: &str,
    mut existing: Vec<LibraryPost>,
) -> Result<(Vec<LibraryPost>, ImportSummary), String> {
    let mut wb: Xlsx<_> = open_workbook(path).map_err(|e: calamine::XlsxError| e.to_string())?;
    let range = wb
        .worksheet_range_at(0)
        .ok_or("시트를 찾을 수 없습니다")?
        .map_err(|e| e.to_string())?;
    let mut rows = range.rows();
    let headers: Vec<String> = rows
        .next()
        .map(|r| r.iter().map(|c| c.to_string()).collect())
        .unwrap_or_default();
    let (Some(i_title), Some(i_body)) = (
        header_index(&headers, "title"),
        header_index(&headers, "body"),
    ) else {
        return Err("필수 컬럼(title/body)이 없습니다".into());
    };
    let i_kind = header_index(&headers, "kind");

    let mut summary = ImportSummary {
        imported: 0,
        skipped: 0,
        errors: vec![],
    };
    let cell = |r: &[Data], i: usize| r.get(i).map(|c| c.to_string()).unwrap_or_default();

    for (n, r) in rows.enumerate() {
        let title_raw = cell(r, i_title).trim().to_owned();
        let body = cell(r, i_body);
        if title_raw.is_empty() || body.trim().is_empty() {
            summary.skipped += 1;
            summary
                .errors
                .push(format!("{}행: title/body 누락", n + 2));
            continue;
        }
        let taken: Vec<String> = existing.iter().map(|p| p.title.clone()).collect();
        let title = unique_title(&title_raw, &taken);
        let kind = i_kind
            .map(|i| parse_kind(&cell(r, i)))
            .unwrap_or(ModeValue::Post);
        let id = format!("imp-{}", crate::util::now_ms() + n as i64);
        let excerpt: String = body.chars().take(60).collect();
        existing.insert(
            0,
            LibraryPost {
                id,
                title,
                kind,
                updated: "방금 전".into(),
                words: body.chars().count() as u32,
                status: PostStatus::Draft,
                excerpt,
                body: Some(body),
                comments: None,
                comment_target: None,
                comment_url: None,
                comment_count: None,
            },
        );
        summary.imported += 1;
    }
    Ok((existing, summary))
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

use crate::ipc::activity::{ActivityItem, ActivityType};
use crate::ipc::log_batches::{BatchItemStatus, LogBatch};

fn activity_type_str(t: &ActivityType) -> &'static str {
    use ActivityType::*;
    match t {
        Success => "성공",
        Error => "실패",
        Info => "정보",
    }
}

fn item_status_str(s: &BatchItemStatus) -> &'static str {
    use BatchItemStatus::*;
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
        use crate::ipc::log_batches::{BatchItem, LogBatch};
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

    // D1 — ImportSummary camelCase serialization
    #[test]
    fn summary_camelcase() {
        let s = ImportSummary {
            imported: 3,
            skipped: 1,
            errors: vec!["bad row".into()],
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"imported\":3"));
        assert!(j.contains("\"skipped\":1"));
    }

    // D2 — import_accounts round-trip: write fixture → import → validate
    #[test]
    fn import_accounts_validates_and_merges() {
        let dir = std::env::temp_dir().join("pstmacro_imp_acct");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("in.xlsx");
        // 헤더 + 2 valid + 1 invalid(빈 pw)
        {
            let mut wb = Workbook::new();
            let s = wb.add_worksheet().set_name("계정").unwrap();
            for (c, h) in ["loginId", "pw", "platform", "tags"].iter().enumerate() {
                s.write_string(0, c as u16, *h).unwrap();
            }
            s.write_string(1, 0, "new_user").unwrap();
            s.write_string(1, 1, "pw1").unwrap();
            s.write_string(1, 2, "forum").unwrap();
            s.write_string(1, 3, "반도체,대형주").unwrap();
            s.write_string(2, 0, "no_pw").unwrap();
            s.write_string(2, 2, "naver").unwrap(); // pw 없음 → skip
            wb.save(&path).unwrap();
        }
        let existing = vec![];
        let (next, summary) = import_accounts(path.to_str().unwrap(), existing).unwrap();
        assert_eq!(summary.imported, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].login_id, "new_user");
        assert_eq!(
            next[0].tags,
            vec!["반도체".to_string(), "대형주".to_string()]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // D3 — unique_title and import_posts de-duplication
    #[test]
    fn unique_title_appends_suffix() {
        let taken = vec!["실적 정리".to_string(), "실적 정리 (1)".to_string()];
        assert_eq!(unique_title("실적 정리", &taken), "실적 정리 (2)");
        assert_eq!(unique_title("새 글", &taken), "새 글");
    }

    #[test]
    fn import_posts_dedupes_titles() {
        let dir = std::env::temp_dir().join("pstmacro_imp_post");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("p.xlsx");
        {
            let mut wb = Workbook::new();
            let s = wb.add_worksheet().set_name("게시글").unwrap();
            for (c, h) in ["title", "body", "kind"].iter().enumerate() {
                s.write_string(0, c as u16, *h).unwrap();
            }
            s.write_string(1, 0, "실적 정리").unwrap();
            s.write_string(1, 1, "본문").unwrap();
            s.write_string(1, 2, "post").unwrap();
            wb.save(&path).unwrap();
        }
        use crate::ipc::posts::PostStatus;
        let existing = vec![LibraryPost {
            id: "l1".into(),
            title: "실적 정리".into(),
            kind: ModeValue::Post,
            updated: "—".into(),
            words: 1,
            status: PostStatus::Draft,
            excerpt: "x".into(),
            body: None,
            comments: None,
            comment_target: None,
            comment_url: None,
            comment_count: None,
        }];
        let (next, summary) = import_posts(path.to_str().unwrap(), existing).unwrap();
        assert_eq!(summary.imported, 1);
        assert!(next.iter().any(|p| p.title == "실적 정리 (1)"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
