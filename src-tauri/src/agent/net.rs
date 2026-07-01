//! 서버 통신(에이전트 → 중앙 서버). 등록/하트비트/상태/결과 POST + SSE 스트림 열기.
//! reqwest 비동기 클라이언트 + `chunk()`로 SSE를 읽으므로 추가 feature 불필요.

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RegisterReq<'a> {
    code: &'a str,
    name: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterResp {
    pub device_id: String,
    pub device_token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HeartbeatReq<'a> {
    ip: Option<&'a str>,
    state: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StateReq<'a> {
    state: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResultReq<'a> {
    level: &'a str,
    msg: &'a str,
}

fn join(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

/// 기기코드로 등록 → 장기 기기토큰 발급(§6).
pub async fn register(
    client: &reqwest::Client,
    base: &str,
    code: &str,
    name: Option<&str>,
) -> Result<RegisterResp, String> {
    let resp = client
        .post(join(base, "/device/register"))
        .json(&RegisterReq { code, name })
        .send()
        .await
        .map_err(|e| format!("등록 요청 실패: {e}"))?;
    if !resp.status().is_success() {
        return Err(server_error(resp).await);
    }
    resp.json::<RegisterResp>()
        .await
        .map_err(|e| format!("등록 응답 파싱 실패: {e}"))
}

/// 하트비트 + 현재 IP/상태 보고(§4-1).
pub async fn heartbeat(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    ip: Option<&str>,
    state: &str,
) -> Result<(), String> {
    post_authed(client, base, token, "/agent/heartbeat", &HeartbeatReq { ip, state }).await
}

/// 상태 전이 보고(ROTATING 등, §4). IP는 heartbeat로 보낸다.
pub async fn post_state(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    state: &str,
) -> Result<(), String> {
    post_authed(client, base, token, "/agent/state", &StateReq { state }).await
}

/// 명령 결과 회신(§10-4).
pub async fn post_result(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    command_id: &str,
    level: &str,
    msg: &str,
) -> Result<(), String> {
    let path = format!("/agent/commands/{command_id}/result");
    post_authed(client, base, token, &path, &ResultReq { level, msg }).await
}

/// 게시 결과 보고(§10-4-2). 하위 로컬 게시 완료 로그(`LogBatch`) 1건을 그대로 올린다.
/// body는 `LogBatch`를 직렬화한 JSON(서버는 id/title/at/items만 읽고 나머지는 무시).
pub async fn post_report(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    body: &serde_json::Value,
) -> Result<(), String> {
    post_authed(client, base, token, "/agent/post-report", body).await
}

/// 로그인 결과 보고(§10-4-1). 4분류 + 누적을 구조화 JSON으로 올린다(결과보고 '로그인 결과' 탭).
pub async fn post_login_report(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    body: &serde_json::Value,
) -> Result<(), String> {
    post_authed(client, base, token, "/agent/login-report", body).await
}

/// 앱 로그 스트림 전송(#324). 링버퍼에서 꺼낸 앱 tracing 로그 줄들을 서버로 올려, Admin 로그
/// 창(통신로그)에 하위의 실제 로그(네이버 원문 응답 등)가 그대로 보이게 한다.
pub async fn post_log(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    lines: &[String],
) -> Result<(), String> {
    #[derive(Serialize)]
    struct LogReq<'a> {
        lines: &'a [String],
    }
    post_authed(client, base, token, "/agent/log", &LogReq { lines }).await
}

/// SSE 스트림 열기(`GET /agent/stream?token=`). 브라우저가 아니므로 토큰을 쿼리로 싣는다.
/// 반환된 Response를 `chunk()`로 읽어 `data:` 줄을 파싱한다(호출부).
pub async fn open_stream(
    client: &reqwest::Client,
    base: &str,
    token: &str,
) -> Result<reqwest::Response, String> {
    let resp = client
        .get(join(base, "/agent/stream"))
        .query(&[("token", token)])
        .send()
        .await
        .map_err(|e| format!("스트림 연결 실패: {e}"))?;
    if !resp.status().is_success() {
        return Err(server_error(resp).await);
    }
    Ok(resp)
}

async fn post_authed<B: Serialize>(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    path: &str,
    body: &B,
) -> Result<(), String> {
    let resp = client
        .post(join(base, path))
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("요청 실패({path}): {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(server_error(resp).await)
    }
}

async fn server_error(resp: reqwest::Response) -> String {
    let code = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    format!("서버 거부({code}): {body}")
}
