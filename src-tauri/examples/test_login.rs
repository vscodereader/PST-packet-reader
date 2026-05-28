//! 실제 네이버 로그인 테스트.
//! 실행: NAVER_ID=아이디 NAVER_PW=비밀번호 cargo run --example test_login
//! (src-tauri 디렉터리에서 실행)

#[tokio::main]
async fn main() {
    let id = std::env::var("NAVER_ID").expect("NAVER_ID 환경변수를 설정하세요");
    let pw = std::env::var("NAVER_PW").expect("NAVER_PW 환경변수를 설정하세요");

    println!("로그인 시도 중: {}****", &id[..id.len().min(2)]);

    match pstmacro_lib::auth::login(&id, &pw).await {
        Ok(v) => println!("성공: {}", v),
        Err(e) => {
            eprintln!("실패: {e}");
            std::process::exit(1);
        }
    }
}
