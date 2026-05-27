use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::json;

use super::{AutomationError, AutomationResult, CdpClient};

impl CdpClient {
    // 글쓰기 버튼을 눌러 글쓰기 모달을 열고, 프로필 설정이 필요하면 먼저 처리하는 함수입니다.
    pub(super) fn open_write_modal(&mut self) -> AutomationResult<()> {
        self.click_text("글쓰기", Duration::from_secs(20))?;
        sleep(Duration::from_secs(2));

        if self.setup_profile_if_needed()? {
            self.click_text("글쓰기", Duration::from_secs(20))?;
            sleep(Duration::from_secs(2));
        }

        Ok(())
    }

    // 댓글 작성 전에 프로필 생성 요구가 있는지 확인하고 소개 2222 설정을 수행하는 함수입니다.
    pub(super) fn ensure_profile_setup_for_comment(&mut self) -> AutomationResult<()> {
        self.click_text("글쓰기", Duration::from_secs(20))?;
        sleep(Duration::from_secs(2));

        if self.setup_profile_if_needed()? {
            return Ok(());
        }

        self.close_write_modal_if_present()?;
        Ok(())
    }

    // 프로필 설정 팝업이 보일 때 소개 2222를 입력하고 완료하는 함수입니다.
    pub(super) fn setup_profile_if_needed(&mut self) -> AutomationResult<bool> {
        sleep(Duration::from_secs(1));

        if !self.has_profile_setup_popup()? {
            return Ok(false);
        }

        self.click_text("설정하기", Duration::from_secs(5))?;
        sleep(Duration::from_millis(1500));
        self.fill_default_profile_intro()?;
        self.click_text("완료", Duration::from_secs(5))?;
        self.reload_after_profile_setup()?;

        Ok(true)
    }

    // 현재 화면에 프로필 설정 요구 팝업이 있는지 확인하는 함수입니다.
    fn has_profile_setup_popup(&mut self) -> AutomationResult<bool> {
        self.evaluate_bool(
            r#"
            (() => {
              const body = document.body?.innerText || '';
              return body.includes('프로필을 먼저 설정해주세요')
                || body.includes('설정하기');
            })()
            "#,
        )
    }

    // 프로필 소개 입력란에 기본 소개값 2222를 입력하는 함수입니다.
    fn fill_default_profile_intro(&mut self) -> AutomationResult<()> {
        let result = self.evaluate_string(
            r#"
            (() => {
              const visible = el => {
                if (!el) return false;
                const r = el.getBoundingClientRect();
                const s = getComputedStyle(el);
                return r.width > 0
                  && r.height > 0
                  && s.display !== 'none'
                  && s.visibility !== 'hidden';
              };
              const fire = node => {
                node.dispatchEvent(new Event('input', { bubbles: true }));
                node.dispatchEvent(new Event('change', { bubbles: true }));
                node.dispatchEvent(new KeyboardEvent('keyup', { bubbles: true }));
              };
              const candidates = [
                ...document.querySelectorAll(
                  "textarea, input, [contenteditable='true'], [role='textbox']"
                )
              ].filter(visible);
              const intro = candidates.find(el => {
                const p = el.getAttribute('placeholder') || '';
                return p.includes('소개')
                  || p.includes('자기소개')
                  || el.getAttribute('maxlength') === '300';
              }) || candidates[0] || null;

              if (!intro) return 'failed:no-intro';

              intro.scrollIntoView({ block: 'center', inline: 'center' });
              intro.focus();

              const tag = intro.tagName.toLowerCase();

              if (tag === 'textarea') {
                const setter = Object.getOwnPropertyDescriptor(
                  HTMLTextAreaElement.prototype,
                  'value'
                ).set;
                setter.call(intro, '2222');
                fire(intro);
                return 'ok:textarea';
              }

              if (tag === 'input') {
                const setter = Object.getOwnPropertyDescriptor(
                  HTMLInputElement.prototype,
                  'value'
                ).set;
                setter.call(intro, '2222');
                fire(intro);
                return 'ok:input';
              }

              intro.textContent = '2222';
              fire(intro);
              return 'ok:text';
            })()
            "#,
        )?;

        if result.starts_with("ok:") {
            sleep(Duration::from_millis(500));
            return Ok(());
        }

        Err(AutomationError::new(
            "프로필 소개 입력란을 찾지 못했습니다.",
        ))
    }

    // 프로필 설정 완료 후 페이지를 새로고침하고 안정화될 때까지 기다리는 함수입니다.
    fn reload_after_profile_setup(&mut self) -> AutomationResult<()> {
        sleep(Duration::from_millis(2500));
        self.call("Page.reload", json!({ "ignoreCache": false }))?;
        self.wait_for_ready_state(Duration::from_secs(30))?;
        sleep(Duration::from_secs(4));
        Ok(())
    }

    // 수동 확인 모드에서 글쓰기 제목과 본문을 화면 입력란에 채우는 함수입니다.
    pub(super) fn fill_post_form(&mut self, title: &str, body: &str) -> AutomationResult<()> {
        if self.fill_modal_post_form(title, body)? {
            return Ok(());
        }

        self.close_write_modal_if_present()?;

        if self.fill_comment_editor(body)? {
            return Ok(());
        }

        Err(AutomationError::new(
            "글쓰기 입력란 또는 댓글 입력란을 찾지 못했습니다.",
        ))
    }

    // 수동 확인 모드에서 댓글 입력란에 내용을 채우는 함수입니다.
    pub(super) fn fill_comment_form(&mut self, body: &str) -> AutomationResult<()> {
        if self.fill_comment_editor(body)? {
            return Ok(());
        }

        Err(AutomationError::new("댓글 입력란을 찾지 못했습니다."))
    }

    // Wireshark/F12에서 확인한 POST /front-api/discussion/add 패킷 구조로 글을 등록하는 함수입니다.
    pub(super) fn submit_post_and_refresh(
        &mut self,
        title: &str,
        body: &str,
    ) -> AutomationResult<()> {
        let title = serde_json::to_string(title)?;
        let body = serde_json::to_string(body)?;
        let result = self.evaluate_string(&format!(
            r#"
            (async () => {{
              const titleValue = {title};
              const bodyValue = {body};
              const current = new URL(window.location.href);
              const match = current.pathname.match(/\/stock\/([^/]+)\/discussion/);

              if (!match) return `failed:no-item-code:${{current.pathname}}`;

              const itemCode = decodeURIComponent(match[1]);
              const discussionType = current.pathname.includes('/domestic/stock/')
                ? 'domesticStock'
                : current.pathname.includes('/worldstock/stock/')
                  ? 'worldStock'
                  : 'domesticStock';
              const uuid = () => globalThis.crypto?.randomUUID?.()
                || `SE-${{Date.now()}}-${{Math.random().toString(16).slice(2)}}`;
              const bodyLength = Array.from(bodyValue).length;
              const payload = {{
                title: titleValue,
                contentJson: {{
                  document: {{
                    version: '2.9.0',
                    theme: 'default',
                    language: 'ko-KR',
                    id: uuid(),
                    components: [{{
                      id: `SE-${{uuid()}}`,
                      layout: 'default',
                      value: [{{
                        id: `SE-${{uuid()}}`,
                        nodes: [{{
                          id: `SE-${{uuid()}}`,
                          value: bodyValue,
                          '@ctype': 'textNode'
                        }}],
                        '@ctype': 'paragraph'
                      }}],
                      '@ctype': 'text'
                    }}],
                    di: {{
                      dif: false,
                      dio: [{{
                        dis: 'N',
                        dia: {{
                          t: 0,
                          p: 0,
                          st: bodyLength,
                          sk: 0
                        }}
                      }}]
                    }}
                  }},
                  documentId: ''
                }},
                isCleanbotDisabled: false,
                danglingImages: [],
                discussionType,
                itemCode,
                txId: uuid(),
                inflow: 'NFS-P-P'
              }};

              const response = await fetch(
                'https://m.stock.naver.com/front-api/discussion/add',
                {{
                  method: 'POST',
                  mode: 'cors',
                  credentials: 'include',
                  headers: {{
                    accept: 'application/json, text/plain, */*',
                    'content-type': 'application/json'
                  }},
                  body: JSON.stringify(payload)
                }}
              );
              const text = await response.text();
              let data = null;

              try {{
                data = text ? JSON.parse(text) : null;
              }} catch (_) {{
                return `failed:invalid-json:${{text.slice(0, 180)}}`;
              }}

              if (!response.ok) {{
                return `failed:http-${{response.status}}:${{text.slice(0, 180)}}`;
              }}

              if (data?.isSuccess !== true) {{
                return `failed:api:${{data?.message || data?.detailCode || text.slice(0, 180)}}`;
              }}

              return `ok:post:${{data?.result?.id || ''}}`;
            }})()
            "#
        ))?;

        if !result.starts_with("ok:post:") {
            return Err(AutomationError::new(format!(
                "패킷 기반 글쓰기 등록 실패: {result}"
            )));
        }

        self.reload_after_submit()
    }

    // Wireshark/F12에서 확인한 cbox 토큰 발급/댓글 생성 패킷 구조로 댓글을 등록하는 함수입니다.
    pub(super) fn submit_comment_and_refresh(&mut self, body: &str) -> AutomationResult<()> {
        let body = serde_json::to_string(body)?;
        let result = self.evaluate_string(&format!(
            r#"
            (async () => {{
              const bodyValue = {body};
              const current = new URL(window.location.href);
              const match = current.pathname.match(/\/discussion\/(\d+)/);

              if (!match) return `failed:no-object-id:${{current.pathname}}`;

              const objectId = match[1];
              const objectUrl = current.href.split('#')[0];
              const tokenParams = new URLSearchParams({{
                ticket: 'finance',
                templateId: 'community',
                pool: 'cbox12',
                _cv: '',
                lang: 'ko',
                pageType: 'more',
                country: '',
                objectId,
                categoryId: '',
                pageSize: '10',
                indexSize: '10',
                groupId: '',
                listType: 'OBJECT',
                clientType: 'web-pc',
                objectUrl
              }});
              const tokenResponse = await fetch(
                `https://apis.naver.com/commentBox/cbox/web_naver_token_json.json?${{tokenParams.toString()}}`,
                {{
                  method: 'GET',
                  mode: 'cors',
                  credentials: 'include',
                  headers: {{
                    accept: 'application/json, text/javascript, */*; q=0.01'
                  }}
                }}
              );
              const tokenText = await tokenResponse.text();
              let tokenJson = null;

              try {{
                tokenJson = tokenText ? JSON.parse(tokenText) : null;
              }} catch (_) {{
                return `failed:token-invalid-json:${{tokenText.slice(0, 180)}}`;
              }}

              if (!tokenResponse.ok) {{
                return `failed:token-http-${{tokenResponse.status}}:${{tokenText.slice(0, 180)}}`;
              }}

              const cboxToken = tokenJson?.result?.cbox_token;

              if (!cboxToken) {{
                return `failed:no-cbox-token:${{tokenText.slice(0, 180)}}`;
              }}

              const form = new URLSearchParams({{
                lang: 'ko',
                pageType: 'more',
                country: '',
                objectId,
                categoryId: '',
                pageSize: '10',
                indexSize: '10',
                groupId: '',
                listType: 'OBJECT',
                clientType: 'web-pc',
                objectUrl,
                contents: bodyValue,
                userType: '',
                pick: 'false',
                manager: 'false',
                score: '0',
                likeItId: '',
                secret: 'false',
                refresh: 'true',
                imageCount: '0',
                commentType: 'txt',
                validateBanWords: 'true',
                invalidateCleanbotAlert: 'false',
                cbox_token: cboxToken
              }});
              const createResponse = await fetch(
                'https://apis.naver.com/commentBox/cbox/web_naver_create_json.json?ticket=finance&templateId=community&pool=cbox12&_cv=',
                {{
                  method: 'POST',
                  mode: 'cors',
                  credentials: 'include',
                  headers: {{
                    accept: 'application/json, text/javascript, */*; q=0.01',
                    'content-type': 'application/x-www-form-urlencoded; charset=UTF-8'
                  }},
                  body: form.toString()
                }}
              );
              const createText = await createResponse.text();
              let createJson = null;

              try {{
                createJson = createText ? JSON.parse(createText) : null;
              }} catch (_) {{
                return `failed:create-invalid-json:${{createText.slice(0, 180)}}`;
              }}

              if (!createResponse.ok) {{
                return `failed:create-http-${{createResponse.status}}:${{createText.slice(0, 180)}}`;
              }}

              const created = createJson?.success === true
                || createJson?.result?.comment
                || createJson?.result?.commentList;

              if (!created) {{
                return `failed:create-api:${{createJson?.message || createJson?.code || createText.slice(0, 180)}}`;
              }}

              return `ok:comment:${{createJson?.result?.comment?.commentNo || ''}}`;
            }})()
            "#
        ))?;

        if !result.starts_with("ok:comment:") {
            return Err(AutomationError::new(format!(
                "패킷 기반 댓글 등록 실패: {result}"
            )));
        }

        self.reload_after_submit()
    }

    // 글쓰기 또는 댓글 등록 후 화면을 새로고침하는 함수입니다.
    fn reload_after_submit(&mut self) -> AutomationResult<()> {
        sleep(Duration::from_millis(2500));
        self.call("Page.reload", json!({ "ignoreCache": false }))?;
        self.wait_for_ready_state(Duration::from_secs(30))?;
        sleep(Duration::from_secs(2));
        Ok(())
    }

    // 글쓰기 모달 안의 제목 입력란과 본문 에디터를 찾아 값을 넣는 함수입니다.
    fn fill_modal_post_form(&mut self, title: &str, body: &str) -> AutomationResult<bool> {
        let title = serde_json::to_string(title)?;
        let body = serde_json::to_string(body)?;
        let end = Instant::now() + Duration::from_secs(20);

        while Instant::now() < end {
            let result = self.evaluate_string(&format!(
                r#"
                (() => {{
                  const titleValue = {title};
                  const bodyValue = {body};
                  const visible = el => {{
                    if (!el) return false;
                    const r = el.getBoundingClientRect();
                    const s = getComputedStyle(el);
                    return r.width > 0
                      && r.height > 0
                      && s.visibility !== 'hidden'
                      && s.display !== 'none';
                  }};
                  const text = el => String(el?.innerText || el?.textContent || '')
                    .replace(/\s+/g, ' ')
                    .trim();
                  const fire = node => {{
                    node.dispatchEvent(new Event('beforeinput', {{ bubbles: true }}));
                    node.dispatchEvent(new Event('input', {{ bubbles: true }}));
                    node.dispatchEvent(new Event('change', {{ bubbles: true }}));
                    node.dispatchEvent(new KeyboardEvent('keyup', {{ bubbles: true }}));
                  }};
                  const modal = findWriteModal();

                  function findWriteModal() {{
                    const registerBtn = [...document.querySelectorAll('button, a')]
                      .filter(visible)
                      .find(el => text(el).includes('등록하기'));

                    if (!registerBtn) return null;

                    let node = registerBtn;

                    while (node && node !== document.body) {{
                      const t = text(node);
                      if (t.includes('글쓰기') && t.includes('등록하기')) return node;
                      node = node.parentElement;
                    }}

                    return null;
                  }}

                  function findTitleInput(modal) {{
                    const inputs = [...modal.querySelectorAll('input')]
                      .filter(visible)
                      .filter(el => (el.type || '').toLowerCase() !== 'hidden');

                    return inputs.find(el => (el.getAttribute('placeholder') || '').includes('제목'))
                      || inputs[0]
                      || null;
                  }}

                  function findBodyEditor(modal) {{
                    const candidates = [
                      ...modal.querySelectorAll(
                      'textarea, [contenteditable="true"], [role="textbox"], div[class*="editor"], div[class*="Editor"], div[class*="content"], div[class*="Content"]'
                      )
                    ].filter(visible);

                    let editor = candidates.find(el => {{
                      const t = [
                        el.getAttribute('placeholder') || '',
                        el.getAttribute('aria-label') || '',
                        el.getAttribute('data-placeholder') || '',
                        text(el)
                      ].join(' ');

                      return t.includes('종목에 대한 의견')
                        || t.includes('의견을 남겨')
                        || el.getAttribute('contenteditable') === 'true'
                        || el.getAttribute('role') === 'textbox'
                        || el.tagName.toLowerCase() === 'textarea';
                    }}) || candidates[0] || null;

                    if (editor) return editor;

                    const placeholder = [...modal.querySelectorAll('*')]
                      .filter(visible)
                      .find(el =>
                        text(el).includes('종목에 대한 의견을 남겨보세요')
                        || text(el).includes('종목에 대한 의견')
                      );

                    if (placeholder) {{
                      placeholder.scrollIntoView({{ block: 'center', inline: 'center' }});
                      placeholder.click();
                    }}

                    return [
                      ...modal.querySelectorAll(
                        'textarea, [contenteditable="true"], [role="textbox"], div[class*="editor"], div[class*="Editor"], div[class*="content"], div[class*="Content"]'
                      )
                    ].filter(visible)[0] || null;
                  }}

                  function setInputValue(input, value) {{
                    input.scrollIntoView({{ block: 'center', inline: 'center' }});
                    input.focus();
                    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')
                      .set.call(input, value);
                    fire(input);
                  }}

                  function setEditorValue(editor, value) {{
                    editor.scrollIntoView({{ block: 'center', inline: 'center' }});
                    editor.focus();

                    const tag = editor.tagName.toLowerCase();

                    if (tag === 'textarea') {{
                      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')
                        .set.call(editor, value);
                      fire(editor);
                      return 'ok:textarea';
                    }}

                    if (tag === 'input') {{
                      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')
                        .set.call(editor, value);
                      fire(editor);
                      return 'ok:input';
                    }}

                    if (
                      editor.isContentEditable
                      || editor.getAttribute('contenteditable') === 'true'
                      || editor.getAttribute('role') === 'textbox'
                    ) {{
                      const selection = window.getSelection();
                      const range = document.createRange();
                      range.selectNodeContents(editor);
                      selection.removeAllRanges();
                      selection.addRange(range);
                      document.execCommand('delete', false, null);
                      const inserted = document.execCommand('insertText', false, value);
                      fire(editor);

                      if (
                        inserted
                        || text(editor).includes(String(value).split('\n')[0].trim().slice(0, 8))
                      ) {{
                        return 'ok:contenteditable-native';
                      }}

                      editor.innerHTML = '';
                      String(value).split('\n').forEach((line, index) => {{
                        if (index > 0) editor.appendChild(document.createElement('br'));
                        editor.appendChild(document.createTextNode(line));
                      }});
                      fire(editor);
                      return 'ok:contenteditable-fallback';
                    }}

                    editor.textContent = value;
                    fire(editor);
                    return 'ok:text-node';
                  }}

                  if (!modal) return 'retry:no-modal';

                  const titleInput = findTitleInput(modal);
                  if (!titleInput) return 'failed:no-title';

                  const bodyEditor = findBodyEditor(modal);
                  if (!bodyEditor) return 'failed:no-editor';

                  setInputValue(titleInput, titleValue);
                  return setEditorValue(bodyEditor, bodyValue);
                }})()
                "#
            ))?;

            if result.starts_with("ok:") {
                sleep(Duration::from_secs(1));
                return Ok(true);
            }

            if result == "failed:no-title"
                || result == "failed:no-editor"
                || result == "failed:unsupported-editor"
            {
                return Ok(false);
            }

            if result.starts_with("failed:") {
                return Err(AutomationError::new(format!("글 입력 실패: {result}")));
            }

            sleep(Duration::from_millis(500));
        }

        Ok(false)
    }

    // 열려 있는 글쓰기 모달이 있으면 닫는 함수입니다.
    fn close_write_modal_if_present(&mut self) -> AutomationResult<()> {
        self.evaluate_bool(
            r#"
            (() => {
              const visible = el => {
                if (!el) return false;
                const r = el.getBoundingClientRect();
                const s = getComputedStyle(el);
                return r.width > 0
                  && r.height > 0
                  && s.visibility !== 'hidden'
                  && s.display !== 'none';
              };
              const text = el => String(el?.innerText || el?.textContent || '')
                .replace(/\s+/g, ' ')
                .trim();
              const buttons = [...document.querySelectorAll('button, a, [role="button"]')]
                .filter(visible);
              const closeButton = buttons.find(el =>
                text(el) === '닫기'
                || text(el) === '취소'
                || (el.getAttribute('aria-label') || '').includes('닫기')
                || (el.className || '').toString().includes('close')
              );

              if (!closeButton) return false;

              closeButton.click();
              return true;
            })()
            "#,
        )?;
        sleep(Duration::from_millis(500));
        Ok(())
    }

    // 댓글 입력 안내 영역을 클릭한 뒤 실제 댓글 에디터에 내용을 넣는 함수입니다.
    fn fill_comment_editor(&mut self, body: &str) -> AutomationResult<bool> {
        let body = serde_json::to_string(body)?;
        let result = self.evaluate_string(&format!(
            r#"
            (() => {{
              const bodyValue = {body};
              const visible = el => {{
                if (!el) return false;
                const r = el.getBoundingClientRect();
                const s = getComputedStyle(el);
                return r.width > 0
                  && r.height > 0
                  && s.visibility !== 'hidden'
                  && s.display !== 'none';
              }};
              const text = el => String(el?.innerText || el?.textContent || '')
                .replace(/\s+/g, ' ')
                .trim();
              const fire = node => {{
                node.dispatchEvent(new Event('beforeinput', {{ bubbles: true }}));
                node.dispatchEvent(new Event('input', {{ bubbles: true }}));
                node.dispatchEvent(new Event('change', {{ bubbles: true }}));
                node.dispatchEvent(new KeyboardEvent('keyup', {{ bubbles: true }}));
              }};
              const clickCommentPlaceholder = () => {{
                const placeholder =
                  document.querySelector('#cbox_module .u_cbox_guide[data-action*="write"]')
                  || document.querySelector('#cbox_module .u_cbox_guide')
                  || document.evaluate(
                    '//*[@id="cbox_module"]/div/div[2]/div[1]/form/fieldset/div/div/div[2]/div/div[2]',
                    document,
                    null,
                    XPathResult.FIRST_ORDERED_NODE_TYPE,
                    null
                  ).singleNodeValue;

                if (!placeholder || !visible(placeholder)) return false;
                placeholder.scrollIntoView({{ block: 'center', inline: 'center' }});
                placeholder.click();
                return true;
              }};
              const findCommentEditor = () => {{
                const root = document.querySelector('#cbox_module') || document;
                const selectors = [
                  '[contenteditable="true"]',
                  '[role="textbox"]',
                  'textarea',
                  '.u_cbox_text',
                  '.u_cbox_write_area textarea',
                  '.u_cbox_write_area [contenteditable="true"]'
                ];

                for (const selector of selectors) {{
                  const editor = [...root.querySelectorAll(selector)].find(visible);
                  if (editor) return editor;
                }}

                return null;
              }};
              const setEditorValue = editor => {{
                editor.scrollIntoView({{ block: 'center', inline: 'center' }});
                editor.focus();

                const tag = editor.tagName.toLowerCase();

                if (tag === 'textarea') {{
                  Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')
                    .set.call(editor, bodyValue);
                  fire(editor);
                  return 'ok:comment-textarea';
                }}

                if (
                  editor.isContentEditable
                  || editor.getAttribute('contenteditable') === 'true'
                  || editor.getAttribute('role') === 'textbox'
                ) {{
                  const selection = window.getSelection();
                  const range = document.createRange();
                  range.selectNodeContents(editor);
                  selection.removeAllRanges();
                  selection.addRange(range);
                  document.execCommand('delete', false, null);
                  const inserted = document.execCommand('insertText', false, bodyValue);
                  fire(editor);

                  if (
                    inserted
                    || text(editor).includes(String(bodyValue).split('\n')[0].trim().slice(0, 8))
                  ) {{
                    return 'ok:comment-contenteditable-native';
                  }}

                  editor.innerHTML = '';
                  String(bodyValue).split('\n').forEach((line, index) => {{
                    if (index > 0) editor.appendChild(document.createElement('br'));
                    editor.appendChild(document.createTextNode(line));
                  }});
                  fire(editor);
                  return 'ok:comment-contenteditable-fallback';
                }}

                return 'failed:unsupported-comment-editor';
              }};

              clickCommentPlaceholder();
              const editor = findCommentEditor();

              if (!editor) return 'retry:no-comment-editor';

              return setEditorValue(editor);
            }})()
            "#
        ))?;

        if result.starts_with("ok:") {
            sleep(Duration::from_secs(1));
            return Ok(true);
        }

        if result.starts_with("failed:") {
            return Err(AutomationError::new(format!("댓글 입력 실패: {result}")));
        }

        Ok(false)
    }

    // 수동 확인 모드에서 등록할 버튼을 빨간 테두리로 표시하는 함수입니다.
    pub(super) fn highlight_manual_submit_target(&mut self) -> AutomationResult<bool> {
        let register_highlighted = self.highlight_register_button()?;
        let comment_highlighted = self.highlight_comment_submit_button()?;

        Ok(register_highlighted || comment_highlighted)
    }

    // 글쓰기 모달의 등록하기 버튼을 빨간 테두리로 표시하는 함수입니다.
    fn highlight_register_button(&mut self) -> AutomationResult<bool> {
        self.evaluate_bool(
            r#"
            (() => {
              const visible = el => {
                const r = el.getBoundingClientRect();
                const s = getComputedStyle(el);
                return r.width > 0
                  && r.height > 0
                  && s.visibility !== 'hidden'
                  && s.display !== 'none';
              };
              const buttons = [...document.querySelectorAll('button, a')]
                .filter(visible)
                .filter(el => (el.innerText || el.textContent || '').includes('등록하기'));

              if (!buttons.length) return false;

              const btn = buttons[0];
              btn.scrollIntoView({ block:'center', inline:'center' });
              btn.style.outline = '4px solid red';
              btn.style.outlineOffset = '3px';
              return true;
            })()
            "#,
        )
    }

    // 댓글 등록 버튼을 빨간 테두리로 표시하는 함수입니다.
    fn highlight_comment_submit_button(&mut self) -> AutomationResult<bool> {
        self.evaluate_bool(
            r#"
            (() => {
              const visible = el => {
                if (!el) return false;
                const r = el.getBoundingClientRect();
                const s = getComputedStyle(el);
                return r.width > 0
                  && r.height > 0
                  && s.visibility !== 'hidden'
                  && s.display !== 'none';
              };
              const root = document.querySelector('#cbox_module') || document;
              const text = el => String(el?.innerText || el?.textContent || '')
                .replace(/\s+/g, ' ')
                .trim();
              const candidates = [
                ...root.querySelectorAll('button, a, [role="button"], input[type="submit"]')
              ].filter(visible);
              const btn = candidates.find(el =>
                text(el).includes('등록')
                || text(el).includes('댓글')
                || text(el).includes('입력')
                || (el.value || '').includes('등록')
              );

              if (!btn) return false;

              btn.scrollIntoView({ block:'center', inline:'center' });
              btn.style.outline = '4px solid red';
              btn.style.outlineOffset = '3px';
              return true;
            })()
            "#,
        )
    }
}
