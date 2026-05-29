#!/usr/bin/env node
// @yao-pkg/pkg로 scripts/naver-login.cjs를 플랫폼별 단일 실행 파일로 패키징
import { execSync } from 'child_process';
import { mkdirSync, writeFileSync, chmodSync } from 'fs';

mkdirSync('src-tauri/binaries', { recursive: true });

// Linux 개발 환경용 스텁 (cargo check / cargo build --dev 에서 바이너리 존재를 요구함)
const linuxStub = 'src-tauri/binaries/naver-login-x86_64-unknown-linux-gnu';
writeFileSync(
  linuxStub,
  '#!/bin/sh\necho "naver-login sidecar: Linux stub only" >&2\nexit 1\n'
);
chmodSync(linuxStub, 0o755);

// Windows x64 실제 실행 파일
execSync(
  'pnpm exec pkg scripts/naver-login.cjs --target node22-win-x64 --output src-tauri/binaries/naver-login-x86_64-pc-windows-msvc --no-bytecode --public',
  { stdio: 'inherit' }
);
