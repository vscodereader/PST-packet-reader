//! Admin–하위 원격제어 중앙 서버 진입점. 설계 §9·§10.
//! DB = PostgreSQL(사수 확정). `DATABASE_URL` 설정 시 Postgres, 미설정 시 in-memory(개발/오프라인).
mod config;
mod crypto;
mod distribute;
mod error;
mod hub;
mod jwt;
mod model;
mod naver_stocks;
mod scheduled;
mod repo;
mod routes;
mod state;

use std::sync::Arc;

use config::Config;
use hub::Hub;
use model::{Operator, Role};
use repo::{MemoryRepo, PostgresRepo, Repository};
use state::AppState;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cfg = Config::from_env();

    // 사수 확정 DB = PostgreSQL. DATABASE_URL 있으면 Postgres, 없으면 in-memory(개발/오프라인 미리보기).
    let repo: Arc<dyn Repository> = match &cfg.database_url {
        Some(url) => match PostgresRepo::connect(url).await {
            Ok(r) => {
                tracing::info!("PostgreSQL 연결됨");
                Arc::new(r)
            }
            Err(e) => {
                tracing::error!("PostgreSQL 연결 실패: {e}");
                std::process::exit(1);
            }
        },
        None => {
            tracing::warn!("DATABASE_URL 미설정 → in-memory 저장소(개발/오프라인 미리보기). 운영은 PostgreSQL 사용.");
            Arc::new(MemoryRepo::new())
        }
    };

    seed_super_admin(repo.as_ref()).await;

    // 보안: 운영 모드(DATABASE_URL 설정)에서 개발용 기본 비밀이 그대로면 기동 거부(fail-closed).
    // 기본 JWT 비밀은 소스에 노출돼 토큰 위조가 가능하고, 기본 암호화 키는 저장된 계정 PW를 누구나
    // 복호화할 수 있게 한다(§13 B). in-memory(개발) 모드에서는 경고만.
    let prod = cfg.database_url.is_some();
    if cfg.jwt_is_default {
        if prod {
            tracing::error!("운영 모드(DATABASE_URL 설정)인데 PSTMACRO_JWT_SECRET 미설정/기본값 — 토큰 위조 위험. 기동 거부.");
            std::process::exit(1);
        }
        tracing::warn!("⚠ 개발용 기본 JWT 비밀 사용 중 — 운영 배포 시 PSTMACRO_JWT_SECRET 반드시 설정.");
    }
    if cfg.enc_is_default {
        if prod {
            tracing::error!("운영 모드인데 PSTMACRO_ENC_KEY 미설정/기본값 — 저장된 계정 PW 복호화 위험. 기동 거부.");
            std::process::exit(1);
        }
        tracing::warn!("⚠ 개발용 기본 암호화 키 사용 중 — 운영 배포 시 PSTMACRO_ENC_KEY(32바이트) 반드시 설정.");
    }
    if cfg.public_server_url.is_none() {
        tracing::warn!("PSTMACRO_PUBLIC_URL 미설정 — 서버 주소는 '배포 전 결정'(사수). UI에 안내 표시.");
    }
    if cfg.tls_cert_path.is_none() || cfg.tls_key_path.is_none() {
        tracing::warn!("TLS 인증서/키 미설정 — '배포 전 결정'. 운영 배포 시 인증서/리버스프록시로 HTTPS 적용.");
    }

    // 계정 열거 방지용 더미 해시(없는 아이디 로그인 시에도 argon2 1회 — state.rs/login).
    let dummy_pw_hash =
        crypto::hash_password("pstmacro-dummy-verify-target").unwrap_or_default();

    let bind = cfg.bind_addr.clone();
    let state = AppState {
        repo,
        hub: Arc::new(Hub::new()),
        cfg: Arc::new(cfg),
        dummy_pw_hash: Arc::new(dummy_pw_hash),
        inventory: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        queue_states: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        scheduled: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    // 예약 게시 스케줄러(07-게시명령 4단계) — 1초마다 도래한 예약을 하위로 발송한다.
    tokio::spawn(scheduled::scheduler_loop(state.clone()));
    let app = routes::build_router(state);

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("바인드 실패 {bind}: {e}"));
    tracing::info!("pstmacro-server listening on {bind}");
    axum::serve(listener, app).await.expect("서버 종료");
}

/// SuperAdmin 부트스트랩(§5): 없으면 기본 `Superadmin`/`Superadmin` 시드 + 강제 비번변경 플래그.
async fn seed_super_admin(repo: &dyn Repository) {
    match repo.count_super_admins().await {
        Ok(0) => {
            let hash = match crypto::hash_password("Superadmin") {
                Ok(h) => h,
                Err(e) => {
                    tracing::error!("시드 해시 실패: {e}");
                    return;
                }
            };
            let _ = repo
                .create_operator(Operator {
                    login_id: "Superadmin".into(),
                    pw_hash: hash,
                    role: Role::Super,
                    approved: true,
                    must_change_password: true, // 기본 비번 → 첫 로그인 시 강제 변경(§5)
                    token_version: 1,
                })
                .await;
            tracing::info!("기본 SuperAdmin 시드(Superadmin/Superadmin) — 첫 로그인 시 비번 변경 강제");
        }
        Ok(n) => tracing::info!("SuperAdmin {n}명 — 시드 생략"),
        Err(e) => tracing::error!("SuperAdmin 조회 실패: {e}"),
    }
}
