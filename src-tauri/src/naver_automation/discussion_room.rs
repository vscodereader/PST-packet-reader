use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::devtools_connection::string_field;
use super::types::DiscussionSelection;
use super::{AutomationError, AutomationResult, CdpClient, DISCUSSION_URL};

impl CdpClient {
    // 네이버 증권 토론 메인에서 랜덤 카테고리와 종목을 선택해 해당 종목 토론방으로 이동하는 함수입니다.
    pub(super) fn open_random_discussion_room(&mut self) -> AutomationResult<DiscussionSelection> {
        self.require_manual_npay_agreement_if_present()?;
        self.navigate(DISCUSSION_URL)?;
        self.require_manual_npay_agreement_if_present()?;

        if !self.current_url()?.contains("stock.naver.com/discussion")
            || self.is_npay_agreement_page()?
        {
            self.navigate(DISCUSSION_URL)?;
            self.require_manual_npay_agreement_if_present()?;
        }

        let labels = serde_json::to_string(&[
            "토론 급상승",
            "상승",
            "하락",
            "거래량",
            "거래대금",
            "외국인 순매수 상위",
            "기관 순매수 상위",
        ])?;

        let script = format!(
            r#"
            (async (categoryLabels) => {{
              const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
              const text = el => {{
                if (!el) return '';
                const value = el.innerText || el.textContent || '';
                return String(value).replace(/\s+/g, ' ').trim();
              }};
              const visible = el => {{
                if (!el) return false;
                const r = el.getBoundingClientRect();
                const s = getComputedStyle(el);
                return r.width > 0
                  && r.height > 0
                  && s.display !== 'none'
                  && s.visibility !== 'hidden'
                  && s.opacity !== '0';
              }};
              const clickFast = el => {{
                if (!el) return false;
                const target = el.closest('a,button,[role="button"],li') || el;
                target.scrollIntoView({{ block: 'center', inline: 'center' }});
                const rr = target.getBoundingClientRect();
                const x = Math.floor(rr.left + Math.min(Math.max(rr.width * 0.5, 20), Math.max(rr.width - 10, 20)));
                const y = Math.floor(rr.top + rr.height / 2);
                const pointTarget = document.elementFromPoint(x, y) || target;

                for (const type of ['mouseover', 'mousemove', 'mousedown', 'mouseup', 'click']) {{
                  pointTarget.dispatchEvent(new MouseEvent(type, {{
                    bubbles: true,
                    cancelable: true,
                    clientX: x,
                    clientY: y,
                    button: 0
                  }}));
                }}

                try {{
                  target.click();
                }} catch (_) {{}}

                return true;
              }};
              const shuffle = arr => {{
                const copy = arr.slice();
                for (let i = copy.length - 1; i > 0; i--) {{
                  const j = Math.floor(Math.random() * (i + 1));
                  [copy[i], copy[j]] = [copy[j], copy[i]];
                }}
                return copy;
              }};
              const findDiscussionSection = () => {{
                const candidates = Array.from(document.querySelectorAll('section, div'))
                  .filter(el => visible(el) && text(el).includes('오늘의 종목 토론 둘러보기'));

                candidates.sort((a, b) => {{
                  const ar = a.getBoundingClientRect();
                  const br = b.getBoundingClientRect();
                  const aScore = Math.abs(ar.top) + Math.abs(ar.left);
                  const bScore = Math.abs(br.top) + Math.abs(br.left);

                  if (aScore !== bScore) return aScore - bScore;

                  return text(a).length - text(b).length;
                }});

                return candidates[0] || null;
              }};
              const waitForDiscussionSection = async maxMs => {{
                const end = Date.now() + maxMs;

                while (Date.now() < end) {{
                  const section = findDiscussionSection();

                  if (section && text(section).includes('오늘의 종목 토론 둘러보기')) {{
                    return section;
                  }}

                  await sleep(50);
                }}

                return findDiscussionSection();
              }};
              const findCategoryButton = (section, label) => {{
                if (!section) return null;
                const controls = Array.from(section.querySelectorAll('button, a, [role="button"]'))
                  .filter(visible);

                return controls.find(el => text(el) === label)
                  || controls.find(el => text(el).includes(label))
                  || null;
              }};
              const findViewAllButton = () => {{
                const controls = Array.from(document.querySelectorAll('a, button, [role="button"]'))
                  .filter(visible);
                return controls.find(el => text(el) === '전체 토론글 보러가기')
                  || controls.find(el => text(el).includes('전체 토론글 보러가기'))
                  || controls.find(el => text(el).includes('전체 토론글'))
                  || null;
              }};
              const previewSignature = () => {{
                const btn = findViewAllButton();

                if (!btn) return '';

                let box = btn;

                for (let i = 0; i < 8 && box && box.parentElement; i++) {{
                  const t = text(box);

                  if (t.includes('전체 토론글') && t.length > 20) {{
                    return t.slice(0, 800);
                  }}

                  box = box.parentElement;
                }}

                return text(btn);
              }};
              const scrollRankArea = (section, position) => {{
                if (!section) return;
                const sectionRect = section.getBoundingClientRect();
                const scrollers = Array.from(section.querySelectorAll('*'))
                  .filter(el => {{
                    try {{
                      if (!visible(el)) return false;
                      if (el.scrollHeight <= el.clientHeight + 20) return false;
                      const r = el.getBoundingClientRect();
                      return r.left < sectionRect.left + 430;
                    }} catch (_) {{
                      return false;
                    }}
                  }});

                for (const el of scrollers) {{
                  try {{
                    const max = el.scrollHeight - el.clientHeight;
                    el.scrollTop = Math.max(0, Math.floor(max * position));
                  }} catch (_) {{}}
                }}
              }};
              const findRankItemOnce = (section, rank) => {{
                if (!section) return null;
                const sectionRect = section.getBoundingClientRect();
                const rankRe = new RegExp('^\\s*' + rank + '\\s+');
                const percentRe = /[-+]?\\d+(?:\\.\\d+)?%/;
                const candidates = Array.from(section.querySelectorAll('a, button, [role="button"], li, div'))
                  .filter(visible)
                  .map(el => {{
                    const r = el.getBoundingClientRect();
                    const t = text(el);
                    return {{ el, r, t }};
                  }})
                  .filter(item => {{
                    const r = item.r;
                    const t = item.t;

                    if (!rankRe.test(t)) return false;
                    if (!percentRe.test(t)) return false;
                    if (r.left > sectionRect.left + 430) return false;
                    if (r.width < 70) return false;
                    if (r.width > 430) return false;
                    if (r.height < 18) return false;
                    if (r.height > 135) return false;
                    if (t.length > 160) return false;

                    const multiRank = (t.match(/(?:^|\\s)(10|[1-9])\\s+[^\\s].*?[-+]?\\d+(?:\\.\\d+)?%/g) || []).length;
                    if (multiRank >= 2) return false;

                    return true;
                  }});

                candidates.sort((a, b) => {{
                  const clickableA = a.el.matches('a,button,[role="button"],li') ? 0 : 1;
                  const clickableB = b.el.matches('a,button,[role="button"],li') ? 0 : 1;

                  if (clickableA !== clickableB) return clickableA - clickableB;

                  return a.t.length - b.t.length;
                }});

                if (!candidates.length) return null;

                let target = candidates[0].el.closest('a,button,[role="button"],li') || candidates[0].el;
                let targetText = text(target);
                let targetRect = target.getBoundingClientRect();

                if (
                  !rankRe.test(targetText)
                  || !percentRe.test(targetText)
                  || targetRect.width > 450
                  || targetRect.height > 145
                ) {{
                  target = candidates[0].el;
                }}

                target.style.outline = '4px solid red';
                target.style.outlineOffset = '2px';
                return target;
              }};
              const findRankItemFast = async (section, rank) => {{
                const positions = [0, 0.18, 0.36, 0.54, 0.72, 1];

                for (const pos of positions) {{
                  scrollRankArea(section, pos);
                  await sleep(35);
                  const item = findRankItemOnce(section, rank);

                  if (item) return item;
                }}

                return null;
              }};
              const waitForViewButtonAfterClick = async (rank, beforeSignature) => {{
                const end = Date.now() + 450;

                while (Date.now() < end) {{
                  const btn = findViewAllButton();

                  if (btn) {{
                    const afterSignature = previewSignature();

                    if (
                      rank === 1
                      || !beforeSignature
                      || !afterSignature
                      || afterSignature !== beforeSignature
                    ) {{
                      btn.style.outline = '4px solid blue';
                      btn.style.outlineOffset = '2px';
                      return {{ btn, afterSignature }};
                    }}
                  }}

                  await sleep(50);
                }}

                return null;
              }};

              try {{
                let section = await waitForDiscussionSection(1500);

                if (!section || !text(section).includes('오늘의 종목 토론 둘러보기')) {{
                  throw new Error('오늘의 종목 토론 둘러보기 영역을 찾지 못했습니다.');
                }}

                let selectedCategory = null;

                for (const label of shuffle(categoryLabels)) {{
                  section = findDiscussionSection();
                  const btn = findCategoryButton(section, label);

                  if (!btn) continue;

                  clickFast(btn);
                  selectedCategory = label;
                  break;
                }}

                if (!selectedCategory) {{
                  selectedCategory = '기본 미리보기';
                }}

                await sleep(120);
                section = findDiscussionSection();

                const ranks = shuffle([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
                const tried = [];

                for (const rank of ranks) {{
                  section = findDiscussionSection();
                  const item = await findRankItemFast(section, rank);

                  if (!item) {{
                    tried.push(rank + ':not-found');
                    continue;
                  }}

                  const itemText = text(item);
                  const beforeSignature = previewSignature();
                  tried.push(rank + ':' + itemText);
                  clickFast(item);

                  const viewResult = await waitForViewButtonAfterClick(rank, beforeSignature);

                  if (!viewResult || !viewResult.btn) {{
                    tried.push(rank + ':view-all-not-visible');
                    continue;
                  }}

                  clickFast(viewResult.btn);

                  return JSON.stringify({{
                    ok: true,
                    category: selectedCategory,
                    rank: String(rank),
                    itemText: itemText,
                    method: 'view-all-button-fast',
                    tried: tried
                  }});
                }}

                const fallbackBtn = findViewAllButton();

                if (fallbackBtn) {{
                  fallbackBtn.style.outline = '4px solid blue';
                  fallbackBtn.style.outlineOffset = '2px';
                  clickFast(fallbackBtn);

                  return JSON.stringify({{
                    ok: true,
                    category: selectedCategory,
                    rank: 'fallback-visible',
                    itemText: '현재 보이는 기본 미리보기',
                    method: 'view-all-button-visible-fallback',
                    tried: tried
                  }});
                }}

                throw new Error(
                  '1~10위 중 전체 토론글 보러가기 버튼이 보이는 종목을 찾지 못했습니다. tried='
                  + tried.join(' | ')
                );
              }} catch (error) {{
                return JSON.stringify({{
                  ok: false,
                  error: error.message || String(error)
                }});
              }}
            }})({labels})
            "#
        );

        let raw_result = self.evaluate_string(&script)?;
        let result: Value = serde_json::from_str(&raw_result)?;

        if !result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            return Err(AutomationError::new(
                result
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("랜덤 선택 실패"),
            ));
        }

        self.wait_for_stock_discussion_url(Duration::from_secs(12))?;
        sleep(Duration::from_secs(1));

        Ok(DiscussionSelection {
            category: string_field(&result, "category"),
            rank: string_field(&result, "rank"),
            item_text: string_field(&result, "itemText"),
            method: string_field(&result, "method"),
        })
    }

    // 댓글 작성을 위해 현재 토론방 목록에서 임의의 게시글 하나를 열고 댓글 영역까지 이동하는 함수입니다.
    pub(super) fn open_random_discussion_post(&mut self) -> AutomationResult<()> {
        let result = self.evaluate_string(
            r#"
            (async () => {
              const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
              const text = el => String(el?.innerText || el?.textContent || '')
                .replace(/\s+/g, ' ')
                .trim();
              const visible = el => {
                if (!el) return false;
                const r = el.getBoundingClientRect();
                const s = getComputedStyle(el);
                return r.width > 0
                  && r.height > 0
                  && s.display !== 'none'
                  && s.visibility !== 'hidden'
                  && s.opacity !== '0';
              };
              const shuffle = items => {
                const copy = items.slice();
                for (let i = copy.length - 1; i > 0; i--) {
                  const j = Math.floor(Math.random() * (i + 1));
                  [copy[i], copy[j]] = [copy[j], copy[i]];
                }
                return copy;
              };
              const clickFast = el => {
                if (!el) return false;
                const target = findClickableTarget(el);
                target.scrollIntoView({ block: 'center', inline: 'center' });
                const r = target.getBoundingClientRect();
                const x = Math.floor(r.left + Math.max(20, Math.min(r.width * 0.45, r.width - 20)));
                const y = Math.floor(r.top + Math.max(16, Math.min(r.height * 0.45, r.height - 16)));
                const pointTarget = document.elementFromPoint(x, y) || target;

                for (const type of ['mouseover', 'mousemove', 'mousedown', 'mouseup', 'click']) {
                  pointTarget.dispatchEvent(new MouseEvent(type, {
                    bubbles: true,
                    cancelable: true,
                    clientX: x,
                    clientY: y,
                    button: 0
                  }));
                }

                try {
                  target.click();
                } catch (_) {}

                return true;
              };
              const findClickableTarget = el => {
                let node = el;

                for (let i = 0; i < 7 && node && node !== document.body; i++) {
                  const cursor = getComputedStyle(node).cursor;
                  if (
                    node.matches?.('a[href], button, [role="button"]')
                    || typeof node.onclick === 'function'
                    || cursor === 'pointer'
                  ) {
                    return node;
                  }
                  node = node.parentElement;
                }

                return el.closest?.('a[href], button, [role="button"]') || el;
              };
              const forbidden = value =>
                !value
                || value.includes('글쓰기')
                || value.includes('주주인증')
                || value.includes('주주오픈톡')
                || value.includes('클린봇')
                || value.includes('설정')
                || value.includes('더 알아보기')
                || value.includes('차트·시세')
                || value.includes('종목분석')
                || value.includes('뉴스·공시')
                || value.includes('공매도현황')
                || value.includes('인사이트')
                || value.includes('전체 주주글만 소식글만 소식글 제외');
              const likelyPostText = value =>
                value.includes('방금 전')
                || value.includes('분 전')
                || value.includes('시간 전')
                || value.includes('일 전')
                || value.includes('좋아요')
                || /팔로우\s+/.test(value);
              const candidateScore = el => {
                const r = el.getBoundingClientRect();
                const value = text(el);

                if (!visible(el)) return -1;
                if (forbidden(value)) return -1;
                if (value.length < 12 || value.length > 500) return -1;
                if (r.left < 330 || r.top < 250) return -1;
                if (r.width < 220 || r.width > 900) return -1;
                if (r.height < 28 || r.height > 260) return -1;

                let score = 0;
                if (likelyPostText(value)) score += 5;
                if (el.matches('a[href]')) score += 4;
                if (findClickableTarget(el) !== el) score += 2;
                if (value.length >= 25 && value.length <= 220) score += 2;
                if (value.includes('팔로우')) score += 1;
                if (/댓글|답글|의견/.test(value)) score += 1;

                return score;
              };
              const collectCandidates = () => {
                const selectors = [
                  'a[href*="/discussion"]',
                  'a[href*="discussion"]',
                  'article',
                  'li',
                  'div[class*="Discussion"]',
                  'div[class*="discussion"]',
                  'div[class*="Item"]',
                  'div[class*="item"]',
                  'div[class*="Card"]',
                  'div[class*="card"]'
                ];
                const nodes = [...new Set(selectors.flatMap(selector =>
                  [...document.querySelectorAll(selector)]
                ))];

                return nodes
                  .map(el => ({ el, score: candidateScore(el), value: text(el) }))
                  .filter(item => item.score >= 4)
                  .sort((a, b) => b.score - a.score);
              };
              const commentModuleReady = () =>
                Boolean(document.querySelector('#cbox_module'))
                || (document.body?.innerText || '').includes('의견을 남겨 보세요');
              const waitForCommentModule = async maxMs => {
                const end = Date.now() + maxMs;

                while (Date.now() < end) {
                  if (commentModuleReady()) return true;
                  window.scrollBy(0, Math.floor(window.innerHeight * 0.8));
                  await sleep(250);
                }

                return commentModuleReady();
              };
              const scrollPositions = [0, 0.18, 0.36, 0.54, 0.72, 1];

              for (const position of scrollPositions) {
                const maxScroll = Math.max(0, document.documentElement.scrollHeight - window.innerHeight);
                window.scrollTo(0, Math.floor(maxScroll * position));
                await sleep(400);

                const candidates = collectCandidates();
                const picked = shuffle(candidates.slice(0, 8))[0];

                if (!picked) continue;

                picked.el.style.outline = '4px solid red';
                picked.el.style.outlineOffset = '3px';
                const beforeUrl = location.href;
                clickFast(picked.el);
                await sleep(1500);

                if (location.href !== beforeUrl || commentModuleReady()) {
                  const ready = await waitForCommentModule(7000);

                  return JSON.stringify({
                    ok: ready,
                    error: ready ? '' : '댓글 영역을 찾지 못했습니다.',
                    selectedText: picked.value.slice(0, 140),
                    url: location.href
                  });
                }
              }

              return JSON.stringify({
                ok: false,
                error: '현재 토론방 목록에서 클릭 가능한 토론글을 찾지 못했습니다.'
              });
            })()
            "#,
        )?;

        let value: Value = serde_json::from_str(&result)?;

        if !value.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            return Err(AutomationError::new(
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("랜덤 토론글 선택 실패"),
            ));
        }

        sleep(Duration::from_secs(1));
        Ok(())
    }

    // 종목별 토론방 URL로 이동이 끝났는지 기다리는 함수입니다.
    fn wait_for_stock_discussion_url(&mut self, timeout: Duration) -> AutomationResult<()> {
        let end = Instant::now() + timeout;

        while Instant::now() < end {
            let url = self.current_url()?;

            if url.contains("/domestic/stock/") || url.contains("/worldstock/stock/") {
                return Ok(());
            }

            sleep(Duration::from_millis(500));
        }

        Ok(())
    }
}
