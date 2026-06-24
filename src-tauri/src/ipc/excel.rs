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
        Blog => "blog",
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
        Waiting => "waiting",
        OnHold => "onHold",
        TimedOut => "timedOut",
        BadCredentials => "badCredentials",
        Challenge => "challenge",
        Blocked => "blocked",
        Error => "error",
    }
}

fn parse_platform(s: &str) -> Option<PlatformId> {
    match s.trim().to_lowercase().as_str() {
        "forum" => Some(PlatformId::Forum),
        "naver" => Some(PlatformId::Naver),
        "blog" => Some(PlatformId::Blog),
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
    // M-4: empty sheet check before required-column check
    if headers.is_empty() {
        return Err("시트가 비어 있습니다".into());
    }
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
    // I-2: track loginIds seen in this import to surface within-file duplicates
    let mut seen = std::collections::HashSet::<String>::new();

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
        // I-2: warn if this loginId was already seen earlier in this import
        if seen.contains(&login) {
            summary
                .errors
                .push(format!("{}행: loginId '{}' 중복 — 덮어씀", n + 2, login));
        }
        seen.insert(login.clone());
        let tags: Vec<String> = i_tags
            .map(|i| {
                cell(r, i)
                    .split(',')
                    .map(|t| t.trim().to_owned())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let mut acct = Account {
            id: login.clone(),
            platform: plat.unwrap(),
            login_id: login.clone(),
            pw,
            status: AccountStatus::New,
            status_msg: None,
            last: "—".into(),
            tags,
        };
        match existing.iter_mut().find(|a| a.login_id == login) {
            Some(a) => {
                acct.id = a.id.clone(); // I-1: preserve existing opaque id
                *a = acct;
            }
            None => existing.push(acct), // 신규 → 추가
        }
        summary.imported += 1;
    }
    Ok((existing, summary))
}

/// 제목이 이미 `is_taken`이면 " (1)", " (2)" … 접미사를 붙여 유일하게 만든다.
/// 술어로 추상화해 호출부가 `HashSet`(O(1) 조회)로 뒷받침할 수 있게 한다.
fn unique_title_with(title: &str, is_taken: impl Fn(&str) -> bool) -> String {
    if !is_taken(title) {
        return title.to_owned();
    }
    let mut n = 1;
    loop {
        let cand = format!("{title} ({n})");
        if !is_taken(&cand) {
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
    // M-4: empty sheet check before required-column check
    if headers.is_empty() {
        return Err("시트가 비어 있습니다".into());
    }
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
    // M-6: hoist now_ms() so it's called once per import, not per row
    let base_ms = crate::util::now_ms();
    // Track taken titles in a set seeded from existing posts and updated as each
    // row is added — avoids rebuilding+rescanning a Vec per row (was O(n²)).
    let mut taken: std::collections::HashSet<String> =
        existing.iter().map(|p| p.title.clone()).collect();

    for (n, r) in rows.enumerate() {
        let title_raw = cell(r, i_title).trim().to_owned();
        let body = cell(r, i_body);
        if title_raw.is_empty() || body.trim().is_empty() {
            summary.skipped += 1;
            summary.errors.push(format!("{}행: title/body 누락", n + 2));
            continue;
        }
        let title = unique_title_with(&title_raw, |t| taken.contains(t));
        taken.insert(title.clone());
        let kind = i_kind
            .map(|i| parse_kind(&cell(r, i)))
            .unwrap_or(ModeValue::Post);
        let id = format!("imp-{}", base_ms + n as i64);
        // I-3: align with writer-modal — non-whitespace char count, 70-char excerpt
        let words = body.chars().filter(|c| !c.is_whitespace()).count() as u32;
        let excerpt: String = body.chars().take(70).collect();
        existing.insert(
            0,
            LibraryPost {
                id,
                title,
                kind,
                updated: "방금 전".into(),
                words,
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
            .write_string(row, 4, a.tags.join(","))
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
        Skip => "건너뜀",
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
        "시각(ms)",
        "제목",
        "플랫폼",
        "대상",
        "코드",
        "계정",
        "상태",
        "메시지",
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
        s2.write_string(rr, 2, &a.text).map_err(|e| e.to_string())?;
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
            status_msg: None,
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
            body: None,
            comment: None,
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
                posted: None,
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
        let taken = ["실적 정리".to_string(), "실적 정리 (1)".to_string()];
        let is_taken = |t: &str| taken.iter().any(|x| x == t);
        assert_eq!(unique_title_with("실적 정리", is_taken), "실적 정리 (2)");
        assert_eq!(unique_title_with("새 글", is_taken), "새 글");
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
            // row 1: valid row that duplicates the existing title
            s.write_string(1, 0, "실적 정리").unwrap();
            s.write_string(1, 1, "본문").unwrap();
            s.write_string(1, 2, "post").unwrap();
            // M-5: row 2: empty title → should be skipped with an error recorded
            s.write_string(2, 1, "body without title").unwrap();
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
        assert_eq!(
            summary.skipped, 1,
            "empty-title row must be counted as skipped"
        );
        assert!(
            !summary.errors.is_empty(),
            "empty-title row must record an error"
        );
        assert!(next.iter().any(|p| p.title == "실적 정리 (1)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Enum-string mappers ───────────────────────────────────────────────

    #[test]
    fn platform_str_all_arms() {
        assert_eq!(platform_str(&PlatformId::Forum), "forum");
        assert_eq!(platform_str(&PlatformId::Naver), "naver");
        assert_eq!(platform_str(&PlatformId::Blog), "blog");
        assert_eq!(platform_str(&PlatformId::Band), "band");
        assert_eq!(platform_str(&PlatformId::Instagram), "instagram");
        assert_eq!(platform_str(&PlatformId::Threads), "threads");
    }

    #[test]
    fn status_str_all_arms() {
        assert_eq!(status_str(&AccountStatus::New), "new");
        assert_eq!(status_str(&AccountStatus::Active), "active");
        assert_eq!(status_str(&AccountStatus::BadCredentials), "badCredentials");
        assert_eq!(status_str(&AccountStatus::Challenge), "challenge");
        assert_eq!(status_str(&AccountStatus::Blocked), "blocked");
        assert_eq!(status_str(&AccountStatus::OnHold), "onHold");
        assert_eq!(status_str(&AccountStatus::TimedOut), "timedOut");
        assert_eq!(status_str(&AccountStatus::Error), "error");
    }

    #[test]
    fn activity_type_str_all_arms() {
        use crate::ipc::activity::ActivityType;
        assert_eq!(activity_type_str(&ActivityType::Success), "성공");
        assert_eq!(activity_type_str(&ActivityType::Error), "실패");
        assert_eq!(activity_type_str(&ActivityType::Info), "정보");
    }

    #[test]
    fn item_status_str_all_arms() {
        use crate::ipc::log_batches::BatchItemStatus;
        assert_eq!(item_status_str(&BatchItemStatus::Success), "성공");
        assert_eq!(item_status_str(&BatchItemStatus::Fail), "실패");
        assert_eq!(item_status_str(&BatchItemStatus::Running), "처리중");
        assert_eq!(item_status_str(&BatchItemStatus::Waiting), "대기");
    }

    // ── Parser helpers ────────────────────────────────────────────────────

    #[test]
    fn parse_platform_all_values() {
        assert_eq!(parse_platform("forum"), Some(PlatformId::Forum));
        assert_eq!(parse_platform("naver"), Some(PlatformId::Naver));
        assert_eq!(parse_platform("blog"), Some(PlatformId::Blog));
        assert_eq!(parse_platform("band"), Some(PlatformId::Band));
        assert_eq!(parse_platform("instagram"), Some(PlatformId::Instagram));
        assert_eq!(parse_platform("threads"), Some(PlatformId::Threads));
    }

    #[test]
    fn parse_platform_case_insensitive() {
        assert_eq!(parse_platform("FORUM"), Some(PlatformId::Forum));
        assert_eq!(parse_platform("Naver"), Some(PlatformId::Naver));
        assert_eq!(parse_platform("  Band  "), Some(PlatformId::Band));
    }

    #[test]
    fn parse_platform_unknown_returns_none() {
        assert_eq!(parse_platform("twitter"), None);
        assert_eq!(parse_platform(""), None);
        assert_eq!(parse_platform("kakao"), None);
    }

    #[test]
    fn parse_kind_all_branches() {
        assert_eq!(parse_kind("comment"), ModeValue::Comment);
        assert_eq!(parse_kind("both"), ModeValue::Both);
        // "post" maps to the default arm which returns ModeValue::Post
        assert_eq!(parse_kind("post"), ModeValue::Post);
        // unknown value → default (Post)
        assert_eq!(parse_kind("unknown"), ModeValue::Post);
        assert_eq!(parse_kind(""), ModeValue::Post);
        // case-insensitive
        assert_eq!(parse_kind("COMMENT"), ModeValue::Comment);
        assert_eq!(parse_kind("  Both  "), ModeValue::Both);
    }

    #[test]
    fn header_index_found_not_found_and_case_insensitive() {
        let headers = vec![
            "LoginId".to_string(),
            "PW".to_string(),
            "Platform".to_string(),
        ];
        // exact case-insensitive match
        assert_eq!(header_index(&headers, "loginId"), Some(0));
        assert_eq!(header_index(&headers, "pw"), Some(1));
        assert_eq!(header_index(&headers, "platform"), Some(2));
        // not found
        assert_eq!(header_index(&headers, "tags"), None);
        assert_eq!(header_index(&headers, ""), None);
        // case variations
        assert_eq!(header_index(&headers, "LOGINID"), Some(0));
    }

    // ── import_accounts error/edge paths ─────────────────────────────────

    #[test]
    fn import_accounts_empty_sheet_error() {
        let dir = std::env::temp_dir().join("pstmacro_imp_acct_empty");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("empty.xlsx");
        {
            let mut wb = Workbook::new();
            // create a worksheet with no rows at all
            let _s = wb.add_worksheet().set_name("계정").unwrap();
            wb.save(&path).unwrap();
        }
        let result = import_accounts(path.to_str().unwrap(), vec![]);
        assert!(result.is_err(), "empty sheet must return Err");
        assert_eq!(result.unwrap_err(), "시트가 비어 있습니다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_accounts_missing_required_column_error() {
        let dir = std::env::temp_dir().join("pstmacro_imp_acct_noheader");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("noheader.xlsx");
        {
            let mut wb = Workbook::new();
            let s = wb.add_worksheet().set_name("계정").unwrap();
            // only "loginId" and "platform" — no "pw" column
            for (c, h) in ["loginId", "platform"].iter().enumerate() {
                s.write_string(0, c as u16, *h).unwrap();
            }
            s.write_string(1, 0, "user1").unwrap();
            s.write_string(1, 1, "forum").unwrap();
            wb.save(&path).unwrap();
        }
        let result = import_accounts(path.to_str().unwrap(), vec![]);
        assert!(result.is_err(), "missing pw column must return Err");
        assert_eq!(
            result.unwrap_err(),
            "필수 컬럼(loginId/pw/platform)이 없습니다"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_accounts_duplicate_preserves_existing_id() {
        let dir = std::env::temp_dir().join("pstmacro_imp_acct_dup_id");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("dup.xlsx");
        {
            let mut wb = Workbook::new();
            let s = wb.add_worksheet().set_name("계정").unwrap();
            for (c, h) in ["loginId", "pw", "platform"].iter().enumerate() {
                s.write_string(0, c as u16, *h).unwrap();
            }
            s.write_string(1, 0, "existing_user").unwrap();
            s.write_string(1, 1, "newpw").unwrap();
            s.write_string(1, 2, "naver").unwrap();
            wb.save(&path).unwrap();
        }
        // Pre-existing account with a different opaque id (uuid-style)
        let existing = vec![Account {
            id: "opaque-uuid-123".into(),
            platform: PlatformId::Forum,
            login_id: "existing_user".into(),
            pw: "oldpw".into(),
            status: AccountStatus::Active,
            status_msg: None,
            last: "—".into(),
            tags: vec![],
        }];
        let (next, summary) = import_accounts(path.to_str().unwrap(), existing).unwrap();
        assert_eq!(summary.imported, 1);
        let updated = next.iter().find(|a| a.login_id == "existing_user").unwrap();
        // The existing opaque id must be preserved, NOT replaced by loginId
        assert_eq!(
            updated.id, "opaque-uuid-123",
            "existing account id must be preserved on update"
        );
        // The new password should have been applied
        assert_eq!(updated.pw, "newpw");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_accounts_within_file_duplicate_adds_error() {
        let dir = std::env::temp_dir().join("pstmacro_imp_acct_infile_dup");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("infiledup.xlsx");
        {
            let mut wb = Workbook::new();
            let s = wb.add_worksheet().set_name("계정").unwrap();
            for (c, h) in ["loginId", "pw", "platform"].iter().enumerate() {
                s.write_string(0, c as u16, *h).unwrap();
            }
            // Same loginId appears twice in the import file
            s.write_string(1, 0, "dup_user").unwrap();
            s.write_string(1, 1, "pw1").unwrap();
            s.write_string(1, 2, "band").unwrap();
            s.write_string(2, 0, "dup_user").unwrap();
            s.write_string(2, 1, "pw2").unwrap();
            s.write_string(2, 2, "band").unwrap();
            wb.save(&path).unwrap();
        }
        let (_, summary) = import_accounts(path.to_str().unwrap(), vec![]).unwrap();
        // Both rows count as imported (the second overwrites the first)
        assert_eq!(summary.imported, 2);
        // The within-file duplicate must push an error message
        assert!(
            !summary.errors.is_empty(),
            "within-file duplicate must push an error"
        );
        assert!(
            summary.errors.iter().any(|e| e.contains("중복")),
            "error must mention '중복'"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── import_posts error/edge paths ─────────────────────────────────────

    #[test]
    fn import_posts_empty_sheet_error() {
        let dir = std::env::temp_dir().join("pstmacro_imp_post_empty");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("empty.xlsx");
        {
            let mut wb = Workbook::new();
            let _s = wb.add_worksheet().set_name("게시글").unwrap();
            wb.save(&path).unwrap();
        }
        let result = import_posts(path.to_str().unwrap(), vec![]);
        assert!(result.is_err(), "empty sheet must return Err");
        assert_eq!(result.unwrap_err(), "시트가 비어 있습니다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_posts_missing_required_column_error() {
        let dir = std::env::temp_dir().join("pstmacro_imp_post_noheader");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("noheader.xlsx");
        {
            let mut wb = Workbook::new();
            let s = wb.add_worksheet().set_name("게시글").unwrap();
            // only "title" — no "body" column
            s.write_string(0, 0, "title").unwrap();
            s.write_string(1, 0, "some title").unwrap();
            wb.save(&path).unwrap();
        }
        let result = import_posts(path.to_str().unwrap(), vec![]);
        assert!(result.is_err(), "missing body column must return Err");
        assert_eq!(result.unwrap_err(), "필수 컬럼(title/body)이 없습니다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_posts_skips_empty_title_or_body() {
        let dir = std::env::temp_dir().join("pstmacro_imp_post_skip");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("skip.xlsx");
        {
            let mut wb = Workbook::new();
            let s = wb.add_worksheet().set_name("게시글").unwrap();
            for (c, h) in ["title", "body"].iter().enumerate() {
                s.write_string(0, c as u16, *h).unwrap();
            }
            // row 1: valid
            s.write_string(1, 0, "정상 제목").unwrap();
            s.write_string(1, 1, "정상 본문").unwrap();
            // row 2: empty title → skip
            s.write_string(2, 1, "본문만 있음").unwrap();
            // row 3: empty body → skip
            s.write_string(3, 0, "제목만 있음").unwrap();
            wb.save(&path).unwrap();
        }
        let (next, summary) = import_posts(path.to_str().unwrap(), vec![]).unwrap();
        assert_eq!(summary.imported, 1, "only one valid row");
        assert_eq!(summary.skipped, 2, "two rows must be skipped");
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].title, "정상 제목");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_posts_kind_column_parsed_correctly() {
        let dir = std::env::temp_dir().join("pstmacro_imp_post_kind");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("kind.xlsx");
        {
            let mut wb = Workbook::new();
            let s = wb.add_worksheet().set_name("게시글").unwrap();
            for (c, h) in ["title", "body", "kind"].iter().enumerate() {
                s.write_string(0, c as u16, *h).unwrap();
            }
            s.write_string(1, 0, "코멘트 글").unwrap();
            s.write_string(1, 1, "본문 코멘트").unwrap();
            s.write_string(1, 2, "comment").unwrap();
            s.write_string(2, 0, "둘다 글").unwrap();
            s.write_string(2, 1, "본문 둘다").unwrap();
            s.write_string(2, 2, "both").unwrap();
            wb.save(&path).unwrap();
        }
        let (next, summary) = import_posts(path.to_str().unwrap(), vec![]).unwrap();
        assert_eq!(summary.imported, 2);
        // inserted in reverse (insert at 0), so next[0] is row 2 (both), next[1] is row 1 (comment)
        let kinds: Vec<&ModeValue> = next.iter().map(|p| &p.kind).collect();
        assert!(
            kinds.contains(&&ModeValue::Comment),
            "comment kind must be parsed"
        );
        assert!(
            kinds.contains(&&ModeValue::Both),
            "both kind must be parsed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
