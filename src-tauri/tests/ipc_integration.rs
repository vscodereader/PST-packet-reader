//! IPC integration tests.
//!
//! These drive the *real* command handlers through a Tauri mock runtime —
//! the same `register_handlers` / `manage_stores` wiring `run()` uses in
//! production — so the `tauri::State` plumbing and the `#[tauri::command]`
//! argument deserialization get exercised end to end. No webview rendering,
//! USB/ADB device, or naver-login sidecar is involved.

use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY,
};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow, WebviewWindowBuilder};
use tempfile::TempDir;

use pstmacro_lib::{manage_stores, register_handlers};

/// Builds a mock app carrying the production command surface and a fresh
/// seeded store set rooted at a temp dir. The returned [`TempDir`] guard must
/// be kept alive for the app's lifetime.
fn mock_app() -> (App<MockRuntime>, TempDir) {
    let dir = tempfile::tempdir().expect("create temp app data dir");
    let app = register_handlers(mock_builder())
        .build(mock_context(noop_assets()))
        .expect("build mock app");
    manage_stores(app.handle(), dir.path()).expect("seed and manage stores");
    (app, dir)
}

fn main_webview(app: &App<MockRuntime>) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, "main", Default::default())
        .build()
        .expect("build mock webview")
}

fn invoke(wv: &WebviewWindow<MockRuntime>, cmd: &str, args: Value) -> Result<Value, Value> {
    get_ipc_response(
        wv,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| body.deserialize::<Value>().expect("deserialize response"))
}

fn invoke_ok(wv: &WebviewWindow<MockRuntime>, cmd: &str, args: Value) -> Value {
    invoke(wv, cmd, args).unwrap_or_else(|err| panic!("`{cmd}` returned an error: {err}"))
}

fn array(value: &Value) -> &Vec<Value> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("expected a JSON array, got: {value}"))
}

#[test]
fn every_list_command_returns_its_seeded_collection() {
    let (app, _dir) = mock_app();
    let wv = main_webview(&app);

    for cmd in [
        "list_accounts",
        "list_posts",
        "list_queue_now",
        "list_queue_scheduled",
        "list_stocks",
        "list_activity",
        "list_stats",
        "list_log_batches",
        "list_cafes",
        "list_bands",
    ] {
        let out = invoke_ok(&wv, cmd, json!({}));
        assert!(!array(&out).is_empty(), "`{cmd}` returned an empty seed");
    }
}

#[test]
fn greet_command_round_trips_the_name() {
    let (app, _dir) = mock_app();
    let wv = main_webview(&app);

    let out = invoke_ok(&wv, "greet", json!({ "name": "Pallas" }));

    assert_eq!(out, json!("Hello, Pallas! You've been greeted from Rust!"));
}

#[test]
fn account_mutations_round_trip_through_the_store() {
    let (app, _dir) = mock_app();
    let wv = main_webview(&app);

    let before = invoke_ok(&wv, "list_accounts", json!({}));
    let total = array(&before).len();
    let account = array(&before)[0].clone();
    let id = account["id"].as_str().expect("account id").to_string();

    let after_delete = invoke_ok(&wv, "delete_accounts", json!({ "ids": [id] }));
    assert_eq!(array(&after_delete).len(), total - 1);

    let after_add = invoke_ok(&wv, "add_account", json!({ "account": account }));
    assert_eq!(array(&after_add).len(), total);

    // update replaces in place — length stays put.
    let after_update = invoke_ok(&wv, "update_account", json!({ "account": account }));
    assert_eq!(array(&after_update).len(), total);
}

#[test]
fn post_mutations_round_trip_through_the_store() {
    let (app, _dir) = mock_app();
    let wv = main_webview(&app);

    let before = invoke_ok(&wv, "list_posts", json!({}));
    let total = array(&before).len();
    let post = array(&before)[0].clone();
    let id = post["id"].as_str().expect("post id").to_string();

    // upsert of an existing post replaces in place.
    let after_upsert = invoke_ok(&wv, "upsert_post", json!({ "post": post }));
    assert_eq!(array(&after_upsert).len(), total);

    let after_delete = invoke_ok(&wv, "delete_post", json!({ "id": id }));
    assert_eq!(array(&after_delete).len(), total - 1);
}

#[test]
fn queue_now_cancel_removes_the_targeted_item() {
    let (app, _dir) = mock_app();
    let wv = main_webview(&app);

    let before = invoke_ok(&wv, "list_queue_now", json!({}));
    let total = array(&before).len();
    let id = array(&before)[0]["id"]
        .as_str()
        .expect("now id")
        .to_string();

    let after = invoke_ok(&wv, "cancel_queue_now", json!({ "id": id }));

    assert_eq!(array(&after).len(), total - 1);
}

#[test]
fn scheduled_add_promote_and_cancel_flow() {
    let (app, _dir) = mock_app();
    let wv = main_webview(&app);

    let scheduled = invoke_ok(&wv, "list_queue_scheduled", json!({}));
    let item = array(&scheduled)[0].clone();
    let id = item["id"].as_str().expect("scheduled id").to_string();
    let total = array(&scheduled).len();

    // A far-future timestamp (year 2100) clears the past-time guard.
    let after_add = invoke_ok(
        &wv,
        "add_queue_scheduled",
        json!({ "item": item, "at": 4_102_444_800_000_i64 }),
    );
    assert_eq!(array(&after_add).len(), total + 1);

    // A past timestamp is rejected with an error.
    let rejected = invoke(
        &wv,
        "add_queue_scheduled",
        json!({ "item": array(&scheduled)[0].clone(), "at": 0_i64 }),
    );
    assert!(rejected.is_err(), "past schedule time should be rejected");

    // Promote drops it from scheduled and appends to the now queue.
    let now_after = invoke_ok(&wv, "promote_queue_scheduled", json!({ "id": id }));
    assert!(now_after.is_array());

    let cancelled = invoke_ok(&wv, "cancel_queue_scheduled", json!({ "id": id }));
    assert!(cancelled.is_array());
}

#[test]
fn auth_commands_operate_against_a_temp_app_data_root() {
    // The auth commands resolve their paths from LOCALAPPDATA; point it at a
    // temp dir so the test never touches the real user data directory. This is
    // the only test that mutates the env var, so no lock is needed.
    let data = tempfile::tempdir().expect("temp LOCALAPPDATA");
    std::env::set_var("LOCALAPPDATA", data.path());

    let (app, _dir) = mock_app();
    let wv = main_webview(&app);

    // bootstrap_runtime creates the runtime dirs and reports their paths.
    let paths = invoke_ok(&wv, "bootstrap_runtime", json!({}));
    assert!(paths["root"].is_string(), "runtime paths: {paths}");
    assert!(data.path().join("pstmacro").join("accounts").is_dir());

    // save_accounts with an empty list writes accounts.json and echoes [].
    let saved = invoke_ok(&wv, "save_accounts", json!({ "accounts": [] }));
    assert_eq!(saved, json!([]));

    // An unknown account has no cookie file → null. Tauri exposes command
    // arguments in camelCase, so `account_id` is sent as `accountId`.
    let cookies = invoke_ok(&wv, "get_account_cookies", json!({ "accountId": "nobody" }));
    assert!(cookies.is_null());

    // The cookie-refresh queue starts idle.
    let status = invoke_ok(&wv, "get_queue_status", json!({}));
    assert_eq!(status["isRunning"], json!(false));

    // Enqueuing with no account ids is a no-op that still returns a well-formed
    // status (the worker finds nothing pending and stops — no sidecar/adb).
    let enqueued = invoke_ok(
        &wv,
        "enqueue_cookie_refresh",
        json!({ "accountIds": [], "headless": true, "useAdb": false }),
    );
    assert!(enqueued["jobs"].is_array(), "queue status: {enqueued}");

    std::env::remove_var("LOCALAPPDATA");
}
