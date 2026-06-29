use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{AutomationError, AutomationResult, CdpClient, DISCUSSION_URL};

impl CdpClient {
    // 화면에서 지정한 문구가 들어간 버튼이나 링크를 찾아 클릭하는 함수입니다.
    pub(super) fn click_text(&mut self, text: &str, timeout: Duration) -> AutomationResult<()> {
        let quoted = serde_json::to_string(text)?;
        let end = Instant::now() + timeout;

        while Instant::now() < end {
            let expression = format!(
                r#"
                (() => {{
                  const needle = {quoted};
                  const visible = el => {{
                    if (!el) return false;
                    const r = el.getBoundingClientRect();
                    const s = getComputedStyle(el);
                    return r.width > 0
                      && r.height > 0
                      && s.display !== 'none'
                      && s.visibility !== 'hidden'
                      && !el.disabled;
                  }};
                  const text = el => String(el?.innerText || el?.textContent || '')
                    .replace(/\s+/g, ' ')
                    .trim();

                  const candidates = [...document.querySelectorAll('button, a, [role="button"]')]
                    .filter(visible)
                    .filter(el => text(el).includes(needle));

                  if (!candidates.length) return false;

                  const target = candidates[0];
                  target.scrollIntoView({{ block: 'center', inline: 'center' }});
                  target.click();
                  return true;
                }})()
                "#
            );

            if self.evaluate_bool(&expression)? {
                sleep(Duration::from_millis(500));
                return Ok(());
            }

            sleep(Duration::from_millis(300));
        }

        Err(AutomationError::new(format!(
            "'{text}' 버튼을 찾지 못했습니다."
        )))
    }

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

    // 현재 화면이 네이버페이 약관 동의 페이지인지 확인하는 함수입니다.
    pub(super) fn is_npay_agreement_page(&mut self) -> AutomationResult<bool> {
        self.evaluate_bool(
            r#"
            (() => {
              const body = document.body?.innerText || '';
              return location.href.includes('member.pay.naver.com/financial-member/agreement')
                || body.includes('서비스 이용을 위해')
                || body.includes('약관에 동의해 주세요');
            })()
            "#,
        )
    }

    // 네이버페이 약관 동의 화면이 떠 있으면 자동으로 동의 처리한다(약관 모두 동의 → 필수
    // 항목 체크 → 동의하기). 종토방 게시 전 로그인 직후 이 화면이 떠 있으면 막지 않고
    // 자동 진행한다. 약관 페이지가 아니면 즉시 통과해 매 게시에 불필요한 대기를 넣지 않는다.
    pub(super) fn handle_npay_agreement_if_present(&mut self) -> AutomationResult<bool> {
        if !self.is_npay_agreement_page()? {
            return Ok(false);
        }

        // 약관 페이지면 1회 자동 동의 처리하고 결과로 빠져나온다.
        let result = self.evaluate_string(
            r#"
                (async () => {
                  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
                  const requiredItems = [
                    {
                      id: 'service',
                      labelSelector: 'label[for="service"]',
                      fallbackXpath: '//*[@id="__next"]/div/div/ul[1]/li/label'
                    },
                    {
                      id: 'privateNaver',
                      labelSelector: 'label[for="privateNaver"]',
                      fallbackXpath: '//*[@id="__next"]/div/div/ul[2]/li/label'
                    },
                    {
                      id: 'privateNF',
                      labelSelector: 'label[for="privateNF"]',
                      fallbackXpath: '//*[@id="__next"]/div/div/ul[3]/li/label'
                    }
                  ];
                  const visible = el => {
                    if (!el) return false;
                    const r = el.getBoundingClientRect();
                    const s = getComputedStyle(el);
                    return r.width > 0
                      && r.height > 0
                      && s.display !== 'none'
                      && s.visibility !== 'hidden';
                  };
                  const text = el => String(el?.innerText || el?.textContent || '');
                  const getByXpath = xpath => document.evaluate(
                    xpath,
                    document,
                    null,
                    XPathResult.FIRST_ORDERED_NODE_TYPE,
                    null
                  ).singleNodeValue;
                  const clickEl = el => {
                    el.scrollIntoView({ block: 'center', inline: 'center' });
                    el.click();
                  };
                  // 라벨 클릭이 React 컨트롤드 체크박스에 안 먹는 경우가 있어, input.checked를
                  // 직접 확인하고 안 되면 input 클릭 + change/input 이벤트까지 디스패치한다.
                  const ensureChecked = (input, label) => {
                    if (label) clickEl(label);
                    if (!input) return;
                    if (!input.checked) {
                      try { input.click(); } catch (e) {}
                    }
                    if (!input.checked) {
                      try {
                        input.checked = true;
                        input.dispatchEvent(new Event('input', { bubbles: true }));
                        input.dispatchEvent(new Event('change', { bubbles: true }));
                      } catch (e) {}
                    }
                  };
                  // 고정 sleep 대신 조건이 참이 될 때까지 폴링한다(버튼 활성화가 늦게
                  // 반영돼도 잡도록). pred는 매 회 재시도 로직을 겸할 수 있다.
                  const waitUntil = async (pred, timeoutMs, stepMs) => {
                    const end = Date.now() + timeoutMs;
                    while (Date.now() < end) {
                      if (pred()) return true;
                      await sleep(stepMs);
                    }
                    return pred();
                  };
                  const clicked = [];
                  const allAgree = [...document.querySelectorAll('label, button, div, span')]
                    .find(el => visible(el) && text(el).includes('약관 모두 동의하기'));

                  if (allAgree) {
                    clickEl(allAgree);
                    clicked.push('all-agree:clicked');
                    await sleep(600);
                  }

                  for (const item of requiredItems) {
                    const input = document.getElementById(item.id);
                    const label =
                      document.querySelector(item.labelSelector)
                      || getByXpath(item.fallbackXpath);

                    if (!label) {
                      clicked.push(item.id + ':label-not-found');
                      continue;
                    }

                    if (input && input.checked) {
                      clicked.push(item.id + ':already-checked');
                      continue;
                    }

                    ensureChecked(input, label);
                    clicked.push(
                      item.id + (input && input.checked ? ':checked' : ':clicked')
                    );
                    await sleep(300);
                  }

                  await sleep(600);

                  let agreeBtn = [...document.querySelectorAll('button')]
                    .find(btn => visible(btn) && text(btn).includes('동의하기'));

                  if (!agreeBtn) {
                    agreeBtn = getByXpath('//*[@id="__next"]/div/div/div[2]/div/button');
                  }

                  if (!agreeBtn) {
                    return JSON.stringify({
                      ok: false,
                      error: '동의하기 버튼을 찾지 못했습니다.',
                      clicked
                    });
                  }

                  agreeBtn.scrollIntoView({ block: 'center', inline: 'center' });

                  // 버튼이 활성화될 때까지 최대 6초 폴링하며, 매 회 미체크 필수항목을 재시도한다.
                  // (라벨 클릭이 한 번에 안 먹거나 활성화가 늦게 반영되는 경우 대비)
                  const enabled = await waitUntil(() => {
                    if (!agreeBtn.disabled) return true;
                    for (const item of requiredItems) {
                      const input = document.getElementById(item.id);
                      if (input && !input.checked) {
                        const label =
                          document.querySelector(item.labelSelector)
                          || getByXpath(item.fallbackXpath);
                        ensureChecked(input, label);
                      }
                    }
                    return !agreeBtn.disabled;
                  }, 6000, 300);

                  if (!enabled) {
                    const checkedState = requiredItems.map(it => {
                      const input = document.getElementById(it.id);
                      return it.id + '=' + (input ? (input.checked ? 'on' : 'off') : 'none');
                    });
                    return JSON.stringify({
                      ok: false,
                      error: '동의하기 버튼이 아직 비활성화 상태입니다. (필수항목 체크 상태: '
                        + checkedState.join(', ') + ')',
                      clicked
                    });
                  }

                  agreeBtn.click();
                  return JSON.stringify({ ok: true, clicked });
                })()
                "#,
        )?;

        let data: Value = serde_json::from_str(&result)?;

        if !data.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            return Err(AutomationError::new(
                data.get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("약관 동의 처리 실패"),
            ));
        }

        self.wait_for_ready_state(Duration::from_secs(10)).ok();
        sleep(Duration::from_secs(4));

        if !self.current_url()?.contains("stock.naver.com/discussion") {
            self.navigate(DISCUSSION_URL)?;
        }

        Ok(true)
    }

    // 자동화 시작 전에 네이버 증권 토론 메인 화면으로 이동시키는 함수입니다.
    pub(super) fn ensure_discussion_page(&mut self) -> AutomationResult<()> {
        self.click_device_dontsave_if_present(Duration::from_secs(2))?;
        self.handle_npay_agreement_if_present()?;

        if !self.current_url()?.contains("stock.naver.com/discussion")
            || self.is_npay_agreement_page()?
        {
            self.navigate(DISCUSSION_URL)?;
        }

        self.click_device_dontsave_if_present(Duration::from_secs(2))?;
        self.handle_npay_agreement_if_present()?;

        if !self.current_url()?.contains("stock.naver.com/discussion") {
            self.navigate(DISCUSSION_URL)?;
        }

        self.handle_npay_agreement_if_present()?;

        Ok(())
    }
}
