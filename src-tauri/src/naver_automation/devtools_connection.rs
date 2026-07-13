use serde::Deserialize;
use std::io::ErrorKind;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use url::Url;

use super::{AutomationError, AutomationResult, DISCUSSION_URL};

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ChromeTarget {
    #[serde(default)]
    url: String,
    #[serde(default)]
    #[serde(rename = "type")]
    target_type: String,
    #[serde(default, rename = "webSocketDebuggerUrl")]
    pub(super) web_socket_debugger_url: String,
}

// Chrome DevTools의 열린 탭 중 네이버 증권 탭을 우선 선택하고, 없으면 새 탭을 만드는 함수입니다.
pub(super) fn select_or_create_target(host: &str, port: u16) -> AutomationResult<ChromeTarget> {
    let body = http_request(host, port, "GET", "/json/list")?;
    let targets: Vec<ChromeTarget> = serde_json::from_str(&body)?;

    if let Some(target) = targets
        .iter()
        .filter(|target| target.target_type == "page")
        .filter(|target| !target.web_socket_debugger_url.is_empty())
        .find(|target| target.url.contains("stock.naver.com"))
        .cloned()
    {
        return Ok(target);
    }

    if let Some(target) = targets
        .into_iter()
        .filter(|target| target.target_type == "page")
        .find(|target| !target.web_socket_debugger_url.is_empty())
    {
        return Ok(target);
    }

    let path = format!("/json/new?{}", percent_encode(DISCUSSION_URL));
    let created = http_request(host, port, "PUT", &path)?;
    let target: ChromeTarget = serde_json::from_str(&created)?;

    if target.web_socket_debugger_url.is_empty() {
        return Err(AutomationError::new(
            "Chrome DevTools 대상 탭을 만들 수 없습니다.",
        ));
    }

    Ok(target)
}

// DevTools가 준 WebSocket URL을 WSL에서 접근 가능한 host/port로 바꾸는 함수입니다.
pub(super) fn websocket_url_for_host(
    raw_url: &str,
    host: &str,
    port: u16,
) -> AutomationResult<Url> {
    let mut url = Url::parse(raw_url).map_err(|error| AutomationError::new(error.to_string()))?;
    url.set_host(Some(host))
        .map_err(|_| AutomationError::new("Chrome DevTools WebSocket host가 올바르지 않습니다."))?;
    url.set_port(Some(port))
        .map_err(|_| AutomationError::new("Chrome DevTools WebSocket port가 올바르지 않습니다."))?;
    Ok(url)
}

// Chrome DevTools HTTP 엔드포인트에 직접 요청을 보내는 함수입니다.
fn http_request(host: &str, port: u16, method: &str, path: &str) -> AutomationResult<String> {
    let mut stream = TcpStream::connect((host, port)).map_err(|error| {
        AutomationError::new(format!(
            "Chrome DevTools 포트 {host}:{port}에 연결할 수 없습니다. Chrome을 --remote-debugging-port={port} 옵션으로 실행하고 로그인한 뒤 다시 실행하세요. ({error})"
        ))
    })?;

    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );

    stream.write_all(request.as_bytes())?;

    let response = read_http_response(&mut stream)?;

    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| AutomationError::new("Chrome DevTools HTTP 응답을 해석할 수 없습니다."))?;

    let status_ok = headers.starts_with("HTTP/1.1 200")
        || headers.starts_with("HTTP/1.1 201")
        || headers.starts_with("HTTP/1.0 200")
        || headers.starts_with("HTTP/1.0 201");

    if !status_ok {
        return Err(AutomationError::new(format!(
            "Chrome DevTools HTTP 요청 실패: {headers}"
        )));
    }

    Ok(body.to_owned())
}

// Chrome DevTools HTTP 응답 전체를 읽는 함수입니다.
fn read_http_response(stream: &mut TcpStream) -> AutomationResult<String> {
    let end = Instant::now() + Duration::from_secs(10);
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];

    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                bytes.extend_from_slice(&buffer[..count]);

                if response_body_complete(&bytes) {
                    break;
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if response_body_complete(&bytes) {
                    break;
                }

                if Instant::now() >= end {
                    return Err(AutomationError::new(
                        "Chrome DevTools HTTP 응답 읽기 시간이 초과되었습니다.",
                    ));
                }

                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::ConnectionReset | ErrorKind::UnexpectedEof
                ) && response_body_complete(&bytes) =>
            {
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }

    String::from_utf8(bytes)
        .map_err(|error| AutomationError::new(format!("HTTP 응답이 UTF-8이 아닙니다: {error}")))
}

// Content-Length 기준으로 HTTP body를 모두 읽었는지 확인하는 함수입니다.
fn response_body_complete(bytes: &[u8]) -> bool {
    let Some(header_end) = find_header_end(bytes) else {
        return false;
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let Some(content_length) = parse_content_length(&headers) else {
        return false;
    };
    let body_start = header_end + 4;

    bytes.len().saturating_sub(body_start) >= content_length
}

// HTTP 헤더와 본문을 나누는 빈 줄 위치를 찾는 함수입니다.
fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

// HTTP 헤더에서 Content-Length 값을 읽는 함수입니다.
fn parse_content_length(headers: &str) -> Option<usize> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;

        if name.eq_ignore_ascii_case("content-length") {
            value.trim().parse::<usize>().ok()
        } else {
            None
        }
    })
}

// /json/new 요청에 넣을 URL을 percent-encoding 하는 함수입니다.
fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_header_end_locates_blank_line() {
        assert_eq!(find_header_end(b"HTTP/1.1 200 OK\r\n\r\nbody"), Some(15));
        assert_eq!(find_header_end(b"no header terminator"), None);
        assert_eq!(find_header_end(b""), None);
    }

    #[test]
    fn parse_content_length_reads_value_case_insensitively() {
        assert_eq!(
            parse_content_length("Content-Length: 42\r\nHost: x"),
            Some(42)
        );
        assert_eq!(parse_content_length("content-length:   7"), Some(7));
        assert_eq!(parse_content_length("Host: x\r\nAccept: y"), None);
        assert_eq!(parse_content_length("Content-Length: abc"), None);
    }

    #[test]
    fn response_body_complete_requires_full_body() {
        let full = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nabcd";
        assert!(response_body_complete(full));

        let partial = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nab";
        assert!(!response_body_complete(partial));

        // 헤더 종료(\r\n\r\n)가 없으면 미완료
        assert!(!response_body_complete(b"HTTP/1.1 200 OK"));
        // Content-Length 헤더가 없으면 미완료
        assert!(!response_body_complete(
            b"HTTP/1.1 200 OK\r\nHost: x\r\n\r\nbody"
        ));
    }

    #[test]
    fn percent_encode_preserves_unreserved_and_escapes_rest() {
        assert_eq!(percent_encode("abcXYZ-._~09"), "abcXYZ-._~09");
        assert_eq!(percent_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(percent_encode("?=&"), "%3F%3D%26");
    }
}
