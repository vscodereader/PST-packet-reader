use serde_json::json;

use super::types::NaverLoginProfile;
use super::{AutomationError, AutomationResult, CdpClient};

const GET_PROFILE_METHOD: &str = "GET";
const GET_PROFILE_AUTHORITY: &str = "static.nid.naver.com";
const GET_PROFILE_SCHEME: &str = "https";
const GET_PROFILE_PATH: &str = "/getProfile";
const GET_PROFILE_SERVICE: &str = "my";

#[derive(Debug, Clone)]
pub(super) struct PacketSignature {
    pub method: &'static str,
    pub authority: &'static str,
    pub scheme: &'static str,
    pub path: &'static str,
    pub query: &'static str,
    pub response_type: &'static str,
}

pub(super) const GET_PROFILE_PACKET: PacketSignature = PacketSignature {
    method: GET_PROFILE_METHOD,
    authority: GET_PROFILE_AUTHORITY,
    scheme: GET_PROFILE_SCHEME,
    path: GET_PROFILE_PATH,
    query: "svc=my&callback=<jsonp callback>",
    response_type: "application/x-javascript",
};

impl CdpClient {
    // Wireshark/F12에서 확인한 getProfile 패킷을 재현해 로그인 프로필을 확인하는 함수입니다.
    pub(super) fn read_login_profile_from_packet(&mut self) -> AutomationResult<NaverLoginProfile> {
        let raw_result = self.evaluate_string(&build_get_profile_script())?;
        serde_json::from_str(&raw_result).map_err(AutomationError::from)
    }
}

// Chrome 탭 안에서 JSONP getProfile 요청을 실행하는 JavaScript를 만드는 함수입니다.
fn build_get_profile_script() -> String {
    let packet = json!({
        "method": GET_PROFILE_PACKET.method,
        "authority": GET_PROFILE_PACKET.authority,
        "scheme": GET_PROFILE_PACKET.scheme,
        "path": GET_PROFILE_PACKET.path,
        "query": GET_PROFILE_PACKET.query,
        "responseType": GET_PROFILE_PACKET.response_type,
    });

    format!(
        r#"
        (async () => {{
          const packet = {packet};
          const callbackName = `pstmacroProfile_${{Date.now()}}_${{Math.floor(Math.random() * 1000000)}}`;
          const url = `${{packet.scheme}}://${{packet.authority}}${{packet.path}}?svc={GET_PROFILE_SERVICE}&callback=${{encodeURIComponent(callbackName)}}`;

          return await new Promise(resolve => {{
            const script = document.createElement('script');
            const cleanup = () => {{
              window[callbackName] = undefined;
              try {{
                delete window[callbackName];
              }} catch (_) {{}}
              script.remove();
            }};
            const finish = data => {{
              cleanup();
              resolve(JSON.stringify(data));
            }};
            const timer = setTimeout(() => {{
              finish({{
                logged_in: false,
                nickname: null,
                image_url: null,
                message: 'getProfile timeout'
              }});
            }}, 5000);

            window[callbackName] = data => {{
              clearTimeout(timer);
              finish({{
                logged_in: data?.rtn_cd === '0',
                nickname: data?.nick_name || null,
                image_url: data?.image_url || null,
                message: data?.rtn_msg || data?.rtn_cd || 'unknown'
              }});
            }};

            script.onerror = () => {{
              clearTimeout(timer);
              finish({{
                logged_in: false,
                nickname: null,
                image_url: null,
                message: 'getProfile request failed'
              }});
            }};
            script.src = url;
            document.head.appendChild(script);
          }});
        }})()
        "#
    )
}
