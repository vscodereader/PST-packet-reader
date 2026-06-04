//! IPC integration tests.
//!
//! These drive the *real* command handlers through a Tauri mock runtime —
//! the same `register_handlers` / `manage_stores` wiring `run()` uses in
//! production — so the `tauri::State` plumbing and the `#[tauri::command]`
//! argument deserialization get exercised end to end. No webview rendering,
//! USB/ADB device, or naver-login sidecar is involved.

use std::sync::Mutex;

use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY,
};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow, WebviewWindowBuilder};
use tempfile::TempDir;

use pstmacro_lib::{manage_stores, register_handlers};

/// Mutex serialising tests that mutate the `LOCALAPPDATA` environment variable.
/// Because `std::env::set_var` is process-wide, parallel tests that set this
/// variable concurrently will race. Holding this mutex for the duration of any
/// such test prevents that.
static LOCALAPPDATA_LOCK: Mutex<()> = Mutex::new(());

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

    // These domains have static seed data and must always return non-empty.
    for cmd in [
        "list_accounts",
        "list_posts",
        "list_queue_now",
        "list_queue_scheduled",
        "list_stocks",
        "list_stats",
        "list_cafes",
        "list_bands",
    ] {
        let out = invoke_ok(&wv, cmd, json!({}));
        assert!(!array(&out).is_empty(), "`{cmd}` returned an empty seed");
    }

    // activity and log_batches start empty — they are filled by real actions,
    // not a static seed. Verify the commands return a JSON array (even if []).
    for cmd in ["list_activity", "list_log_batches"] {
        let out = invoke_ok(&wv, cmd, json!({}));
        let _ = array(&out); // panics if not an array
    }
}

#[test]
fn get_environment_status_returns_chrome_and_adb_state() {
    let (app, _dir) = mock_app();
    let wv = main_webview(&app);

    // Always resolves (never throws) and carries both probes' state, even on a
    // CI host with no Chrome and no ADB device (both report false).
    let out = invoke_ok(&wv, "get_environment_status", json!({}));

    assert!(out["chrome"]["installed"].is_boolean(), "chrome: {out}");
    assert!(out["adb"]["connected"].is_boolean(), "adb: {out}");
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
fn account_mutation_appends_to_activity_feed() {
    let (app, _dir) = mock_app();
    let wv = main_webview(&app);

    // Activity feed starts empty (no static seed).
    let before = invoke_ok(&wv, "list_activity", serde_json::json!({}));
    assert_eq!(array(&before).len(), 0);

    // add_account should prepend one activity row.
    let account = serde_json::json!({
        "id": "test-acct",
        "platform": "forum",
        "loginId": "test_user",
        "pw": "pw123",
        "status": "new",
        "last": "—",
        "tags": []
    });
    invoke_ok(
        &wv,
        "add_account",
        serde_json::json!({ "account": account }),
    );

    let after = invoke_ok(&wv, "list_activity", serde_json::json!({}));
    assert_eq!(array(&after).len(), 1);
    let text = after[0]["text"].as_str().expect("activity text");
    assert!(text.contains("추가됨"), "expected '추가됨' in: {text}");
}

#[test]
fn auth_commands_operate_against_a_temp_app_data_root() {
    // The auth commands resolve their paths from LOCALAPPDATA; point it at a
    // temp dir so the test never touches the real user data directory.
    // Hold the process-wide lock so this test doesn't race with other tests
    // that also mutate LOCALAPPDATA.
    let _guard = LOCALAPPDATA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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

#[test]
fn export_accounts_xlsx_appends_to_activity_feed() {
    let (app, _dir) = mock_app();
    let wv = main_webview(&app);

    // Activity feed starts empty.
    let before = invoke_ok(&wv, "list_activity", json!({}));
    assert_eq!(array(&before).len(), 0, "activity should start empty");

    // Export to a temp xlsx path.
    let out_path = std::env::temp_dir().join("test_export_accounts.xlsx");
    let result = invoke(
        &wv,
        "export_accounts_xlsx",
        json!({ "path": out_path.to_str().unwrap() }),
    );
    assert!(
        result.is_ok(),
        "export_accounts_xlsx should succeed: {result:?}"
    );

    // The output file must exist.
    assert!(
        out_path.exists(),
        "exported xlsx file should exist at {out_path:?}"
    );
    let _ = std::fs::remove_file(&out_path);

    // The activity feed should contain an entry with "내보냈어요".
    let after = invoke_ok(&wv, "list_activity", json!({}));
    let found = array(&after)
        .iter()
        .any(|item| item["text"].as_str().unwrap_or("").contains("내보냈어요"));
    assert!(
        found,
        "activity feed should contain '내보냈어요'; got: {after}"
    );
}

#[test]
fn export_activity_xlsx_appends_to_activity_feed() {
    let (app, _dir) = mock_app();
    let wv = main_webview(&app);

    // Activity feed starts empty.
    let before = invoke_ok(&wv, "list_activity", json!({}));
    assert_eq!(array(&before).len(), 0, "activity should start empty");

    // Export to a temp xlsx path.
    let out_path = std::env::temp_dir().join("test_export_activity.xlsx");
    let result = invoke(
        &wv,
        "export_activity_xlsx",
        json!({ "path": out_path.to_str().unwrap() }),
    );
    assert!(
        result.is_ok(),
        "export_activity_xlsx should succeed: {result:?}"
    );

    // The output file must exist.
    assert!(
        out_path.exists(),
        "exported xlsx file should exist at {out_path:?}"
    );
    let _ = std::fs::remove_file(&out_path);

    // The activity feed should now contain an entry with "내보냈어요".
    let after = invoke_ok(&wv, "list_activity", json!({}));
    let found = array(&after)
        .iter()
        .any(|item| item["text"].as_str().unwrap_or("").contains("내보냈어요"));
    assert!(
        found,
        "activity feed should contain '내보냈어요'; got: {after}"
    );
}

#[test]
fn enqueue_cookie_refresh_appends_login_start_to_activity_feed() {
    // Point LOCALAPPDATA at a temp dir to avoid touching real user data.
    // Hold the process-wide lock so this test doesn't race with other tests
    // that also mutate LOCALAPPDATA.
    let _guard = LOCALAPPDATA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let data = tempfile::tempdir().expect("temp LOCALAPPDATA");
    std::env::set_var("LOCALAPPDATA", data.path());

    let (app, _dir) = mock_app();
    let wv = main_webview(&app);

    // Bootstrap the runtime so account dirs exist (enqueue relies on them).
    invoke_ok(&wv, "bootstrap_runtime", json!({}));

    // Activity feed starts empty.
    let before = invoke_ok(&wv, "list_activity", json!({}));
    assert_eq!(array(&before).len(), 0, "activity should start empty");

    // Enqueue a login for one fake account. The background worker will try and
    // fail a real browser login, but we don't await it — we only care that the
    // "로그인 시작" activity row was appended synchronously by the command itself.
    invoke_ok(
        &wv,
        "enqueue_cookie_refresh",
        json!({ "accountIds": ["fake_login"], "headless": true, "useAdb": false }),
    );

    // Check for "로그인 시작" in the activity feed. The worker may append more
    // rows asynchronously, so we assert CONTAINS rather than exact count.
    let after = invoke_ok(&wv, "list_activity", json!({}));
    let found = array(&after)
        .iter()
        .any(|item| item["text"].as_str().unwrap_or("").contains("로그인 시작"));
    assert!(
        found,
        "activity feed should contain '로그인 시작'; got: {after}"
    );

    std::env::remove_var("LOCALAPPDATA");
}
