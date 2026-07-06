//! 실행 중 게시큐 "완전 종료(kill)"용 취소 신호 레지스트리.
//! 설계: `docs/Admin UI/08-게시큐-완전종료.md`.
//!
//! 지금까지 실행 중 아이템을 중간에 끊을 방법이 없었다 — 취소(`cancel_queue_now`)는 디스크
//! 큐에서 항목만 지우고, 워커는 그룹/배치 경계에서만(`item_present`) 협조적으로 확인했다.
//! 이 모듈은 큐 id별 취소 신호(`CancelSignal`)를 in-memory로 보관해, 게시 루프가 **종목
//! 사이·대기 중**에도 이 신호를 보고 스스로 멈추게 한다. 신호는 "멈춰달라는 표시"일 뿐이고
//! 실제 정지는 루프가 안전 경계에서 확인 후 스스로 빠져나오며 하므로, Chrome 종료는 기존
//! `ChromeHandle::drop`(Windows `taskkill /T`) 정상 경로를 그대로 타 **고아 프로세스가 남지
//! 않는다**(강제로 죽이지 않는 것이 캡차 방지의 핵심 — 설계서 §3).
//!
//! Stage2(강제 taskkill 에스컬레이션)용 `in_critical`·`chrome_pids`는 정의만 두고 후속
//! 단계에서 배선한다.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// 큐 아이템 1개의 취소 신호. Stage1(협조적 정지)은 `flag`만 쓴다.
#[derive(Default)]
pub struct CancelSignal {
    /// 협조적 취소 요청됨. 게시 루프가 안전 경계(종목 사이·대기 중)에서 확인하고 멈춘다.
    flag: AtomicBool,
    /// (Stage2) add POST 임계구역 — 강제 kill을 이 구간에선 응답까지 보류한다.
    in_critical: AtomicBool,
    /// (Stage2) 이 큐가 띄운 Chrome 메인 PID들 — hang 시 직접 taskkill 대상.
    chrome_pids: Mutex<Vec<u32>>,
}

impl CancelSignal {
    /// 취소가 요청됐는지.
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
    /// 취소를 요청한다(멈춰달라고 표시만 — 실제 정지는 루프가 확인 후 스스로).
    pub fn request(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }
    /// (Stage2) add POST 임계구역 진입 — 강제 kill 보류.
    pub fn enter_critical(&self) {
        self.in_critical.store(true, Ordering::SeqCst);
    }
    /// (Stage2) add POST 임계구역 해제.
    pub fn leave_critical(&self) {
        self.in_critical.store(false, Ordering::SeqCst);
    }
    /// (Stage2) 지금 add POST 임계구역인지.
    pub fn in_critical(&self) -> bool {
        self.in_critical.load(Ordering::SeqCst)
    }
    /// (Stage2) 이 큐가 띄운 Chrome 메인 PID 등록.
    pub fn register_pid(&self, pid: u32) {
        if let Ok(mut v) = self.chrome_pids.lock() {
            v.push(pid);
        }
    }
    /// (Stage2) 등록된 Chrome PID들을 꺼낸다(강제 taskkill용).
    pub fn take_pids(&self) -> Vec<u32> {
        self.chrome_pids
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }
}

/// 큐 id → 취소 신호. Tauri managed state로 앱 전역에서 `app.state()`로 조회한다 —
/// `execute_item`/`run_forum_targets`가 runner를 안 받고 `app`만 받으므로, 신호를
/// `RunnerInner`가 아니라 이 레지스트리에 둬 어느 계층에서든 id로 찾게 한다.
#[derive(Default)]
pub struct CancelRegistry {
    map: Mutex<HashMap<String, Arc<CancelSignal>>>,
}

impl CancelRegistry {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<CancelSignal>>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// id의 신호를 가져오거나(없으면) 새로 만들어 등록하고 Arc를 돌려준다. 아이템이 **실행을
    /// 시작할 때**(예: `run_forum_targets` 진입) 호출해 신호를 등록한다.
    pub fn get_or_create(&self, id: &str) -> Arc<CancelSignal> {
        self.lock().entry(id.to_owned()).or_default().clone()
    }

    /// id의 신호를 조회한다(없으면 None).
    pub fn get(&self, id: &str) -> Option<Arc<CancelSignal>> {
        self.lock().get(id).cloned()
    }

    /// id에 취소를 요청한다. 신호가 **이미 등록돼 있을 때만** 표시한다 — 아직 등록 전(막
    /// 시작)이면 flag는 놓치지만, `kill_queue_now`가 큐에서 항목도 제거하므로 배치 경계의
    /// `item_present`가 협조적 취소를 잡는다(설계서 §5, 둘의 상호보완). 등록되지 않은 id에
    /// 빈 신호를 만들어 두면 map이 새는 것을 막는다.
    pub fn request(&self, id: &str) {
        if let Some(sig) = self.get(id) {
            sig.request();
        }
    }

    /// 아이템 종료 시 신호를 제거해 map이 무한정 커지지 않게 한다(없으면 무해한 no-op).
    pub fn remove(&self, id: &str) {
        self.lock().remove(id);
    }
}

/// 큐 아이템 1개를 완전 종료(kill)한다 — 로컬 UI(`kill_queue_now`)와 Admin 원격(agent
/// `kill_publish`)이 공유하는 **단일 경로**(설계서 08 §5, "kill primitive 1개가 두 트리거를
/// 커버"). ① 취소 신호 set(실행 중 루프가 종목 사이·대기 중에 스스로 정지) ② 큐에서 제거
/// (배치 경계 협조 취소 + 워커 재실행 방지) ③ Stage2 강제 감시 spawn(hang 대비).
pub fn kill_one<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: &str) {
    use tauri::Manager;
    let cancels = app.state::<CancelRegistry>();
    cancels.request(id);
    app.state::<crate::store::JsonStore<crate::ipc::queue::QueueNowItem>>()
        .mutate(|items| crate::ipc::queue::apply_cancel_now(items, id));
    tracing::info!(id = %id, "[QUEUE] 중지(kill) — 취소 신호 set + 큐에서 제거");
    if let Some(sig) = cancels.get(id) {
        let app_bg = app.clone();
        let id_bg = id.to_owned();
        tauri::async_runtime::spawn(async move {
            escalate_kill(app_bg, id_bg, sig).await;
        });
    }
}

/// Stage2 강제 종료 감시(설계서 08 §4-3). 협조적 정지(Stage1)가 `GRACE_SECS` 안에 끝나면
/// (신호가 레지스트리에서 제거됨 = 아이템 정상 종료) 아무것도 안 한다. 안 끝나면(hang) 등록된
/// Chrome PID를 `taskkill /T`로 강제 종료한다 — 단 add POST 임계구역(`in_critical`)이면 그 창이
/// 풀릴 때까지(상한 `CRIT_MAX_SECS`) 기다린 뒤 kill해 "글은 올라갔는데 기록 못 함=이중게시"를
/// 막는다. `kill_queue_now`가 백그라운드 태스크로 띄운다.
pub async fn escalate_kill<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
    sig: Arc<CancelSignal>,
) {
    use tauri::Manager;
    const GRACE_SECS: u64 = 10;
    const CRIT_MAX_SECS: u64 = 30;

    // 신호가 레지스트리에서 사라졌으면(finish_item이 제거) 아이템이 정상 종료된 것 → 강제 불필요.
    let finished = |app: &tauri::AppHandle<R>| app.state::<CancelRegistry>().get(&id).is_none();

    // ① 협조 정지 유예: 매 초 정상 종료됐는지 확인.
    for _ in 0..GRACE_SECS {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if finished(&app) {
            return;
        }
    }
    // ② 아직 안 끝남 → 강제. 단 add POST 임계구역이면 풀릴 때까지(상한) 대기.
    let mut waited = 0u64;
    while sig.in_critical() && waited < CRIT_MAX_SECS {
        if finished(&app) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        waited += 1;
    }
    if finished(&app) {
        return;
    }
    // ③ 등록된 Chrome PID를 강제 종료(프로세스 트리 taskkill). PID가 없으면(카페/밴드=HTTP
    //    이거나 아직 Chrome 미기동) 협조 정지에 의존한다.
    let pids = sig.take_pids();
    if pids.is_empty() {
        tracing::warn!(
            id = %id,
            "[QUEUE] 강제 종료 감시: 등록된 Chrome PID 없음 — 협조 정지에 의존(카페/밴드 HTTP이거나 Chrome 미기동)"
        );
        return;
    }
    tracing::warn!(
        id = %id,
        count = pids.len(),
        "[QUEUE] 협조 정지 타임아웃 — Chrome 강제 종료(taskkill 트리) 에스컬레이션"
    );
    for pid in pids {
        crate::auth::force_kill_tree(pid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_starts_uncancelled_then_flips_on_request() {
        let sig = CancelSignal::default();
        assert!(!sig.is_cancelled());
        sig.request();
        assert!(sig.is_cancelled());
    }

    #[test]
    fn request_reaches_the_running_loops_shared_arc() {
        // run_forum_targets가 get_or_create로 잡은 Arc와 kill_queue_now의 request가
        // 같은 신호를 가리켜, 실행 중 루프가 취소를 본다.
        let reg = CancelRegistry::default();
        let running = reg.get_or_create("q1"); // 루프가 들고 있는 클론
        assert!(!running.is_cancelled());
        reg.request("q1"); // kill 명령
        assert!(running.is_cancelled());
    }

    #[test]
    fn request_before_registration_is_dropped_but_leaks_nothing() {
        // 등록 전 취소는 flag를 놓치지만(코스 경계 item_present가 커버), 빈 신호를 남기지 않는다.
        let reg = CancelRegistry::default();
        reg.request("ghost");
        assert!(reg.get("ghost").is_none());
    }

    #[test]
    fn remove_clears_the_signal() {
        let reg = CancelRegistry::default();
        let _ = reg.get_or_create("q2");
        assert!(reg.get("q2").is_some());
        reg.remove("q2");
        assert!(reg.get("q2").is_none());
    }

    #[test]
    fn critical_section_and_pid_registry_roundtrip() {
        let sig = CancelSignal::default();
        assert!(!sig.in_critical());
        sig.enter_critical();
        assert!(sig.in_critical());
        sig.leave_critical();
        assert!(!sig.in_critical());
        sig.register_pid(1234);
        sig.register_pid(5678);
        assert_eq!(sig.take_pids(), vec![1234, 5678]);
        assert!(sig.take_pids().is_empty(), "take는 비운다");
    }
}
