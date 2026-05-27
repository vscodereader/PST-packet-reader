use pstmacro_lib::naver_automation::{
    run_naver_discussion_macro, AutomationTarget, NaverDiscussionRequest,
};
use std::env;
use std::fs;
use std::io::{self, Read, Write};

// CLI 프로그램의 진입점입니다.
fn main() {
    match run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("오류: {error}");
            std::process::exit(1);
        }
    }
}

// 사용자 입력을 받아 네이버 토론 자동화를 실행하는 함수입니다.
fn run() -> Result<(), String> {
    let args = CliArgs::parse(env::args().skip(1).collect())?;
    let target = match args.target {
        Some(target) => target,
        None => prompt_target()?,
    };
    let title = if matches!(target, AutomationTarget::Post) {
        match (args.title, args.title_file) {
            (Some(title), None) => title,
            (None, Some(path)) => read_text_file(&path, "제목")?,
            (Some(_), Some(_)) => {
                return Err("--title과 --title-file은 같이 사용할 수 없습니다.".to_owned());
            }
            (None, None) => prompt_line("제목 입력: ")?,
        }
    } else {
        if args.title.is_some() || args.title_file.is_some() {
            return Err("댓글쓰기에서는 제목을 입력하지 않습니다.".to_owned());
        }
        String::new()
    };
    let body = match (args.body, args.body_file) {
        (Some(body), None) => body,
        (None, Some(path)) => read_text_file(&path, body_label(&target))?,
        (Some(_), Some(_)) => {
            return Err("--body와 --body-file은 같이 사용할 수 없습니다.".to_owned());
        }
        (None, None) => prompt_body(body_label(&target))?,
    };

    println!();
    println!(
        "Chrome DevTools 엔드포인트 {}:{}에 연결합니다.",
        args.host, args.port
    );
    println!("네이버 로그인 완료 상태의 Chrome 탭을 사용합니다.");
    println!(
        "{} 실행 후 화면을 새로고침합니다.",
        target_submit_label(&target)
    );
    println!();

    let report = run_naver_discussion_macro(NaverDiscussionRequest {
        title,
        body,
        host: args.host,
        port: args.port,
        target,
        submit_after_fill: true,
    })
    .map_err(|error| error.to_string())?;

    println!("완료");
    println!(
        "로그인 확인: {}",
        report
            .login_profile
            .nickname
            .as_deref()
            .unwrap_or("닉네임 없음")
    );
    println!("선택 카테고리: {}", report.selected.category);
    println!("선택 순위: {}", report.selected.rank);
    println!("선택 종목: {}", report.selected.item_text);
    println!("입력 대상: {}", target_label(&report.target));
    if report.submitted {
        println!("등록 실행: 완료");
    }
    println!("현재 URL: {}", report.current_url);

    if report.register_button_highlighted {
        println!("등록하기 버튼을 빨간 테두리로 표시했습니다.");
    }

    Ok(())
}

struct CliArgs {
    title: Option<String>,
    title_file: Option<String>,
    body: Option<String>,
    body_file: Option<String>,
    host: String,
    port: u16,
    target: Option<AutomationTarget>,
}

impl CliArgs {
    // 명령줄 옵션을 해석해 CLI 설정값으로 변환하는 함수입니다.
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut title = None;
        let mut title_file = None;
        let mut body = None;
        let mut body_file = None;
        let mut host = "127.0.0.1".to_owned();
        let mut port = 9222;
        let mut target = None;
        let mut index = 0;

        while index < args.len() {
            match args[index].as_str() {
                "--title" => {
                    index += 1;
                    title = Some(read_arg_value(&args, index, "--title")?);
                }
                "--title-file" => {
                    index += 1;
                    title_file = Some(read_arg_value(&args, index, "--title-file")?);
                }
                "--body" => {
                    index += 1;
                    body = Some(read_arg_value(&args, index, "--body")?);
                }
                "--body-file" => {
                    index += 1;
                    body_file = Some(read_arg_value(&args, index, "--body-file")?);
                }
                "--port" => {
                    index += 1;
                    port = read_arg_value(&args, index, "--port")?
                        .parse::<u16>()
                        .map_err(|error| format!("--port 값이 올바르지 않습니다: {error}"))?;
                }
                "--host" => {
                    index += 1;
                    host = read_arg_value(&args, index, "--host")?;
                }
                "--target" => {
                    index += 1;
                    target = Some(parse_target(&read_arg_value(&args, index, "--target")?)?);
                }
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                unknown => {
                    return Err(format!(
                        "알 수 없는 옵션입니다: {unknown}\n도움말: cargo run --bin naver_discussion_cli -- --help"
                    ));
                }
            }

            index += 1;
        }

        Ok(Self {
            title,
            title_file,
            body,
            body_file,
            host,
            port,
            target,
        })
    }
}

// 자동화 대상 enum을 화면에 표시할 한글 이름으로 바꾸는 함수입니다.
fn target_label(target: &AutomationTarget) -> &'static str {
    match target {
        AutomationTarget::Post => "글쓰기",
        AutomationTarget::Comment => "댓글",
    }
}

// 선택한 작업의 등록 동작 설명을 만드는 함수입니다.
fn target_submit_label(target: &AutomationTarget) -> &'static str {
    match target {
        AutomationTarget::Post => "등록하기 버튼 클릭",
        AutomationTarget::Comment => "댓글 입력 후 Enter",
    }
}

// 선택한 작업에 맞는 본문 입력 라벨을 반환하는 함수입니다.
fn body_label(target: &AutomationTarget) -> &'static str {
    match target {
        AutomationTarget::Post => "내용",
        AutomationTarget::Comment => "댓글 내용",
    }
}

// --target 옵션 문자열을 글쓰기/댓글쓰기 enum으로 변환하는 함수입니다.
fn parse_target(value: &str) -> Result<AutomationTarget, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "post" | "write" | "글쓰기" => Ok(AutomationTarget::Post),
        "comment" | "reply" | "댓글" => Ok(AutomationTarget::Comment),
        _ => Err("--target 값은 post 또는 comment 여야 합니다.".to_owned()),
    }
}

// UTF-8 텍스트 파일에서 제목 또는 본문을 읽는 함수입니다.
fn read_text_file(path: &str, label: &str) -> Result<String, String> {
    let value = fs::read_to_string(path)
        .map_err(|error| format!("{label} 파일을 읽을 수 없습니다: {path} ({error})"))?;
    let value = value.trim().to_owned();

    if value.is_empty() {
        return Err(format!("{label} 파일 내용이 비어 있습니다."));
    }

    Ok(value)
}

// 명령줄 옵션 뒤에 붙은 값을 안전하게 읽는 함수입니다.
fn read_arg_value(args: &[String], index: usize, name: &str) -> Result<String, String> {
    args.get(index)
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} 뒤에 값이 필요합니다."))
}

// 사용자가 1. 글쓰기 또는 2. 댓글쓰기 중 하나를 고르게 하는 함수입니다.
fn prompt_target() -> Result<AutomationTarget, String> {
    println!("작업 선택:");
    println!("1. 글쓰기");
    println!("2. 댓글쓰기");
    print!("번호 입력: ");
    io::stdout()
        .flush()
        .map_err(|error| format!("입력 프롬프트를 표시할 수 없습니다: {error}"))?;

    match read_console_line_lossy()?.as_str() {
        "1" => Ok(AutomationTarget::Post),
        "2" => Ok(AutomationTarget::Comment),
        value => Err(format!("1 또는 2를 입력해야 합니다. 입력값: {value}")),
    }
}

// 한 줄짜리 제목 입력을 받는 함수입니다.
fn prompt_line(label: &str) -> Result<String, String> {
    print!("{label}");
    io::stdout()
        .flush()
        .map_err(|error| format!("입력 프롬프트를 표시할 수 없습니다: {error}"))?;

    let value = read_console_line_lossy()?;

    if value.is_empty() {
        return Err("제목이 비어 있습니다.".to_owned());
    }

    Ok(value)
}

// 여러 줄 본문을 입력받고 END가 나오면 입력을 끝내는 함수입니다.
fn prompt_body(label: &str) -> Result<String, String> {
    println!("{label} 입력:");
    println!("여러 줄 입력 가능. 마지막 줄에 END 입력 후 Enter를 누르면 실행합니다.");

    let mut lines = Vec::new();

    loop {
        let line = read_console_line_lossy()?;

        if line.trim() == "END" {
            break;
        }

        lines.push(line);
    }

    let body = lines.join("\n").trim().to_owned();

    if body.is_empty() {
        return Err(format!("{label}이 비어 있습니다."));
    }

    Ok(body)
}

// PowerShell/WSL 인코딩 차이로 깨질 수 있는 입력을 손실 허용 방식으로 읽는 함수입니다.
fn read_console_line_lossy() -> Result<String, String> {
    let mut bytes = Vec::new();

    loop {
        let mut byte = [0_u8; 1];
        let count = io::stdin()
            .read(&mut byte)
            .map_err(|error| format!("입력을 읽을 수 없습니다: {error}"))?;

        if count == 0 || byte[0] == b'\n' {
            break;
        }

        if byte[0] != b'\r' {
            bytes.push(byte[0]);
        }
    }

    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}

// CLI 사용법을 출력하는 함수입니다.
fn print_help() {
    println!("NAVER discussion macro CLI");
    println!();
    println!("사용:");
    println!("  cargo run --bin naver_discussion_cli");
    println!("  cargo run --bin naver_discussion_cli -- --title \"제목\" --body \"내용\"");
    println!();
    println!("옵션:");
    println!("  --title <text>  글쓰기 제목");
    println!("  --title-file <path>  UTF-8 제목 파일 경로");
    println!("  --body <text>   글쓰기 내용");
    println!("  --body-file <path>   UTF-8 내용 파일 경로");
    println!("  --host <host>   Chrome DevTools host. 기본값: 127.0.0.1");
    println!("  --port <port>   Chrome DevTools 포트. 기본값: 9222");
    println!("  --target <post|comment>   입력 대상. 생략하면 1/2 메뉴를 표시합니다.");
}
