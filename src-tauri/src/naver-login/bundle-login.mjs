#!/usr/bin/env node
import { execSync } from "node:child_process";
import { chmodSync, cpSync, existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
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

const linuxStub = path.join(root, "src-tauri/binaries/naver-login-x86_64-unknown-linux-gnu");
writeFileSync(linuxStub, '#!/bin/sh\necho "naver-login sidecar: Linux stub only" >&2\nexit 1\n');
chmodSync(linuxStub, 0o755);

const outPath = path.join(root, "src-tauri/binaries/naver-login-x86_64-pc-windows-msvc");
execSync(
  `pnpm exec pkg "${compiledLoginScript}" --target node22-win-x64 --output "${outPath}" --no-bytecode --public --config "${path.join(buildDir, "package.json")}"`,
  { cwd: root, stdio: "inherit" },
);

rmSync(buildDir, { recursive: true });
