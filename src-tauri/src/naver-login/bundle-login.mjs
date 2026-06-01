#!/usr/bin/env node
import { execSync } from "node:child_process";
import { chmodSync, cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const sourceDir = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(sourceDir, "../../..");
const buildDir = path.join(root, ".sidecar-build");
const compiledLoginScript = path.join(buildDir, "naver-login.cjs");

if (existsSync(buildDir)) rmSync(buildDir, { recursive: true });
mkdirSync(buildDir, { recursive: true });

cpSync(path.join(sourceDir, "sidecar-package.json"), path.join(buildDir, "package.json"));
cpSync(
  path.join(sourceDir, "sidecar-package-lock.json"),
  path.join(buildDir, "package-lock.json"),
);

execSync(
  [
    "pnpm exec swc",
    `"${path.join(sourceDir, "naver-login.ts")}"`,
    `--out-file "${compiledLoginScript}"`,
    "--config jsc.parser.syntax=typescript",
    "--config jsc.target=es2022",
    "--config module.type=commonjs",
  ].join(" "),
  { cwd: root, stdio: "inherit" },
);

execSync("npm ci", { cwd: buildDir, stdio: "inherit" });

mkdirSync(path.join(root, "src-tauri/binaries"), { recursive: true });

// Linux/WSL와 Windows 양쪽에서 로그인 sidecar가 실제로 동작하도록 두 타깃을 모두 빌드한다.
// (이전에는 Linux를 동작하지 않는 더미 stub으로 두었으나, WSL에서도 로그인을 돌리기 위해
//  실제 Linux 바이너리를 빌드한다. 시스템에 설치된 Chrome을 executablePath로 실행한다.)
const targets = [
  { pkgTarget: "node22-linux-x64", output: "naver-login-x86_64-unknown-linux-gnu" },
  { pkgTarget: "node22-win-x64", output: "naver-login-x86_64-pc-windows-msvc" },
];

for (const { pkgTarget, output } of targets) {
  const outPath = path.join(root, "src-tauri/binaries", output);
  execSync(
    `pnpm exec pkg "${compiledLoginScript}" --target ${pkgTarget} --output "${outPath}" --no-bytecode --public --config "${path.join(buildDir, "package.json")}"`,
    { cwd: root, stdio: "inherit" },
  );
  if (pkgTarget.startsWith("node22-linux")) {
    chmodSync(outPath, 0o755);
  }
}

rmSync(buildDir, { recursive: true });
