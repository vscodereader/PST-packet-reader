# Tauri capabilities

Capabilities declare which Tauri / plugin permissions the frontend may
request at runtime. Keep the set as small as the app needs.

## Current state

`default.json` grants `core:default` only — Tauri's baseline core
permissions for window, webview, event, path, etc. There are no plugin
permissions.

## Adding capabilities

1. **Add the plugin** to `Cargo.toml` (`tauri-plugin-<name>`) and register
   it in `src/lib.rs` with `.plugin(tauri_plugin_<name>::init())`. If it
   has a JS counterpart, add `@tauri-apps/plugin-<name>` to the root
   `package.json`.
2. **Prefer the narrowest permission**. Instead of `<plugin>:default`
   (which usually allows everything the plugin can do), enumerate the
   specific permissions you need (e.g. `opener:allow-open-url`) and add
   a scope object if the plugin supports one. The plugin's documentation
   lists available permissions.
3. **Document why** each permission was added — a one-liner comment in
   the capability JSON is acceptable (JSON-with-comments is fine for
   capability files in Tauri 2).

## Removing capabilities

When a plugin is no longer used, remove it from:
- `src-tauri/Cargo.toml` `[dependencies]`
- `src-tauri/src/lib.rs` (the `.plugin(...)` call)
- root `package.json` (the JS bindings)
- this `capabilities/*.json` file

`src-tauri/gen/` is regenerated on build, no manual edit needed.
