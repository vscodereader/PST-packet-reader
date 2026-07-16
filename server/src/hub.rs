//! SSE 허브(§3·§4-1). device_id → 살아있는 스트림 송신핸들(라우팅 표) + Admin 브로드캐스트.
use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::broadcast;
use uuid::Uuid;

/// 한 기기에 SSE 미연결 동안 쌓아둘 수 있는 최대 대기 명령 수. 초과하면 가장 오래된 것부터 버린다
/// (재연결 안 하는 죽은 기기의 무한 증가 방지). 계정 분배 몇 건 수준이라 넉넉하다.
const MAX_PENDING: usize = 128;

pub struct Hub {
    admin: broadcast::Sender<String>,
    devices: Mutex<HashMap<Uuid, broadcast::Sender<String>>>,
    /// SSE가 그 순간 미연결이라 즉시 전달 못한 명령을 device_id별로 쌓아둔다. 재연결(구독) 시 flush.
    /// 모바일 CGNAT로 SSE 장수명 연결이 자주 끊겨도 분배 명령이 유실되지 않게 한다(2026-07-15).
    pending: Mutex<HashMap<Uuid, Vec<String>>>,
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
            pending: Mutex::new(HashMap::new()),
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
    /// 구독 직후, 미연결 동안 쌓인 대기 명령(pending)을 이 채널로 flush 한다 → 방금 붙은 스트림이 받는다.
    pub fn device_subscribe(&self, id: Uuid) -> broadcast::Receiver<String> {
        let tx = {
            let mut g = self.devices.lock().unwrap();
            g.entry(id)
                .or_insert_with(|| broadcast::channel(128).0)
                .clone()
        };
        // subscribe를 먼저 해야 이어서 send하는 pending 명령을 이 수신자가 받는다(broadcast는 현재
        // 구독자에게만 전달됨).
        let rx = tx.subscribe();
        let queued = {
            let mut p = self.pending.lock().unwrap();
            p.remove(&id).unwrap_or_default()
        };
        for msg in queued {
            let _ = tx.send(msg);
        }
        rx
    }
    /// 대상 기기 SSE로 명령 push. 구독 중인 스트림이 있으면 즉시 전달하고 true. 없으면(미연결) 버리지
    /// 않고 pending 큐에 쌓아 두고 false를 돌려준다 — 재연결(device_subscribe) 시 flush 되어 전달된다.
    pub fn device_push(&self, id: Uuid, msg: String) -> bool {
        let delivered = {
            let g = self.devices.lock().unwrap();
            match g.get(&id) {
                Some(tx) if tx.receiver_count() > 0 => tx.send(msg.clone()).is_ok(),
                _ => false,
            }
        };
        if !delivered {
            let mut p = self.pending.lock().unwrap();
            let q = p.entry(id).or_default();
            q.push(msg);
            // 죽은 기기의 무한 증가 방지: 상한 초과 시 가장 오래된 것부터 버린다.
            if q.len() > MAX_PENDING {
                let overflow = q.len() - MAX_PENDING;
                q.drain(0..overflow);
            }
        }
        delivered
    }

    /// 대상 기기의 대기 명령(pending)을 꺼내 비운다(하트비트 pull 경로용, 2026-07-16). SSE가 IP 회전으로
    /// 계속 끊겨 device_subscribe flush가 못 도는 기기(모바일 CGNAT)를 위해, 하트비트 응답에 실어 확실히
    /// 전달한다. device_subscribe(SSE flush)와 **같은 pending 뮤텍스**를 공유하므로, 큐에 쌓인 명령은
    /// 둘 중 먼저 꺼낸 경로로만 전달된다(이중 전달 없음). 에이전트는 commandId 중복까지 무시해 안전하다.
    pub fn drain_pending(&self, id: Uuid) -> Vec<String> {
        let mut p = self.pending.lock().unwrap();
        p.remove(&id).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 미연결 기기에 push하면 버리지 않고 큐에 쌓였다가, 재연결(subscribe) 시 그 스트림으로 전달된다.
    #[tokio::test]
    async fn queues_command_while_offline_and_flushes_on_reconnect() {
        let hub = Hub::new();
        let id = Uuid::new_v4();
        // 구독자 없음 → 즉시 전달 실패(false)지만 큐에 보관.
        assert!(!hub.device_push(id, "cmd1".into()));
        assert!(!hub.device_push(id, "cmd2".into()));
        // 재연결(구독) → 쌓인 명령이 flush 되어 순서대로 도착.
        let mut rx = hub.device_subscribe(id);
        assert_eq!(rx.recv().await.unwrap(), "cmd1");
        assert_eq!(rx.recv().await.unwrap(), "cmd2");
    }

    // 연결된 기기에는 즉시 전달(true)되고 큐에 남지 않는다.
    #[tokio::test]
    async fn delivers_immediately_when_connected() {
        let hub = Hub::new();
        let id = Uuid::new_v4();
        let mut rx = hub.device_subscribe(id);
        assert!(hub.device_push(id, "now".into()));
        assert_eq!(rx.recv().await.unwrap(), "now");
    }
}
