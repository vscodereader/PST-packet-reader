#!/usr/bin/env node
import { execSync } from 'child_process';
import { mkdirSync, writeFileSync, chmodSync, cpSync, existsSync, rmSync } from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const buildDir = path.join(root, '.sidecar-build');

if (existsSync(buildDir)) rmSync(buildDir, { recursive: true });
mkdirSync(buildDir, { recursive: true });

cpSync(path.join(root, 'scripts/naver-login.cjs'), path.join(buildDir, 'naver-login.cjs'));
cpSync(path.join(root, 'scripts/sidecar-package.json'), path.join(buildDir, 'package.json'));
cpSync(path.join(root, 'scripts/sidecar-package-lock.json'), path.join(buildDir, 'package-lock.json'));

execSync('npm ci', { cwd: buildDir, stdio: 'inherit' });

mkdirSync(path.join(root, 'src-tauri/binaries'), { recursive: true });

// cargo check가 바이너리 존재를 요구하므로 Linux 개발 환경용 스텁을 생성한다
const linuxStub = path.join(root, 'src-tauri/binaries/naver-login-x86_64-unknown-linux-gnu');
writeFileSync(linuxStub, '#!/bin/sh\necho "naver-login sidecar: Linux stub only" >&2\nexit 1\n');
chmodSync(linuxStub, 0o755);

const outPath = path.join(root, 'src-tauri/binaries/naver-login-x86_64-pc-windows-msvc');
execSync(
  `pnpm exec pkg "${path.join(buildDir, 'naver-login.cjs')}" --target node22-win-x64 --output "${outPath}" --no-bytecode --public --config "${path.join(buildDir, 'package.json')}"`,
  { cwd: root, stdio: 'inherit' }
);

rmSync(buildDir, { recursive: true });
