//! 에이전트 로컬 설정(서버 주소 + 기기토큰) 영속화. 기존 앱 데이터 루트(쿠키·계정과 같은 위치)에
//! `agent.json`으로 저장한다. 사람이 입력하는 건 서버 주소 + 기기코드 둘뿐이고(§6-2), 기기토큰은
//! 등록 성공 시 서버가 내려준 값을 자동 저장한다.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Admin 서버 고정 주소(§4-1). 예: http://123.45.67.89:8080
    pub server_url: String,
    /// 등록 성공 시 서버가 발급한 장기 기기토큰(JWT, 만료 없음, §6).
    pub device_token: String,
    /// 서버가 붙인 기기 이름(표시용).
    #[serde(default)]
    pub device_name: String,
}

fn config_path() -> Result<PathBuf, String> {
    // 기존 헬퍼 재사용(auth가 재노출): 쿠키/계정과 같은 앱 데이터 루트(§7).
    let root = crate::auth::app_data_root().map_err(|e| e.to_string())?;
    Ok(root.join("agent.json"))
}

/// 저장된 설정을 읽는다(없으면 None).
pub fn load() -> Option<AgentConfig> {
    let path = config_path().ok()?;
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// 설정을 저장한다(앱 데이터 루트 생성 포함).
pub fn save(cfg: &AgentConfig) -> Result<(), String> {
    let path = config_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| e.to_string())
}

/// 설정을 지운다(등록 해제). 파일이 없어도 성공.
pub fn clear() -> Result<(), String> {
    if let Ok(path) = config_path() {
        let _ = fs::remove_file(path);
    }
    Ok(())
}
