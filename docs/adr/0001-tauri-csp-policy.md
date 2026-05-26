# 1. Tauri CSP policy

Date: 2026-05-26

## Status

Accepted

## Context

`src-tauri/tauri.conf.json` shipped with `app.security.csp = null`,
which disables the Content Security Policy entirely — any script can be
loaded or evaluated, defeating Tauri's defense-in-depth against an XSS
in the webview. Tauri 2 also requires several non-standard schemes for
its own machinery (`asset:`, `ipc:`, `https://asset.localhost`,
`http://ipc.localhost`) plus `'unsafe-eval'` for HMR in dev.

## Decision

Set a single CSP that covers both dev and bundled use:

```
default-src 'self';
img-src 'self' data: asset: https://asset.localhost;
style-src 'self' 'unsafe-inline';
script-src 'self' 'unsafe-eval' 'wasm-unsafe-eval';
connect-src 'self' ipc: http://ipc.localhost
```

- `'unsafe-inline'` in `style-src` is required by Vite's runtime CSS
  injection and by libraries that inject inline styles (TanStack Query
  devtools etc.).
- `'unsafe-eval'` + `'wasm-unsafe-eval'` in `script-src` keep HMR and
  any wasm-based deps working.
- `ipc:` + `http://ipc.localhost` are the official Tauri 2 IPC schemes.
- `asset:` + `https://asset.localhost` cover Tauri's asset protocol.

## Consequences

- XSS surface narrows to `'self'` for scripts and the explicitly listed
  Tauri schemes — no third-party CDN, no remote eval.
- If the app later needs to load remote images, fonts, or scripts, the
  corresponding directive must be relaxed via a follow-up ADR.
- A stricter production-only CSP (dropping `'unsafe-eval'`) is a
  follow-up: should be set in a dedicated `Capability` once the bundled
  build no longer needs HMR-style eval.
