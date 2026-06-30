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

    // 게시 전 네이버페이 약관 동의(=네이버페이 금융서비스 가입) 화면이 떠 있으면 자동으로 통과한다.
    // 패킷 분석 결과 "동의하기"는 사실 체크박스가 아니라 가입 URL로 가는 GET 리다이렉트 체인이다
    // (join?...consent=N → 302 → 약관동의 termcd=40 → 콜백 → 가입 완료). 그래서 ⒜ 먼저 그 가입
    // URL로 직접 이동해 브라우저가 체인을 따라가게 하고(견고·DOM 무관, 선택 동의는 모두 N으로 거절),
    // ⒝ 그래도 약관 페이지에 남아 있으면 기존 체크박스 동의 로직을 폴백으로 시도한다(기존 코드 보존).
    // 약관 페이지가 아니면 즉시 통과해 매 게시에 불필요한 대기를 넣지 않는다.
    pub(super) fn handle_npay_agreement_if_present(&mut self) -> AutomationResult<bool> {
        if !self.is_npay_agreement_page()? {
            return Ok(false);
        }

        // ⒜ 주 경로: 가입(약관동의) URL로 직접 이동. 선택 동의(마케팅/마이데이터/머니스토리)는 N으로
        // 거절하고, 성공 시 토론 페이지로·실패 시 약관 페이지로 되돌아오게 한다(실패면 아래 폴백이 탄다).
        const FINANCIAL_JOIN_URL: &str = "https://member-web.pay.naver.com/financial-service/join?from_pc=Y&nf_personalized_service_consent=N&naver_personalized_service_consent=N&optional_ads_and_mydata_usage_consent=N&moneystory_subscription_consent=N&join_success_url=https://stock.naver.com/discussion&join_fail_url=https://member.pay.naver.com/financial-member/agreement";
        self.navigate(FINANCIAL_JOIN_URL)?;
        self.wait_for_ready_state(Duration::from_secs(30)).ok();
        sleep(Duration::from_secs(2));
        if !self.is_npay_agreement_page()? {
            // 가입 완료(약관 페이지를 벗어남). 토론 페이지로 복귀하고 끝낸다.
            self.wait_for_ready_state(Duration::from_secs(10)).ok();
            if !self.current_url()?.contains("stock.naver.com/discussion") {
                self.navigate(DISCUSSION_URL)?;
            }
            return Ok(true);
        }

        // ⒝ 폴백: 가입 URL로 안 끝났으면(아직 약관 페이지) 기존 체크박스 동의 로직을 시도한다.
        let result = self.evaluate_string(
            r#"
                (async () => {
                  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
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
                  // 약관 폼 컨테이너를 느슨하게 잡는다(고정 id 의존 제거). 못 잡으면 document 전체.
                  const root =
                    document.querySelector('#__next') ||
                    document.querySelector('form') ||
                    document.body;
                  const esc = s => (window.CSS && CSS.escape ? CSS.escape(s) : s);
                  const labelFor = box =>
                    (box.id && root.querySelector('label[for="' + esc(box.id) + '"]'))
                    || box.closest('label');
                  const labelText = box => {
                    const row = box.closest('li, div');
                    return (text(labelFor(box)) + ' ' + text(row)).trim();
                  };
                  const scanBoxes = () =>
                    [...root.querySelectorAll('input[type="checkbox"]')].filter(visible);

                  // 컨테이너 안의 모든 체크박스를 1급 시민으로 다룬다(하드코딩 id/xpath 제거).
                  let boxes = scanBoxes();

                  // 전체동의 마스터: '모두/전체 동의' 라벨이 붙은 체크박스, 없으면 문서순 첫 체크박스.
                  const master =
                    boxes.find(b => /(약관\s*)?(모두|전체)\s*동의/.test(labelText(b))) || boxes[0];
                  if (master && !master.checked) {
                    ensureChecked(master, labelFor(master));
                    clicked.push('master:click');
                    await sleep(400);
                    boxes = scanBoxes(); // 마스터가 자식 항목을 켰을 수 있으니 다시 읽는다.
                  }

                  // 노출된 체크박스를 전부 checked 보장 — label이 없어도 input을 직접 처리한다
                  // (기존 'label 못 찾으면 체크 스킵' 결함을 해소: input이 살아있으면 무조건 켠다).
                  for (const box of boxes) {
                    if (box.checked) { clicked.push((box.id || 'box') + ':already'); continue; }
                    ensureChecked(box, labelFor(box));
                    clicked.push((box.id || 'box') + (box.checked ? ':checked' : ':try'));
                    await sleep(150);
                  }

                  await sleep(400);

                  // 동의 버튼 탐색. '동의하기'(정식 동의 버튼)를 최우선으로, 그다음 '동의' 포함(단
                  // '모두/전체 동의'는 제외), 그다음 폼 submit 버튼까지만 본다. 못 찾으면 "마지막
                  // 버튼" 같은 추측 클릭을 하지 않고 에러+스냅샷으로 끝낸다(취소/닫기 등 오클릭 방지).
                  const buttons =
                    [...root.querySelectorAll('button, input[type="submit"]')].filter(visible);
                  const btnText = b => (text(b) || b.value || '').trim();
                  // 실패 진단용 DOM 스냅샷(어떤 체크박스/버튼이 있었는지). picked는 고른 버튼(없으면 null).
                  const snapshot = picked => JSON.stringify({
                    checkboxCount: boxes.length,
                    checkboxes: scanBoxes().map(b => ({
                      id: b.id || null,
                      name: b.name || null,
                      checked: b.checked,
                      label: labelText(b).slice(0, 40)
                    })),
                    buttons: buttons.map(b => ({
                      text: btnText(b).slice(0, 30),
                      disabled: b.disabled,
                      type: b.type || null
                    })),
                    picked: picked ? btnText(picked).slice(0, 30) : null
                  });
                  const agreeBtn =
                    buttons.find(b => btnText(b).includes('동의하기')) ||
                    buttons.find(b => btnText(b).includes('동의') && !/(모두|전체)\s*동의/.test(btnText(b))) ||
                    buttons.find(b => b.type === 'submit') ||
                    null;

                  if (!agreeBtn) {
                    return JSON.stringify({
                      ok: false,
                      error: '동의 버튼을 찾지 못했습니다. snapshot=' + snapshot(null),
                      clicked
                    });
                  }

                  agreeBtn.scrollIntoView({ block: 'center', inline: 'center' });

                  // 버튼이 활성화될 때까지 최대 6초 폴링하며, 매 회 미체크 체크박스를 재시도한다.
                  const enabled = await waitUntil(() => {
                    if (!agreeBtn.disabled) return true;
                    for (const box of scanBoxes()) {
                      if (!box.checked) ensureChecked(box, labelFor(box));
                    }
                    return !agreeBtn.disabled;
                  }, 6000, 300);

                  if (!enabled) {
                    return JSON.stringify({
                      ok: false,
                      error: '동의 버튼이 활성화되지 않았습니다. snapshot=' + snapshot(agreeBtn),
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
