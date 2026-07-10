use std::thread::sleep;
use std::time::{Duration, Instant};

use super::{AutomationResult, CdpClient};

impl CdpClient {
    // 네이버 로그인 후 기기 등록 안내가 나오면 "등록안함"을 클릭하는 함수입니다.
    // 로그인 흐름(auth::login_flow)에서도 재사용하므로 crate 범위로 공개한다.
    pub(crate) fn click_device_dontsave_if_present(
        &mut self,
        timeout: Duration,
    ) -> AutomationResult<bool> {
        let end = Instant::now() + timeout;

        while Instant::now() < end {
            if self.evaluate_bool(
                r#"
                (() => {
                  const visible = el => {
                    if (!el) return false;
                    const r = el.getBoundingClientRect();
                    const s = getComputedStyle(el);
                    return r.width > 0
                      && r.height > 0
                      && s.display !== 'none'
                      && s.visibility !== 'hidden'
                      && !el.disabled;
                  };
                  const text = el => String(el?.innerText || el?.textContent || '')
                    .replace(/\s+/g, ' ')
                    .trim();
                  const byXpath = xpath => document.evaluate(
                    xpath,
                    document,
                    null,
                    XPathResult.FIRST_ORDERED_NODE_TYPE,
                    null
                  ).singleNodeValue;
                  const candidates = [
                    document.querySelector('#new\\.dontsave'),
                    byXpath('//*[@id="new.dontsave"]'),
                    ...[...document.querySelectorAll('a, button')].filter(el =>
                      text(el).includes('등록안함') || text(el).includes('등록 안함')
                    )
                  ].filter(visible);

                  if (!candidates.length) return false;

                  candidates[0].scrollIntoView({ block: 'center', inline: 'center' });
                  candidates[0].click();
                  return true;
                })()
                "#,
            )? {
                // "등록 안함" 클릭이 반영될 짧은 여유. 로그인 체감 속도(#14)를 위해 1초로 줄인다.
                sleep(Duration::from_secs(1));
                return Ok(true);
            }

            sleep(Duration::from_millis(200));
        }

        Ok(false)
    }
}
