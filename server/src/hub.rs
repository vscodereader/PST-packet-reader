//! SSE 허브(§3·§4-1). device_id → 살아있는 스트림 송신핸들(라우팅 표) + Admin 브로드캐스트.
use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::broadcast;
use uuid::Uuid;

pub struct Hub {
    admin: broadcast::Sender<String>,
    devices: Mutex<HashMap<Uuid, broadcast::Sender<String>>>,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Self {
        let (admin, _) = broadcast::channel(512);
        Hub {
            admin,
            devices: Mutex::new(HashMap::new()),
        }
    }

    /// Admin SSE 구독(`GET /admin/stream`).
    pub fn admin_subscribe(&self) -> broadcast::Receiver<String> {
        self.admin.subscribe()
    }
    /// 모든 Admin 스트림으로 이벤트 push(상태·결과·거부 등).
    pub fn admin_push(&self, msg: String) {
        let _ = self.admin.send(msg);
    }

    /// 하위 SSE 구독(`GET /agent/stream`). 재연결 시 같은 device_id 채널에 다시 붙는다(re-attach, §4-1).
    pub fn device_subscribe(&self, id: Uuid) -> broadcast::Receiver<String> {
        let mut g = self.devices.lock().unwrap();
        let tx = g
            .entry(id)
            .or_insert_with(|| broadcast::channel(128).0)
            .clone();
        tx.subscribe()
    }
    /// 대상 기기 SSE로 명령 push. 구독 중인 스트림이 있으면 true(연결됨).
    pub fn device_push(&self, id: Uuid, msg: String) -> bool {
        let g = self.devices.lock().unwrap();
        match g.get(&id) {
            Some(tx) => tx.receiver_count() > 0 && tx.send(msg).is_ok(),
            None => false,
        }
    }
}
