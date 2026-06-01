use std::collections::BTreeMap;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Emitter;

use crate::naver_automation::{
    run_naver_discussion_macro, run_naver_post_with_comment_macro, AutomationReport,
    AutomationTarget, DiscussionStock, NaverDiscussionRequest, NaverPostWithCommentRequest,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
// CSV 템플릿에서 읽은 제목, 내용, 댓글내용 목록을 담는 구조체입니다.
pub struct TemplateColumns {
    pub titles: Vec<String>,
    pub bodies: Vec<String>,
    pub comments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// UI에서 검색하거나 선택할 네이버 종목 후보를 담는 구조체입니다.
pub struct StockCandidate {
    pub name: String,
    pub code: String,
    pub link: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
// 여러 문구 중 어떤 방식으로 값을 선택할지 나타내는 enum입니다.
pub enum PickMode {
    Random,
    Sequential,
    Single,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// UI에서 저장 후 실행 버튼을 눌렀을 때 Rust로 전달되는 batch 설정입니다.
#[serde(rename_all = "camelCase")]
pub struct DiscussionBatchRequest {
    pub host: String,
    pub port: u16,
    pub stocks: Vec<DiscussionStock>,
    pub run_post: bool,
    pub run_comment: bool,
    pub titles: Vec<String>,
    pub bodies: Vec<String>,
    pub comments: Vec<String>,
    pub title_mode: PickMode,
    pub body_mode: PickMode,
    pub comment_mode: PickMode,
    pub count: usize,
    // 로그인 자동화로 저장된 계정 ID(선택). 지정되면 해당 계정 쿠키를 Chrome에 주입합니다.
    #[serde(default)]
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// batch 실행 결과를 UI에 보여주기 위한 구조체입니다.
pub struct DiscussionBatchReport {
    pub completed: usize,
    pub reports: Vec<AutomationReport>,
}

// CSV 템플릿 문자열에서 제목, 내용, 댓글내용 컬럼을 파싱하는 함수입니다.
pub fn parse_discussion_template_csv(csv_text: String) -> Result<TemplateColumns, String> {
    let rows = parse_csv_rows(&csv_text)?;
    let Some(header) = rows.first() else {
        return Err("CSV 파일이 비어 있습니다.".to_owned());
    };

    if header.len() < 3 {
        return Err("CSV 첫 행에는 제목, 내용, 댓글내용 3개 열이 필요합니다.".to_owned());
    }

    let title_index = find_header_index(header, &["제목", "title"]).unwrap_or(0);
    let body_index = find_header_index(header, &["내용", "body", "content"]).unwrap_or(1);
    let comment_index =
        find_header_index(header, &["댓글내용", "댓글 내용", "comment"]).unwrap_or(2);

    let mut titles = Vec::new();
    let mut bodies = Vec::new();
    let mut comments = Vec::new();

    for row in rows.into_iter().skip(1) {
        push_non_empty(&mut titles, row.get(title_index));
        push_non_empty(&mut bodies, row.get(body_index));
        push_non_empty(&mut comments, row.get(comment_index));
    }

    if titles.is_empty() && bodies.is_empty() && comments.is_empty() {
        return Err("CSV에서 가져올 제목, 내용, 댓글내용이 없습니다.".to_owned());
    }

    Ok(TemplateColumns {
        titles,
        bodies,
        comments,
    })
}

// 네이버 증권 공개 API에서 종목 후보를 가져오고 query로 필터링하는 함수입니다.
pub fn search_naver_stocks(query: Option<String>) -> Result<Vec<StockCandidate>, String> {
    let query = query.unwrap_or_default().trim().to_owned();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| format!("종목 검색 HTTP 클라이언트 생성 실패: {error}"))?;
    let paths = [
        "/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=quantTop&startIdx=0&pageSize=80",
        "/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=up&startIdx=0&pageSize=80",
        "/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=down&startIdx=0&pageSize=80",
        "/api/community/discussion/rankings?nationType=KOR&page=1&size=80&postType=HOT",
    ];
    let mut candidates = Vec::new();

    for path in paths {
        let url = format!("https://stock.naver.com{path}");
        let Ok(response) = client
            .get(&url)
            .header("accept", "application/json, text/plain, */*")
            .header(
                "referer",
                "https://stock.naver.com/market/stock/kr/stocklist/priceTop",
            )
            .header("user-agent", "Mozilla/5.0")
            .send()
        else {
            continue;
        };
        let Ok(text) = response.text() else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };

        collect_stock_candidates(&value, &mut candidates);
    }

    if candidates.is_empty() {
        candidates.extend(fallback_stocks());
    }

    let mut unique = dedupe_stocks(candidates);

    if !query.is_empty() {
        let needle = query.to_lowercase();
        unique.retain(|stock| {
            stock.name.to_lowercase().contains(&needle) || stock.code.contains(&needle)
        });
    }

    unique.truncate(80);
    Ok(unique)
}

// UI 설정에 따라 글쓰기/댓글쓰기를 여러 번 실행하는 함수입니다.
pub fn run_discussion_batch(
    request: DiscussionBatchRequest,
    app: tauri::AppHandle,
) -> Result<DiscussionBatchReport, String> {
    validate_batch_request(&request)?;

    let mut reports = Vec::new();
    let total_actions = request.count * usize::from(request.run_post)
        + request.count * usize::from(request.run_comment);
    let mut completed_actions = 0;

    for index in 0..request.count {
        let stock = request.stocks[index % request.stocks.len()].clone();

        if request.run_post && request.run_comment {
            let title = pick_text(&request.titles, &request.title_mode, index, "제목")?;
            let body = pick_text(&request.bodies, &request.body_mode, index, "내용")?;
            let comment = pick_text(&request.comments, &request.comment_mode, index, "댓글내용")?;
            // 마지막 회차가 아닐 때만 글 등록 직후 타이머를 emit하고 1분을 채웁니다.
            let sleep_after = index + 1 < request.count;
            let pair_reports = run_naver_post_with_comment_macro(
                NaverPostWithCommentRequest {
                    title,
                    body,
                    comment,
                    host: request.host.clone(),
                    port: request.port,
                    stock: Some(stock),
                    account_id: request.account_id.clone(),
                },
                &app,
                sleep_after,
            )
            .map_err(|error| error.to_string())?;

            for report in pair_reports {
                reports.push(report);
                completed_actions += 1;
                // sleep은 run_naver_post_with_comment_macro 안에서 처리합니다.
            }

            continue;
        }

        if request.run_post {
            let title = pick_text(&request.titles, &request.title_mode, index, "제목")?;
            let body = pick_text(&request.bodies, &request.body_mode, index, "내용")?;

            reports.push(
                run_naver_discussion_macro(NaverDiscussionRequest {
                    title,
                    body,
                    host: request.host.clone(),
                    port: request.port,
                    target: AutomationTarget::Post,
                    submit_after_fill: true,
                    stock: Some(stock.clone()),
                    account_id: request.account_id.clone(),
                })
                .map_err(|error| error.to_string())?,
            );
            completed_actions += 1;
            sleep_between_actions(completed_actions, total_actions, &app);
        }

        if request.run_comment {
            let comment = pick_text(&request.comments, &request.comment_mode, index, "댓글내용")?;

            reports.push(
                run_naver_discussion_macro(NaverDiscussionRequest {
                    title: String::new(),
                    body: comment,
                    host: request.host.clone(),
                    port: request.port,
                    target: AutomationTarget::Comment,
                    submit_after_fill: true,
                    stock: Some(stock),
                    account_id: request.account_id.clone(),
                })
                .map_err(|error| error.to_string())?,
            );
            completed_actions += 1;
            sleep_between_actions(completed_actions, total_actions, &app);
        }
    }

    Ok(DiscussionBatchReport {
        completed: reports.len(),
        reports,
    })
}

// 등록 또는 댓글 작성 한 건이 끝난 뒤 다음 실행 전 1분을 기다리는 함수입니다.
// 대기 직전 프론트엔드로 "batch-wait-start" 이벤트를 보내 카운트다운 타이머를 표시합니다.
fn sleep_between_actions(completed_actions: usize, total_actions: usize, app: &tauri::AppHandle) {
    if completed_actions < total_actions {
        let _ = app.emit("batch-wait-start", serde_json::json!({ "seconds": 60u64 }));
        sleep(Duration::from_secs(60));
    }
}

// batch 실행 전 필수 입력과 선택 모드를 검증하는 함수입니다.
fn validate_batch_request(request: &DiscussionBatchRequest) -> Result<(), String> {
    if request.stocks.is_empty() {
        return Err("종목을 하나 이상 선택하세요.".to_owned());
    }

    if !request.run_post && !request.run_comment {
        return Err("행동을 선택하세요.".to_owned());
    }

    if request.count != 3 && request.count != 5 {
        return Err("실행 개수는 3개 또는 5개만 선택할 수 있습니다.".to_owned());
    }

    if request.run_post {
        validate_texts(&request.titles, &request.title_mode, "제목")?;
        validate_texts(&request.bodies, &request.body_mode, "내용")?;
    }

    if request.run_comment {
        validate_texts(&request.comments, &request.comment_mode, "댓글내용")?;
    }

    Ok(())
}

// 선택 모드별로 텍스트 목록이 올바른지 확인하는 함수입니다.
fn validate_texts(values: &[String], mode: &PickMode, label: &str) -> Result<(), String> {
    let count = values
        .iter()
        .filter(|value| !value.trim().is_empty())
        .count();

    if count == 0 {
        return Err(format!(
            "{label}이 비어 있습니다. CSV를 가져오거나 텍스트창에 입력하세요."
        ));
    }

    if matches!(mode, PickMode::Single) && count != 1 {
        return Err(format!(
            "{label}의 1개만 모드는 값이 정확히 1개일 때만 사용할 수 있습니다."
        ));
    }

    Ok(())
}

// 선택 모드에 따라 이번 실행에 사용할 문구 하나를 고르는 함수입니다.
fn pick_text(
    values: &[String],
    mode: &PickMode,
    index: usize,
    label: &str,
) -> Result<String, String> {
    let cleaned = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    if cleaned.is_empty() {
        return Err(format!("{label}이 비어 있습니다."));
    }

    let picked = match mode {
        PickMode::Random => cleaned[pseudo_index(cleaned.len())],
        PickMode::Sequential => cleaned[index % cleaned.len()],
        PickMode::Single => cleaned[0],
    };

    Ok(picked.to_owned())
}

// CSV 헤더에서 원하는 열 이름의 위치를 찾는 함수입니다.
fn find_header_index(header: &[String], names: &[&str]) -> Option<usize> {
    header.iter().position(|value| {
        let normalized = value.trim().to_lowercase().replace(' ', "");
        names
            .iter()
            .any(|name| normalized == name.to_lowercase().replace(' ', ""))
    })
}

// 값이 비어 있지 않을 때 목록에 추가하는 함수입니다.
fn push_non_empty(values: &mut Vec<String>, value: Option<&String>) {
    if let Some(value) = value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        values.push(value.to_owned());
    }
}

// 쉼표, 큰따옴표, 줄바꿈을 처리하는 간단한 CSV parser 함수입니다.
fn parse_csv_rows(csv_text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = csv_text.chars().peekable();
    let mut in_quotes = false;

    while let Some(character) = chars.next() {
        match character {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                row.push(field.trim().to_owned());
                field.clear();
            }
            '\n' if !in_quotes => {
                row.push(field.trim().trim_end_matches('\r').to_owned());
                field.clear();
                rows.push(row);
                row = Vec::new();
            }
            _ => field.push(character),
        }
    }

    if in_quotes {
        return Err("CSV 따옴표가 닫히지 않았습니다.".to_owned());
    }

    row.push(field.trim().trim_end_matches('\r').to_owned());

    if row.iter().any(|field| !field.is_empty()) {
        rows.push(row);
    }

    Ok(rows)
}

// 네이버 API 응답 JSON에서 종목 후보를 재귀적으로 수집하는 함수입니다.
fn collect_stock_candidates(value: &Value, candidates: &mut Vec<StockCandidate>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_stock_candidates(item, candidates);
            }
        }
        Value::Object(object) => {
            if let Some(code) = direct_string(
                object,
                &[
                    "itemCode",
                    "itemcode",
                    "stockCode",
                    "stockcode",
                    "code",
                    "symbolCode",
                    "symbolcode",
                    "localCode",
                    "localcode",
                ],
            ) {
                if looks_like_stock_code(&code) {
                    let name = direct_string(
                        object,
                        &[
                            "itemName",
                            "itemname",
                            "stockName",
                            "stockname",
                            "name",
                            "korName",
                            "korname",
                            "displayName",
                            "displayname",
                        ],
                    )
                    .unwrap_or_else(|| code.clone());
                    candidates.push(StockCandidate {
                        link: format!(
                            "https://stock.naver.com/domestic/stock/{code}/discussion?chip=all"
                        ),
                        name,
                        code,
                    });
                }
            }

            for child in object.values() {
                collect_stock_candidates(child, candidates);
            }
        }
        _ => {}
    }
}

// 중복 종목 코드를 제거하는 함수입니다.
fn dedupe_stocks(candidates: Vec<StockCandidate>) -> Vec<StockCandidate> {
    let mut seen = BTreeMap::new();
    let mut unique = Vec::new();

    for stock in candidates {
        if seen.insert(stock.code.clone(), ()).is_none() {
            unique.push(stock);
        }
    }

    unique
}

// JSON 객체의 직접 필드에서 문자열 값을 읽는 함수입니다.
fn direct_string(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        object.get(*key).and_then(|value| match value {
            Value::String(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
    })
}

// 문자열이 네이버 종목 코드 형태인지 확인하는 함수입니다.
fn looks_like_stock_code(value: &str) -> bool {
    value.chars().count() == 6 && value.chars().all(|character| character.is_ascii_digit())
}

// API 실패 시 UI가 완전히 비지 않게 해주는 기본 종목 목록입니다.
fn fallback_stocks() -> Vec<StockCandidate> {
    [
        ("삼성전자", "005930"),
        ("SK하이닉스", "000660"),
        ("NAVER", "035420"),
        ("현대차", "005380"),
        ("LG전자", "066570"),
        ("한화시스템", "272210"),
    ]
    .into_iter()
    .map(|(name, code)| StockCandidate {
        name: name.to_owned(),
        code: code.to_owned(),
        link: format!("https://stock.naver.com/domestic/stock/{code}/discussion?chip=all"),
    })
    .collect()
}

// 목록에서 실행 시점 기준으로 하나를 고르는 함수입니다.
fn pseudo_index(len: usize) -> usize {
    if len == 0 {
        return 0;
    }

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    (nanos as usize) % len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_template_csv_without_header_values() {
        let csv = "제목,내용,댓글내용\n제목1,내용1,댓글1\n제목2,내용2,댓글2\n";

        let parsed = parse_discussion_template_csv(csv.to_owned()).expect("csv should parse");

        assert_eq!(parsed.titles, vec!["제목1", "제목2"]);
        assert_eq!(parsed.bodies, vec!["내용1", "내용2"]);
        assert_eq!(parsed.comments, vec!["댓글1", "댓글2"]);
    }

    #[test]
    fn parses_quoted_csv_fields() {
        let csv = "제목,내용,댓글내용\n\"제목, 쉼표\",\"여러\n줄\",\"댓글\"\"따옴표\"";

        let parsed = parse_discussion_template_csv(csv.to_owned()).expect("csv should parse");

        assert_eq!(parsed.titles, vec!["제목, 쉼표"]);
        assert_eq!(parsed.bodies, vec!["여러\n줄"]);
        assert_eq!(parsed.comments, vec!["댓글\"따옴표"]);
    }

    #[test]
    fn rejects_single_mode_when_multiple_values_exist() {
        let values = vec!["a".to_owned(), "b".to_owned()];

        let error = validate_texts(&values, &PickMode::Single, "제목").unwrap_err();

        assert!(error.contains("정확히 1개"));
    }

    #[test]
    fn collect_stock_candidates_reads_lowercase_naver_stock_fields() {
        let value = serde_json::json!([
            {
                "itemname": "삼성전자",
                "itemcode": "005930"
            }
        ]);
        let mut candidates = Vec::new();

        collect_stock_candidates(&value, &mut candidates);

        assert_eq!(candidates[0].name, "삼성전자");
        assert_eq!(candidates[0].code, "005930");
    }
}
