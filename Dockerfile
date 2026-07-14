# pstmacro-server(Admin 중앙 서버) 컨테이너 — GCP Cloud Run 배포용.
#
# 서버는 실행파일 옆 `dist/`(Admin 웹 admin.html)를 직접 서빙하도록 설계됐다(server/src/routes.rs).
# 그래서 self-contained 이미지로 만든다: ① 프론트(dist) 빌드 → ② 서버(Rust) 빌드 → ③ 런타임에 둘 다 복사.
# Admin 웹을 별도(Vercel/Cloudflare 등)로 배포한다면 아래 `frontend` 스테이지와 dist COPY를 빼면 서버-only.
#
# ⚠️ 이 Dockerfile은 로컬에서 아직 `docker build` 검증 전이다(이 환경엔 docker 없음). 팀장님/배포자가
#    한 번 빌드하며 rust/node 버전·경로를 실환경에 맞춰야 할 수 있다.

# ── 1) 프론트(Admin dist) 빌드 ──
FROM node:20-bookworm-slim AS frontend
WORKDIR /app
RUN corepack enable && corepack prepare pnpm@9.15.0 --activate
# 의존성 먼저(캐시). 루트 프로젝트의 lockfile 사용.
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY . .
# tsc + vite build → dist/ (index.html + admin.html + assets)
RUN pnpm build

# ── 2) 서버(Rust) 빌드 ──
FROM rust:1-bookworm AS server
WORKDIR /app/server
COPY server/ ./
# 네이티브 Linux 타깃 릴리즈 빌드(.cargo/config.toml의 windows-msvc 설정은 이 타깃엔 무영향).
RUN cargo build --release --bin pstmacro-server

# ── 3) 런타임(가벼운 debian + 인증서만) ──
FROM debian:bookworm-slim
# reqwest(rustls)·sqlx TLS·HTTPS 아웃바운드에 필요한 CA 인증서.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=server /app/server/target/release/pstmacro-server /app/pstmacro-server
# 서버가 exe 옆 dist/ 를 찾아 Admin 웹을 서빙한다(routes.rs). 별도 배포면 이 줄 제거.
COPY --from=frontend /app/dist /app/dist
# Cloud Run은 PORT 환경변수를 주입하고 그 포트로 listen 하길 요구한다(config.rs가 PORT 우선 처리).
ENV PORT=8080
EXPOSE 8080
CMD ["/app/pstmacro-server"]
