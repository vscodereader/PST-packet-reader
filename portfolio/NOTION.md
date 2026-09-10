**프로젝트 개요**<br>PSTMACRO는 여러 계정과 채널의 로그인·게시·댓글·신고 작업을 운영하고, 중앙 Admin에서 하위 PC를 원격 제어하는 Windows 자동화 시스템입니다.**<br>**
**핵심 기능**<br>• 네이버 종목토론방·카페·블로그 및 밴드 자동화<br>• 다중 계정 로그인과 세션 유지<br>• 작업 큐, 예약 실행과 계정별 병렬 처리<br>• 중앙 Admin의 기기 등록·명령 분배·결과 보고<br>• 통신 원문 로그와 장애 진단**<br>기술 구성**<br>• **화면**: React 19, TypeScript, Vite<br>• **데스크톱**: Tauri 2, Rust<br>• **브라우저 제어**: Chrome DevTools Protocol<br>• **중앙 서버**: Rust API 서버<br>• **저장소**: PostgreSQL과 테스트용 memory repository**<br>**
**구현 구조<br>**
	1. **데스크톱 애플리케이션**<br>React 화면은 작업 입력과 상태 표시를 담당합니다. Tauri command는 입력을 Rust 도메인 계층으로 넘기며, Rust 모듈은 로그인·게시·신고·계정 상태·CDP 제어를 각각 처리합니다.**<br>**
	2. **중앙 원격제어**<br>중앙 서버는 API route와 repository trait를 분리했습니다. PostgreSQL 구현과 memory 구현이 같은 계약을 사용하므로 실제 저장소 없이도 동작을 빠르게 테스트할 수 있습니다.**<br>**
	3. **기존 코드 재사용**<br>기존 게시 파이프라인의 입력 계약인 `{code, name}` 배열을 유지했습니다. 자동·수동 종목 선택은 입력 배열을 만드는 UI에서만 나눴고, 종목 선택 화면은 공유 view로 추출해 데스크톱에는 IPC 어댑터, Admin에는 API 어댑터를 연결했습니다.**<br>**
	4. **기기 식별**<br>MachineGuid 또는 machine-id를 서버에서 upsert했습니다. 같은 PC는 재설치 후에도 동일한 device_id를 유지합니다.**<br>**
**개발 전 준비**<br>1. 기존 코드와 동료의 이슈·PR 작성 형식을 확인했습니다.<br>2. 서비스 동작이 바뀐 경우 패킷, HTML, CDP 네트워크 로그와 실패 응답을 먼저 수집했습니다.<br>3. 기능을 micro-task로 나누고 의존성을 blockedBy로 기록했습니다.<br>4. 독립 작업은 별도 worktree에서 실행했습니다.<br>5. node_modules와 Husky wrapper만 공유하고 Cargo·Vite 결과물은 분리했습니다.**<br>**
패킷 분석 기반 개발 절차
1. Wireshark로 정상 동작과 실패 동작을 각각 캡처함. 로그인, 약관 동의, 프로필 생성 전후, 글·댓글 작성, 수정, 반응, 실명 인증, CAPTCHA·WASM 실험을 시나리오별 pcapng로 나눠 D:\\packet_copy에 보관함.
2. Chrome의 TLS key log를 캡처 파일과 함께 사용해 HTTPS 트래픽을 복호화함. TCP 연결 위의 TLS 세션을 확인한 뒤 HTTP/2의 stream별 HEADERS와 DATA를 조합해 실제 요청·응답을 읽음.
3. Wireshark GUI에서 문제 구간을 찾고 TShark CLI로 :method, :authority, :path, stream ID와 상태 코드를 추출함. 정적 리소스·광고·분석 요청을 제외하고 기능과 직접 관계된 GET·POST·PUT·DELETE만 정리함.
4. 브라우저에서 성공한 요청의 헤더, Content-Type, Cookie 적용 범위, Referer·Origin, 요청 body, 리다이렉트 순서와 응답 JSON·HTML을 코드의 HTTP 클라이언트 및 CDP 로그와 1:1로 비교함.
5. 패킷에서 관측한 사실과 추정을 분리함. 세션 쿠키 값, 계정 식별값, TLS 비밀키와 개별 게시물 ID는 문서·로그에 남기지 않고 이름·역할·도메인·만료 조건만 기록함.
**코드 생성과 리뷰**<br>1. CLAUDE.md, 설계서, 대상 모듈과 인접 테스트만 작업 문맥으로 제공했습니다.<br>2. 도메인·저장소·API·UI·테스트 단위로 구현하고 작은 커밋으로 나눴습니다.<br>3. 로그인 암호화, 게시 분배, 예약과 계정 상태 등 기존 불변 영역의 회귀를 먼저 검사했습니다.<br>4. TypeScript strict, Vitest, Rust test·clippy와 커버리지 게이트를 실행했습니다.<br>5. 브랜치명, PR 제목, Closes 태그와 검증 결과를 확인했습니다.
**사용한 스킬과 토큰 절감**<br>아래 수치는 작업 한 건을 기준으로 한 추정치입니다. 여러 스킬을 함께 사용했으므로 절감률은 합산하지 않습니다.**<br>**
**\<설계 단계\><br>**
	**1. brainstorming**<br>**사용 시점**<br>구현 전에 요구사항, 제약 조건과 완료 기준을 확정할 때 사용했습니다.<br>**만든 결과**<br>Admin 원격제어, 게시 명령, 로그인과 채널별 자동화의 경계를 설계서에 고정했습니다.<br>**토큰 절감: 약 15\~30%**<br>구현 중에 반복해서 질문할 내용을 시작 전에 결정 목록으로 압축했습니다.**<br>**
	**2. writing-plans**<br>**사용 시점**<br>확정된 요구사항을 파일, 구현 단계와 테스트 단위의 실행 계획으로 바꿀 때 사용했습니다.<br>**만든 결과**<br>원격제어, 로그인 스텔스, 게시·신고·블로그 기능의 ADR과 설계서를 작성했습니다.<br>**토큰 절감: 약 20\~40%**<br>다음 세션에서 저장소 전체를 다시 읽지 않고 계획서와 대상 파일만 읽었습니다.**<br>구현·진단 단계<br>**
	**3. test-driven-development**<br>**사용 시점**<br>큐, repository, API, 패킷 변환과 회귀 오류를 수정할 때 사용했습니다.<br>**만든 결과**<br>실패 테스트를 먼저 만들고 memory repository, wiremock과 Vitest로 최소 구현을 검증했습니다.<br>**토큰 절감: 약 15\~35%**<br>테스트가 기대 결과를 고정해 잘못된 구현과 요구사항 재설명을 줄였습니다.**<br>**
	**4. systematic-debugging**<br>**사용 시점**<br>CAPTCHA, 쿠키, DOM 변경과 HTTP 400·403·404·429·500 오류를 진단할 때 사용했습니다.<br>**만든 결과**<br>CDP 로그, 요청·응답 원문, 백트레이스와 환경 변수 진단 게이트를 만들었습니다.<br>**토큰 절감: 약 30\~60%**<br>전체 코드와 로그 대신 재현 → 관측 → 가설 → 최소 수정 순서에 필요한 증거만 읽었습니다.**<br>**
	**5. dispatching-parallel-agents**<br>**사용 시점**<br>파일과 상태를 공유하지 않는 micro-task를 동시에 처리할 때 사용했습니다.<br>**만든 결과**<br>UI, Rust 도메인, 서버와 테스트 작업을 worktree별로 분리했습니다.<br>**토큰 절감: 약 10\~30%**<br>각 작업에 필요한 문맥만 전달해 전체 저장소를 반복해서 읽는 입력을 줄였습니다.**<br>리뷰·완료 단계<br>**
	**6. requesting-code-review**<br>**사용 시점**<br>구현 후 요구 누락, 회귀, 보안과 테스트 범위를 검토할 때 사용했습니다.<br>**만든 결과**<br>PR 목적, 검증 결과, Closes 태그와 회귀 위험을 확인했습니다.<br>**토큰 절감: 약 10\~25%**<br>리뷰 범위와 확인 항목을 고정해 막연한 전체 코드 리뷰를 줄였습니다.**<br>**
	**7. receiving-code-review**<br>**사용 시점**<br>리뷰 지적을 재현하고 실제 수정 여부를 결정할 때 사용했습니다.<br>**만든 결과**<br>리뷰 내용을 패킷과 테스트 결과에 대조해 필요한 지적만 반영했습니다.<br>**토큰 절감: 약 10\~25%**<br>잘못된 지적이나 범위 밖 변경으로 발생하는 재작업을 줄였습니다.**<br>**
	**8. verification-before-completion**<br>**사용 시점**<br>커밋과 PR을 만들기 직전에 사용했습니다.<br>**만든 결과**<br>TypeScript, Vitest, Rust test·clippy, 커버리지와 diff 검증 결과를 남겼습니다.<br>**토큰 절감: 약 10\~20%**<br>검증 실패 후 PR 설명과 수정 대화를 다시 만드는 일을 줄였습니다.**<br>**
	**9. finishing-a-development-branch**<br>**사용 시점**<br>구현과 검증을 마치고 브랜치와 PR을 정리할 때 사용했습니다.<br>**만든 결과**<br>이슈 연결, 상세 커밋 본문과 바로 검토 가능한 PR을 만들었습니다.<br>**토큰 절감: 약 10\~20%**<br>반복되는 Git과 PR 절차를 정해진 체크리스트로 처리했습니다.**<br>**
**직접 만든 스킬과 프로젝트 하네싱**<br>vscodereader가 직접 집필한 프로젝트 전용 스킬은 확인되지 않았습니다. 위 9개 항목은 저장소에서 사용한 Superpowers 계열 작업 방식입니다.**<br>**
**프로젝트에 적용된 자동화**<br>1. TaskCreated 이벤트를 GitHub 이슈로 연결<br>2. TaskCompleted 이벤트를 GitHub PR과 Closes 태그로 연결<br>3. worktree로 독립 작업 격리<br>4. blockedBy로 micro-task 의존성 관리<br>5. TDD와 커버리지 기준을 완료 조건으로 사용<br>이 하네싱의 최초 작성자는 pallas입니다. 이슈·브랜치·PR 정보를 반복 입력하는 작업을 없애 작업 묶음당 약 15\~30%를 절감했습니다. 필요한 파일과 완료 조건만 에이전트에 전달해 개별 작업당 약 20\~40%를 절감했습니다.**<br>**
**트러블슈팅<br>**
**1. 네이버 로그인 폼 v3 → v4 변경**<br>• **증상**: 자동 로그인이 폼 대기 단계에서 멈췄습니다.<br>• **원인**: 암호화 방식은 유지됐지만 버튼 ID와 준비 조건이 바뀌었습니다.<br>• **해결**: 기존 셀렉터를 보존하고 새 버튼과 CAPTCHA 셀렉터를 OR 조건으로 추가했습니다.**<br>**
**2. 신고 요청·쿠키·CAPTCHA 문제**<br>• **증상**: HTTP 400, 로그인 리다이렉트와 WASM CAPTCHA가 발생했습니다.<br>• **진단**: 요청·응답 원문과 브라우저 쿠키 상태를 기록했습니다.<br>• **해결**: 로그인 쿠키를 주입하고 브라우저가 정확한 사유와 제출 버튼을 선택해 직접 제출하도록 변경했습니다.**<br>**
**3. 삭제 기기의 과거 결과 노출**<br>• **문제**: 삭제한 기기의 과거 리포트가 결과 화면에 남았습니다.<br>• **해결**: 기록은 보존하고 조회 단계에서 현재 등록된 device_id만 반환했습니다.
**<br>날짜별 작업 일지**
**2026년 5월<br>프로젝트 시작**<br>• **05-28** — 종목토론방 패킷 기반 batch UI를 추가했습니다. 이슈 #45**<br>**
**2026년 6월<br>로그인·브라우저 자동화**<br>• **06-01** — 게시 엔진을 기존 큐·IPC에 이식하고 Playwright를 Rust CDP로 교체했습니다. PR #59\~#62<br>• **06-02** — CDP 입력과 프로필 권한을 강화하고 WSL endpoint 하드코딩을 제거했습니다.<br>• **06-04** — incognito, ADB IP 회전, 실제 키 이벤트와 로그인 스텔스를 구현했습니다. ADR-0010, PR #85\~#106<br>• **06-05** — 표준 adb CLI, Chrome 로그와 알림 읽음 처리를 정비했습니다. PR #107\~#121**<br>**
**로그·채널 자동화**<br>• **06-07** — 로그를 pstmacro.log로 통합하고 비밀번호 비노출 테스트를 추가했습니다. PR #128<br>• **06-08** — 밴드 로그인·가입·글·댓글의 HTTP 자동화와 md 서명 분석을 구현했습니다. PR #129\~#157<br>• **06-09** — 밴드 결과 기록, self-healing 로그와 Windows 콘솔 문제를 수정했습니다. PR #159\~#174<br>• **06-12** — 시장 탭, 게시 공통 셋업과 HTTP 429 백오프를 적용했습니다. PR #200\~#206<br>• **06-15** — 본문 링크와 종목명·종목코드·링크 변수 치환을 구현했습니다. PR #208\~#214<br>• **06-16** — 프로필 404와 완료 기록의 게시 내용·링크 누락을 해결했습니다. PR #215\~#224**<br>**
**큐·병렬 처리·IP 관리**<br>• **06-17** — 큐 우선순위, 중지·재개와 계정별 병렬 처리를 구현했습니다. PR #231\~#241<br>• **06-18** — 로그인 없이 실행하는 ADB IP 변경과 결과 확인을 구현했습니다. PR #242\~#251<br>• **06-22** — 특정 게시글 댓글과 여러 링크 작업 분배를 구현했습니다. PR #254\~#260<br>• **06-23** — 카페 다중 게시판의 게시 간격을 실험하고 요구에 따라 제거했습니다. PR #261\~#264<br>• **06-24** — 블로그 최신 글 댓글, 계정별 큐, 동시 작업과 publish modal 손상을 수정했습니다. PR #277\~#290**<br>**
**진단·원격제어 기반**<br>• **06-25** — 로그인 백트레이스·키 이벤트 게이트와 블로그 groupId를 수정했습니다. PR #304\~#311<br>• **06-26** — 봇 차단 대응 헤더, 오류 원문과 네이버 클립 댓글을 구현했습니다. PR #312\~#318<br>• **06-27** — 자동 로그인 플랫폼과 기존 선택 로그인 경로 재사용을 설계서에 확정했습니다.<br>• **06-28** — Admin 화면과 감사 통신 로그·txt 내보내기를 만들었습니다.<br>• **06-29** — 중앙 서버·하위 에이전트·Admin의 계정 분배·로그인·게시·예약을 구현했습니다. PR #323\~#330<br>• **06-30** — 재시도 표시, 빠른 실패, 큐 충돌과 다중 URL 회귀를 보완했습니다. PR #341\~#352**<br>**
**2026년 7월<br>패킷·세션 안정화**<br>• **07-01** — 패킷을 1:1로 대조해 UMON 403·500, npay 동의와 세션 쿠키 문제를 해결했습니다. PR #367\~#375<br>• **07-02** — 와이어 추적, 수동 로그인, 쿠키 만료 표시와 Admin 게시 명령을 구현했습니다. PR #376\~#381<br>• **07-03** — 좋아요 흐름, 차단·만료 판정, 실명인증과 죽은 계정 표시를 보완했습니다. PR #382\~#388**<br>**
**Admin·채널 기능 확장**<br>• **07-06** — 카페 계정 분배, 게시판 링크 명령, 특정글 댓글과 통신 기록을 구현했습니다.<br>• **07-07** — UA·언어·IP 대역 실험과 종목토론방 싫어요를 추가했습니다. PR #389\~#395<br>• **07-08** — UA·Client Hints 로테이션, WASM 실험과 PostgreSQL 초기화를 구성했습니다. PR #396\~#397<br>• **07-09** — 로그인 지문 추적, 조회수 부스트와 다중 댓글 분배를 구현했습니다. PR #399\~#406<br>• **07-10** — 닉네임 랜덤, 글 수정, 나눠서 게시와 백그라운드 edit를 통합했습니다. PR #407\~#409<br>• **07-12** — Admin 기타 명령과 흰 화면·edit 실패·raw 로그·잔여 횟수를 보완했습니다.<br>• **07-13** — 밴드 OAuth와 블로그 새 글 HTTP 클라이언트·작성 UI를 구현했습니다. PR #410\~#413**<br>**
**배포·다기기·관측성**<br>• **07-14** — 신고 직접 제출과 블로그 이미지·서명 URL·공개 범위를 수정했습니다. PR #422\~#429<br>• **07-15** — 신고·블로그·원격 미디어·Cloud Run과 채널별 회귀를 정리했습니다. PR #432\~#435<br>• **07-16** — 네이버 v4 폼, 분배 지연, 다기기와 계정 상태 화면을 수정했습니다. PR #436\~#438<br>• **07-20** — 통신 기록 필터, 안정적인 기기 식별, 종목 자동·수동 선택과 고아 리포트 처리를 구현했습니다. PR #440\~#448<br>[GitHub 저장소에서 전체 이력 확인](https://github.com/beyondsoft-kr/pstmacro)
패킷 캡처 시나리오와 확인한 요청 흐름
1. HTTP/2 패킷 선정 방법<br>• 시작 필터: Wireshark 표시 필터에 http2를 입력해 TLS 복호화 후의 HTTP/2 프레임만 남김.<br>• 시간 기준: 자동화하려는 버튼을 누른 시각과 가까운 프레임부터 확인함.<br>• 도메인 기준: :authority가 nid.naver.com, stock.naver.com, m.stock.naver.com, apis.naver.com, member-web.pay.naver.com인 요청을 우선 선택함.<br>• 경로 기준: :path에 login, profile, discussion, commentBox, join, reactions 등 구현할 기능명이 포함된 요청을 선택함.<br>• 제외 기준: pstatic 이미지, 광고, nlog·tivan 분석 요청, Chrome 백그라운드 통신은 기능 결과를 바꾸지 않아 제외함.<br>• 핵심 패킷: OPTIONS는 CORS 사전 확인으로 분리하고, POST·PUT·DELETE 및 같은 stream ID의 응답 HEADERS·DATA를 핵심으로 선정함.<br>• 사용 스킬: systematic-debugging의 재현→관측→가설 순서와 TShark CLI 추출을 사용함.
2. 선택한 패킷에서 본 내용과 의미<br>• \:method\: GET은 조회, POST는 생성·실행, PUT은 수정·취소, DELETE는 삭제를 의미함.<br>• \:authority\: 요청 대상 서버이며 쿠키 도메인과 Origin을 맞출 기준이 됨.<br>• \:path\: 실제 API 경로와 query string을 보여 주므로 기능 요청을 식별하는 핵심 기준임.<br>• Content-Type·Content-Length: DATA body의 JSON/form-urlencoded 형식과 전송 길이를 판단함.<br>• Origin·Referer: 요청이 어느 화면과 흐름에서 시작됐는지 판단함.<br>• Cookie: 로그인·세션 상태를 판단하되 값은 기록하지 않고 이름·도메인·만료 조건만 비교함.<br>• 응답: 같은 stream ID의 :status, location, set-cookie, content-type과 DATA의 JSON·HTML·오류 원문을 묶어 확인함.<br>• 구현 판단: URL뿐 아니라 필수 헤더, body 구조, 쿠키 범위와 리다이렉트 처리까지 재현해야 한다고 판단함.
3. 기능별 판단과 수정<br>• 로그인: GET /nidlogin.login → GET /login/dynamicEcKey/... → POST /nidlogin.login → GET /signin/v3/finalize 순서와 dynamicKey·eccpw·sessionKey를 확인함.<br>• 쿠키: NID_JKL=expired는 신규 저장값이 아니라 기존 쿠키 만료 지시이므로 쿠키 JSON에서 빠지는 것이 정상이라고 판단함.<br>• 약관: financial-service/join → callback → commonTermAgree 리다이렉트 순서를 구현함.<br>• 프로필: users/status와 users/form 결과에 따라 nickname/recommend → introduction/validate → POST users 또는 profile_id PUT으로 분기함.<br>• 글: smartEditor/token과 discussion/form 뒤 POST discussion/add를 호출하고, 수정은 form/edit 뒤 PUT discussion/edit로 구성함.<br>• 댓글: web_naver_token_json.json 뒤 web_naver_create_json.json을 호출하고 대상 글과 계정 상태를 사전 확인하도록 수정함.<br>• 사용 스킬: systematic-debugging으로 차이를 좁히고 test-driven-development로 분기와 상태 코드를 테스트로 고정함.
4. 가장 문제가 많았던 API와 판단<br>• 핵심 난점: 단순 게시 API가 아니라 로그인·세션·약관·프로필 상태와 CAPTCHA/WASM 조건이 결합된 인증 의존 API였음.<br>• 변동 조건: 같은 URL도 NID_AUT·NID_SES, nid.naver.com/.naver.com 도메인 범위, Origin·Referer와 계정 상태에 따라 응답이 달라짐.<br>• 확인 내용: :status뿐 아니라 응답 DATA 오류 원문, location, set-cookie, 요청 cookie 이름, ncpt token 요청, WASM fetch와 pstmacro.log의 동일 시각을 확인함.<br>• 원인 선정: 정상·실패 캡처에서 최초로 달라지는 요청을 원인 후보로 정하고, 그 이전까지 동일한 요청은 원인에서 제외함.<br>• 수정 내용: 쿠키 주입·만료 판정, 약관 callback, 부분 생성 프로필 복구, realNameCheck, CAPTCHA 감지, 429 백오프와 빠른 실패를 각각 독립 분기로 구현함.<br>• 반응 API: reactions POST 생성과 식별자를 포함한 PUT 취소를 서로 다른 동작으로 구분함.<br>• 사용 스킬: systematic-debugging으로 최초 차이를 찾고 receiving-code-review로 가설을 실제 패킷과 대조함.
5. 재검증 방법과 합격 기준<br>• 비교 대상 A: 사람이 Chrome에서 직접 성공시킨 원본 pcapng.<br>• 비교 대상 B: 같은 계정 상태·종목·동작을 PSTMACRO로 실행해 새로 캡처한 pcapng.<br>• 동일 조건: 두 캡처에 같은 TLS key log 설정과 http2 필터를 적용함.<br>• 1차 비교: TShark로 시간, frame number, stream ID, :method, :authority, :path, :status를 추출해 요청 순서를 1:1 대응함.<br>• 2차 비교: Content-Type, Origin, Referer, Cookie 이름·도메인, DATA 필드·JSON 구조·form 인코딩, 응답 location·set-cookie·오류 원문을 비교함.<br>• 동적 값 처리: 동적 키, 세션 값, timestamp, 게시물 ID는 값 자체가 아니라 존재 여부·형식·전달 위치를 비교함.<br>• 최종 확인: 실제 UI의 로그인, 프로필, 게시물·댓글·반응 결과와 pstmacro.log 완료 기록을 확인함.<br>• 재시도 절차: 최초로 달라지는 요청으로 돌아가 최소 수정한 뒤 동일 시나리오를 다시 캡처함.<br>• 합격 기준: 필수 요청 순서, 헤더·body 구조, 예상 status, 최종 UI 결과가 일치하고 비밀번호·쿠키 값·TLS 키가 로그에 노출되지 않아야 함.<br>• 사용 스킬: test-driven-development로 회귀 테스트를 작성하고 verification-before-completion으로 테스트, 패킷 diff와 UI 결과를 모두 확인함.
패킷 읽었던 법 — 실제 pcapng 복호화 기준
<details>
<summary>공통: http2 필터 후 패킷 읽는 순서</summary>
	1. keylogfile.txt를 Wireshark의 TLS Protocol 설정에 연결해 HTTPS를 복호화함.<br>2. 1차 필터에 http2를 입력함. 초록색은 GET·POST 색이 아니라 HTTP/2 패킷에 적용된 색상 규칙임.<br>3. 초록색을 전부 누르지 않음. Info 열에서 HEADERS\[n\]: GET ... 또는 HEADERS\[n\]: POST ...가 보이는 행만 먼저 확인함.<br>4. 더 줄이려면 http2.headers.method 필터를 사용함. POST만 볼 때는 http2.headers.method == "POST", GET만 볼 때는 http2.headers.method == "GET"을 사용함.<br>5. 로그인 예시는 http2.headers.authority == "[nid.naver.com](http://nid.naver.com)" && http2.headers.path contains "nidlogin"으로 후보를 줄임.<br>6. 글쓰기 예시는 http2.headers.authority == "[m.stock.naver.com](http://m.stock.naver.com)" && http2.headers.path contains "discussion"으로 후보를 줄임.<br>7. 후보 HEADERS 행을 선택한 뒤 우클릭 → 따라가기 → HTTP/2 스트림을 실행함. 그러면 Wireshark가 [tcp.stream](http://tcp.stream) eq N and http2.streamid eq M 형태로 같은 대화만 남김.<br>8. Follow HTTP/2 Stream의 빨간색은 클라이언트→서버 요청, 파란색은 서버→클라이언트 응답 방향임. GET과 POST 모두 요청이므로 빨간색에 나타날 수 있으며 색으로 메서드를 구분하지 않음.<br>9. 빨간 요청에서 :method, :authority, :scheme, :path, 일반 헤더를 읽고 이어지는 DATA에서 POST·PUT body를 확인함.<br>10. 파란 응답에서 :status, content-type, location, set-cookie와 DATA의 HTML·JSON을 확인함.<br>11. 이미지 예시처럼 :authority가 이미지 서버이고 accept·content-type이 image/jpeg이며 sec-fetch-dest가 image이면 화면 리소스이므로 기능 API에서 제외함.<br>12. 결론: 일일이 모든 초록색 패킷을 누르는 방식이 아니라 method·authority·path로 후보를 줄인 뒤 필요한 HEADERS 한 건에서 HTTP/2 Stream 따라가기를 사용함.
</details>
<details>
<summary>로그인 GET·POST와 복호화된 body</summary>
	<details>
	<summary>프로필 생성·수정 GET·POST·PUT body</summary>
		- GET /api/community/profile/users/status: 프로필 존재 여부와 현재 사용자 상태를 조회함.<br>• GET /api/community/profile/users/form: 부분 생성된 프로필의 수정용 데이터를 조회함.<br>• POST /nickname/recommend: 추천 닉네임을 요청하고 HTTP 200 응답을 확인함.<br>• POST /introduction/validate: application/json DATA에 targetValue가 들어 있음을 확인함.<br>• POST /api/community/profile/users: DATA에 nickname, introduction, imageUrl, danglingImages가 들어 있고 HTTP 201로 생성됨을 확인함.<br>• PUT /api/community/profile/users/\{profile_id\}: 같은 네 필드로 기존 프로필을 수정함.<br>• 판단: status 결과에 따라 신규 POST와 기존 PUT을 분기하고, 500이면 realNameCheck 흐름을 확인하도록 구현함.
	</details>
	<details>
	<summary>종목토론방 글쓰기·수정 GET·POST·PUT body</summary>
		- GET /front-api/discussion/smartEditor/token: 글 작성에 필요한 편집기 토큰을 조회함.<br>• POST /front-api/discussion/form: 종목과 게시 유형을 보내 txId, itemCode, itemName, 연결 상태를 포함한 준비 응답을 받음.<br>• POST /front-api/discussion/add: DATA에 title, document, documentId, contentJson, isCleanbotDisabled, danglingImages, discussionType, itemCode, txId, inflow가 들어 있음을 복호화함.<br>• PUT /front-api/discussion/edit: title, document, documentId, contentJson, isCleanbotDisabled, danglingImages로 기존 글을 수정함.<br>• 댓글 POST /commentBox/cbox/web_naver_create_json.json: form-urlencoded DATA에 objectId, objectUrl, contents, cbox_token, pageType, listType, clientType 등이 들어 있음을 확인함.<br>• 반응 POST·PUT /reactions: application/json DATA의 reactionType으로 생성과 변경·취소를 구분함.<br>• 판단: 준비 단계의 동적 토큰과 txId를 실제 add·edit·comment 요청에 전달해야 함.
	</details>
	<details>
	<summary>약관·실명 인증·CAPTCHA/WASM 패킷</summary>
		- 약관 캡처: GET /financial-service/join → GET /user2/help/commonTermAgree → GET /financial-service/join/naver-term-consent/callback 순서를 확인함.<br>• 약관 GET query: 동의 상태와 성공·실패 이동 URL을 전달하며, callback의 session_id로 같은 동의 흐름을 연결함.<br>• 실명 인증 캡처: GET /user2/help/realNameCheck와 GET /user2/help/realNameCheck.nhn이 프로필 생성 실패 뒤 나타남을 확인함.<br>• CAPTCHA/WASM 캡처: GET /static/ncaptcha-api.js, GET /login/js/v3/default/rcaptcha_ecc.js, GET [rcaptcha.nid.naver.com/question·rcapt.js·rcaptCss·rcaptUi를](http://rcaptcha.nid.naver.com/question·rcapt.js·rcaptCss·rcaptUi를) 확인함.<br>• CAPTCHA 판정: 단순 로그인 실패가 아니라 ncpt token 요청과 rcaptcha 리소스가 함께 발생하는지를 확인함.<br>• 구현 판단: 약관 미동의, 실명 확인 필요, CAPTCHA 발생을 하나의 오류로 묶지 않고 각각 다른 분기와 사용자 상태로 처리함.
	</details>
	- GET /nidlogin.login: 로그인 HTML 폼과 NID_JST 쿠키를 받는 요청으로 확인함.<br>• GET /login/dynamicEcKey/...: 로그인 폼의 동적 암호화 키를 받는 요청으로 확인함.<br>• POST /nidlogin.login: Content-Type은 application/x-www-form-urlencoded이며 DATA에 localechange, dynamicKey, eccpw, sessionKey, enctp, next_step, show_pk, wtoken, svctype, template_type, smart_LEVEL, bvsd, locale, url, id, pw 필드가 들어 있음을 복호화해 확인함.<br>• POST 응답: HTTP 200과 Set-Cookie를 확인하고 다음 단계 /signin/v3/finalize로 이동하는 HTML을 확인함.<br>• GET /signin/v3/finalize: 발급된 세션을 확정하고 최종 목적지로 이동시키는 요청으로 판단함.<br>• 판단: ID/PW 평문 전송이 아니라 dynamicKey·eccpw·sessionKey를 포함한 폼 제출과 리다이렉트·쿠키 저장 전체를 구현해야 함.<br>• 보안: id, pw, eccpw, sessionKey, dynamicKey와 쿠키 값은 분석만 하고 Notion에는 값 자체를 기록하지 않음.
</details>
# PSTMACRO — 사용 기술과 실제 적용 위치
2026-09-10 저장소 및 전체 브랜치 조사 기준으로 작성함. 프로젝트 전체 기술과 본인 작성 PR은 구분하며, 상세 기여 근거는 PR별 기록과 커밋 이력에 연결함.
## 1. 프로젝트 개요와 구현 구조
여러 Windows PC에서 네이버·밴드 계정의 로그인, 게시, 댓글, 작업 예약을 실행하고 중앙 Admin에서 기기·계정·명령·결과를 관리하는 시스템을 개발함. 브라우저 정상 동작의 패킷을 관찰하여 HTTP 요청 계약을 파악하고, 브라우저 조작이 필요한 구간은 CDP로 처리함.
화면은 `src/`, Windows 실행 계층은 `src-tauri/`, 중앙 서버는 `server/`로 분리함. Admin은 `src/admin/`에서 별도 화면으로 구성함. 원본 기본 브랜치는 `master`이며 실험·미병합 브랜치의 내용을 기본 브랜치에 병합된 것으로 서술하지 않음.
## 2. 화면·데스크톱 기술
### React 19·TypeScript
- 적용 위치: `src/features/`, `src/admin/features/`, `src/admin/api.ts`.
- 계정 목록, 로그인 상태, 종목 선택, 게시 설정, 큐, 기기별 통신 로그와 결과 보고 화면에 사용함.
- 화면 입력을 타입으로 정의하고 API·IPC 응답을 화면 상태로 변환함. 자동 종목 선택과 수동 선택은 공통 게시 입력 계약을 유지하면서 입력을 만드는 단계에서 구분함.
- Admin의 종목 선택 모달은 데스크톱과 공통 화면을 재사용하고 데이터 공급 어댑터만 분리하여 같은 선택 규칙이 다르게 구현되지 않도록 함.
### Mantine·Vite
- Mantine은 공용 입력·모달·알림·레이아웃 구성에 사용함. Vite는 React 개발 서버와 배포 번들 생성에 사용함.
- 적용 근거: 루트 `package.json`, `vite.config.ts`, `src/admin/main.tsx`.
- 실제 채널 통신은 UI 컴포넌트에 넣지 않고 Rust 도메인 또는 서버 API로 위임함.
### Tauri 2·Rust·IPC
- 적용 위치: `src-tauri/src/lib.rs`, `src-tauri/src/ipc/`, `src-tauri/Cargo.toml`.
- React의 명령을 Rust 함수로 전달하여 계정 저장, 브라우저 시작, 게시 실행, Excel 처리와 작업 큐를 구동함.
- Tauri 플러그인은 파일 선택, 자동 시작, 단일 인스턴스, 시스템 알림과 트레이 상주를 담당함.
- `serde`·`serde_json`으로 명령 데이터를 직렬화하고 `ts-rs`와 `gen:bindings`로 Rust 타입을 TypeScript에 전달하는 구성을 둠.
## 3. 패킷 분석·HTTP·브라우저 제어
### Wireshark·TShark·TLS key log
- 사용자가 직접 Chrome 동작을 캡처하고 `D:\packet_copy`의 시나리오별 pcapng와 TLS key log로 HTTP/2 내용을 확인한 개발 방식임.
- Wireshark의 `http2` 필터로 시작하여 요청 HEADERS, 메서드, 도메인, 경로를 확인하고 같은 TCP 연결·HTTP/2 stream의 요청과 응답을 묶음.
- TShark는 GUI에서 찾은 요청의 프레임 번호·stream·헤더·DATA를 반복 추출하고 정상·실패 흐름을 대조하는 데 사용함.
- TLS 복호화 결과에서 JSON·form-urlencoded 필드와 리다이렉트를 확인함. TLS 전송 암호화와 서버의 계정 저장 암호화는 서로 다른 계층임.
- 캡처와 키 파일은 실행 코드 의존성이 아니며 Git 이력 복사만으로 별도 디스크의 캡처가 자동 포함되는 것은 아님.
### reqwest·rustls·응답 압축 처리
- 적용 위치: `naver_automation/packet_client.rs`, `naver_blog/`, `naver_cafe/`, `band_post/client.rs`.
- 관측한 API의 URL, 헤더, 쿠키, JSON·폼 body를 Rust HTTP 클라이언트로 구성함.
- Cargo에서 gzip·brotli·deflate·zstd·multipart 기능을 켜 압축 응답과 파일 업로드를 처리함. Accept-Encoding 문자열만 흉내 내지 않고 실제 해제 기능도 갖추도록 함.
- 상태 코드만으로 성공을 판정하지 않고 오류 body, 인증 상태, 리다이렉트와 최종 결과를 함께 확인하는 진단 흐름에 사용함.
### Chrome DevTools Protocol·WebSocket
- 적용 위치: `auth/chrome.rs`, `auth/login_flow.rs`, `naver_automation/devtools_connection.rs`, `browser_flow.rs`.
- 브라우저 시작, 페이지·DOM 상태 확인, 입력·클릭, 브라우저 쿠키와 네트워크 상태 확인에 사용함.
- 네이버 로그인 폼 변경에는 셀렉터와 준비 조건을 수정하고, 게시·신고에서 브라우저 제출이 필요한 경로는 해당 흐름으로 처리함.
- `tungstenite`는 CDP WebSocket 통신에 사용함. 원격 Admin 명령 전달에 사용하는 SSE와 역할이 다름.
- WASM Fetch 차단은 `feat/wasm-fetch-block-experiment`에 존재하는 실험으로 기록하며 일반적인 인증 해결책 또는 운영 반영 완료로 표현하지 않음.
### ADB
- 적용 위치: `src-tauri/src/auth/adb.rs`.
- Android 기기 연결과 네트워크 전환을 호출하는 CLI 연동에 사용함. 로그인 작업과 별도로 실행 가능한 기기 작업을 구성하고 명령 결과를 확인함.
### 밴드 HMAC 서명·네이버 에디터 문서 변환
- 밴드: `band_post/signature.rs`에서 HMAC-SHA256과 Base64를 사용하여 요청 서명 형식을 처리함. 로그인 쿠키 관리와 게시 요청 생성은 별도 모듈로 분리함.
- 블로그: `naver_blog/document_model.rs`, `editor_api.rs`, `write_client.rs`에서 에디터 문서 구조, 미디어와 게시 요청을 처리함.
- 카페: `naver_cafe/article_list/`, `cafe_ref/`, `comment/`에서 카페·게시판 식별과 글 목록·댓글 요청을 구분함. 최신 글을 카페 전체가 아닌 지정 게시판으로 좁히는 수정 이력이 있음.
## 4. 중앙 서버·데이터·인증
### Axum·Tokio·SSE
- 적용 위치: `server/src/main.rs`, `routes.rs`, `hub.rs`, `scheduled.rs`, `src-tauri/src/agent/net.rs`.
- Axum으로 운영자·기기 API를 제공하고 Tokio로 비동기 요청, 예약과 이벤트 전달을 처리함.
- `hub.rs`는 device_id별 SSE 송신 채널과 Admin 브로드캐스트를 유지함. 연결이 끊긴 기기의 명령은 제한된 대기 큐에 보관하고 재구독 시 전달함.
- 코드의 `MAX_PENDING`은 128이며, 재연결하지 않는 기기의 대기열이 계속 증가하는 것을 제한하는 기술 상한임.
- 단순 전송 성공과 실제 실행 완료를 구분하기 위해 하위의 결과 보고를 Admin 화면에 연결함.
### PostgreSQL·SQLx·repository trait
- 적용 위치: `server/src/repo/postgres.rs`, `repo/memory.rs`, `repo/mod.rs`.
- 운영자·기기·계정·작업과 결과를 PostgreSQL에 보관하고 SQLx로 접근함.
- 동일한 repository 계약의 메모리 구현을 테스트에 사용하여 실제 DB 없이 분배·조회 정책을 검증함.
- 기기 식별은 재설치마다 새 ID를 생성하는 대신 안정적인 machine 식별값으로 upsert하는 수정 이력이 있음.
- 삭제 기기의 과거 결과는 저장 이력을 지우는 방식이 아니라 현재 등록 기기를 기준으로 조회를 제한하는 방식으로 보완함.
### Argon2id·AES-256-GCM·JWT
- 적용 위치: `server/src/crypto.rs`, `jwt.rs`.
- 운영자 비밀번호는 Argon2id와 salt로 단방향 해시하여 로그인 검증에 사용함.
- 하위에 분배할 계정 ID/PW는 복원이 필요하므로 AES-256-GCM으로 저장 암호화함. 코드에서 32바이트 키와 매번 생성한 12바이트 nonce를 사용하고 결과를 Base64로 저장함.
- JWT는 인증된 요청의 자격 확인에 사용함. 비밀번호 해시·저장 암호화·TLS를 동일한 기능으로 설명하지 않음.
- 코드의 암호화 왕복·다른 키 거부·nonce 변경 테스트가 존재함. 이번 문서화 작업에서 운영 계정 로그인이나 원격 작업을 새로 실행한 것은 아님.
## 5. 데이터 입출력·품질·배포
- Excel: `calamine`으로 입력을 읽고 `rust_xlsxwriter`로 출력함. 적용 진입점은 `src-tauri/src/ipc/excel.rs`임.
- 로그: `tracing`, `tracing-subscriber`, `tracing-appender`와 `src-tauri/src/logging.rs`로 진단 정보를 기록함. 화면의 통신 로그 필터와 런타임 로그는 각각 사용 목적에 맞게 연결함.
- 테스트: Vitest·Testing Library는 화면과 TypeScript 로직, Cargo test는 Rust 도메인, wiremock은 외부 HTTP 계약 검증에 사용함.
- 정적 검사: TypeScript, ESLint, Prettier, Stylelint, rustfmt·Clippy를 사용하며 Husky·lint-staged·commitlint로 커밋 전 검사를 구성함.
- 배포: Docker·Cloud Run 관련 설정과 GitHub Actions가 존재함. Windows 크로스 빌드와 PDB 배포 설정은 릴리스에서도 오류 위치를 추적하기 위한 목적임.
## 6. 개인 기여와 검증 자료
본인 기여는 `vscodereader` 작성 PR 및 Git author 이력에서 확인함. 팀 전체 구조를 본인이 단독 작성한 것으로 치환하지 않음. 전체 PR 목록, 본인 PR 본문·변경 파일·리뷰·커밋, 원격·로컬 브랜치와 미커밋 스냅샷을 개인 저장소 문서에서 각각 조회하도록 구성함. 기존 Notion 패킷 분석 설명은 그대로 두고 본 기술별 적용 설명을 보완함.
## 개인 저장소와 전체 기여 근거
- [PST-packet-reader — 코드·이력·브랜치·상세 문서](https://github.com/vscodereader/PST-packet-reader)
- [전체 PR·개인 기여 색인](https://github.com/vscodereader/PST-packet-reader/blob/portfolio/portfolio/PR-INDEX.md)
- [미커밋 작업과 변경 경로](https://github.com/vscodereader/PST-packet-reader/blob/portfolio/portfolio/WORK-IN-PROGRESS.md)
- 환경 파일을 전체 이력에서 제외하고 원격 브랜치·로컬 브랜치·미커밋 스냅샷·과거 PR head를 보존함. 원본 해시 변경은 portfolio의 대응표에 기록함.
- PR 본문·변경 파일·커밋·리뷰·대화 댓글은 개인 저장소에 보존함. 개인 작성 PR 218건을 아래에서 날짜별로 확인할 수 있음.
<details>
<summary>2026-05 개인 PR 전체</summary>
	- 2026-05-28 · **CLOSED** · [#45 네이버 증권 토론 자동화 패킷 기반 batch UI 추가](https://github.com/beyondsoft-kr/pstmacro/pull/45)
</details>
<details>
<summary>2026-06 개인 PR 전체</summary>
	- 2026-06-01 · **MERGED** · [#60 feat(forum): port naver discussion packet posting engine to master](https://github.com/beyondsoft-kr/pstmacro/pull/60)
	- 2026-06-01 · **MERGED** · [#62 feat(auth): switch login to Rust CDP, remove Playwright sidecar](https://github.com/beyondsoft-kr/pstmacro/pull/62)
	- 2026-06-02 · **CLOSED** · [#69 test(auth): make chrome_path fallback assertion host-independent](https://github.com/beyondsoft-kr/pstmacro/pull/69)
	- 2026-06-04 · **MERGED** · [#78 chore: dev config — ignore personal notes, align toolchain pins](https://github.com/beyondsoft-kr/pstmacro/pull/78)
	- 2026-06-04 · **CLOSED** · [#81 test: add regression suite for #60 review fixes](https://github.com/beyondsoft-kr/pstmacro/pull/81)
	- 2026-06-04 · **MERGED** · [#82 test(auth): cover login signal confirmation (2-poll latch guard)](https://github.com/beyondsoft-kr/pstmacro/pull/82)
	- 2026-06-04 · **MERGED** · [#83 fix(accounts): make empty password cell clickable with placeholder](https://github.com/beyondsoft-kr/pstmacro/pull/83)
	- 2026-06-04 · **MERGED** · [#84 fix(auth): pace login typing (2s/1s) and log airplane toggle](https://github.com/beyondsoft-kr/pstmacro/pull/84)
	- 2026-06-04 · **MERGED** · [#85 fix(auth): incognito login, 1s airplane toggle, log IP before/after](https://github.com/beyondsoft-kr/pstmacro/pull/85)
	- 2026-06-04 · **MERGED** · [#87 fix(auth): pace login 2s/2s/2s and send Shift with uppercase keys](https://github.com/beyondsoft-kr/pstmacro/pull/87)
	- 2026-06-04 · **MERGED** · [#88 fix(forum): auto-launch debug Chrome for posting (no manual 9222)](https://github.com/beyondsoft-kr/pstmacro/pull/88)
	- 2026-06-04 · **MERGED** · [#90 feat(auth): log Chrome lifecycle and settle after IP rotation](https://github.com/beyondsoft-kr/pstmacro/pull/90)
	- 2026-06-04 · **MERGED** · [#93 fix(ui): refresh sidebar badge counts on navigation](https://github.com/beyondsoft-kr/pstmacro/pull/93)
	- 2026-06-04 · **MERGED** · [#95 fix(auth): wait for full login-form load before typing, with logs](https://github.com/beyondsoft-kr/pstmacro/pull/95)
	- 2026-06-04 · **MERGED** · [#102 fix(auth): CDP 로그인 봇탐지(ncaptcha) 통과 — 실제 키 이벤트 + 스텔스 보강](https://github.com/beyondsoft-kr/pstmacro/pull/102)
	- 2026-06-04 · **CLOSED** · [#106 docs(adr): CDP 스텔스 보강 설계 (ADR-0010)](https://github.com/beyondsoft-kr/pstmacro/pull/106)
	- 2026-06-05 · **MERGED** · [#107 fix(auth): IP 로테이션 비행기모드 토글 복구 — raw-USB→표준 adb CLI (Zadig 불필요)](https://github.com/beyondsoft-kr/pstmacro/pull/107)
	- 2026-06-05 · **MERGED** · [#108 docs(auth): useAdb 플래그 주석 — IP 로테이션 켜려면 true로](https://github.com/beyondsoft-kr/pstmacro/pull/108)
	- 2026-06-05 · **MERGED** · [#109 docs(adr): ADR-0010 CDP 스텔스 보강 기록 (Accepted)](https://github.com/beyondsoft-kr/pstmacro/pull/109)
	- 2026-06-05 · **MERGED** · [#111 chore(logging): 상태 로그를 파일·콘솔에도 남기기 (eprintln→tracing)](https://github.com/beyondsoft-kr/pstmacro/pull/111)
	- 2026-06-05 · **MERGED** · [#114 feat(notifications): 알림 원문 보기 + 글관리 배지 draft 제외](https://github.com/beyondsoft-kr/pstmacro/pull/114)
	- 2026-06-05 · **MERGED** · [#119 feat(forum): 종목 선택 라이브 네이버 검색 연결 + 크롤 알림 제거](https://github.com/beyondsoft-kr/pstmacro/pull/119)
	- 2026-06-05 · **MERGED** · [#121 fix(ui): 벨 알림 빨간 점 읽음 추적으로 정상화](https://github.com/beyondsoft-kr/pstmacro/pull/121)
	- 2026-06-08 · **MERGED** · [#129 feat(logging): 로그 pstmacro.log 일원화 + 가독성·상세화](https://github.com/beyondsoft-kr/pstmacro/pull/129)
	- 2026-06-08 · **MERGED** · [#140 feat: 네이버 밴드(band.us) 이메일 로그인 (CDP 스텔스, 네이버 미러링)](https://github.com/beyondsoft-kr/pstmacro/pull/140)
	- 2026-06-08 · **CLOSED** · [#151 feat: 네이버 밴드(band.us) 가입·글쓰기·댓글 (순수 HTTP, md 서명 역공학)](https://github.com/beyondsoft-kr/pstmacro/pull/151)
	- 2026-06-09 · **MERGED** · [#159 feat(forum): 종목 선택 화면 네이버 모바일 재디자인](https://github.com/beyondsoft-kr/pstmacro/pull/159)
	- 2026-06-09 · **CLOSED** · [#160 fix(notifications): build_publish_batch clippy too_many_arguments 정리](https://github.com/beyondsoft-kr/pstmacro/pull/160)
	- 2026-06-09 · **MERGED** · [#167 fix(posts): 게시 계정 선택 체크박스 직접 클릭이 선택에 반영되도록](https://github.com/beyondsoft-kr/pstmacro/pull/167)
	- 2026-06-09 · **MERGED** · [#168 feat(logging): 로그 파일을 지우거나 비워도 이후 작업이 자동 재기록(self-healing)](https://github.com/beyondsoft-kr/pstmacro/pull/168)
	- 2026-06-09 · **MERGED** · [#170 fix(posts): 종목 선택 체크박스 직접 클릭이 즉시 반영되도록](https://github.com/beyondsoft-kr/pstmacro/pull/170)
	- 2026-06-09 · **MERGED** · [#171 fix(adb): 윈도우에서 adb 실행 시 콘솔 창 깜빡임 제거(CREATE_NO_WINDOW)](https://github.com/beyondsoft-kr/pstmacro/pull/171)
	- 2026-06-09 · **MERGED** · [#174 feat: 네이버 밴드(band.us) 가입·글쓰기·댓글 + 결과로그·알림기록 (순수 HTTP, md 서명 역공학)](https://github.com/beyondsoft-kr/pstmacro/pull/174)
	- 2026-06-12 · **MERGED** · [#201 feat(stocks): 종목 선택에 전체/코스피/코스닥 시장 구분 탭 추가](https://github.com/beyondsoft-kr/pstmacro/pull/201)
	- 2026-06-12 · **MERGED** · [#204 fix(forum): 글쓰기 form/add 429 재시도로 다종목 게시 일부 실패 해결](https://github.com/beyondsoft-kr/pstmacro/pull/204)
	- 2026-06-12 · **MERGED** · [#206 refactor(forum): 글/글+댓글 매크로 공통 셋업 단일화](https://github.com/beyondsoft-kr/pstmacro/pull/206)
	- 2026-06-15 · **MERGED** · [#209 fix(publish): 본문 링크가 게시 시 사라지는 문제 수정 (붙여넣기·서식링크)](https://github.com/beyondsoft-kr/pstmacro/pull/209)
	- 2026-06-15 · **MERGED** · [#211 fix(publish): 즉시 게시 완료 문구를 큐 등록 안내로 수정](https://github.com/beyondsoft-kr/pstmacro/pull/211)
	- 2026-06-15 · **MERGED** · [#214 feat(publish): 게시 시 변수 토큰(#\{종목명\}/#\{종목코드\}/#\{링크\}) 실제 치환](https://github.com/beyondsoft-kr/pstmacro/pull/214)
	- 2026-06-16 · **MERGED** · [#216 fix(forum): 글쓰기 전 프로필 셋업 + 신규 계정 프로필 POST 생성으로 form 404 해결](https://github.com/beyondsoft-kr/pstmacro/pull/216)
	- 2026-06-16 · **MERGED** · [#218 feat(forum): 종목별 게시 내용(제목/본문/댓글/링크)을 완료 로그에 확인](https://github.com/beyondsoft-kr/pstmacro/pull/218)
	- 2026-06-16 · **CLOSED** · [#221 feat(band): 밴드 게시 내용·글 링크를 완료 로그에 노출](https://github.com/beyondsoft-kr/pstmacro/pull/221)
	- 2026-06-16 · **MERGED** · [#224 fix(band): 밴드 완료 로그 '게시 내용'에 제목·본문·댓글 채우기](https://github.com/beyondsoft-kr/pstmacro/pull/224)
	- 2026-06-17 · **MERGED** · [#231 feat(queue): 게시 큐 자동 우선순위 — 로그인 1순위·종목토론방 2순위 재정렬 (1차)](https://github.com/beyondsoft-kr/pstmacro/pull/231)
	- 2026-06-17 · **MERGED** · [#233 feat(queue): 실행 중 카페/밴드 작업 우선순위 선점 중지·재개 (2차)](https://github.com/beyondsoft-kr/pstmacro/pull/233)
	- 2026-06-17 · **MERGED** · [#234 fix(queue): 종토방 큐 실행 재로그인 제거 + 게시 진행률 분모 버그 수정](https://github.com/beyondsoft-kr/pstmacro/pull/234)
	- 2026-06-17 · **MERGED** · [#235 fix(naver): 본문·댓글 줄바꿈 유실 — 여러 줄이 한 줄로 붙어 게시되던 문제](https://github.com/beyondsoft-kr/pstmacro/pull/235)
	- 2026-06-17 · **MERGED** · [#236 fix(notifications): 올라간 글 '열기' 버튼 대신 링크 주소를 그대로 노출](https://github.com/beyondsoft-kr/pstmacro/pull/236)
	- 2026-06-17 · **MERGED** · [#238 feat(queue): 종목토론방 게시 계정별 병렬 처리 (카페·밴드 무손상)](https://github.com/beyondsoft-kr/pstmacro/pull/238)
	- 2026-06-17 · **MERGED** · [#239 fix(build): fresh 윈도우 빌드 시 pstmacro.pdb 리소스 누락 빌드 실패 수정](https://github.com/beyondsoft-kr/pstmacro/pull/239)
	- 2026-06-17 · **MERGED** · [#241 feat(queue): 종목토론방 아이템 동시 실행 (워커 다중 처리, 카페·밴드 무손상)](https://github.com/beyondsoft-kr/pstmacro/pull/241)
	- 2026-06-18 · **MERGED** · [#242 fix(accounts): '선택 로그인 (N)' 카운트 정확화](https://github.com/beyondsoft-kr/pstmacro/pull/242)
	- 2026-06-18 · **MERGED** · [#248 feat(accounts): 로그인 없이 IP만 회전하는 'IP 변경' 버튼](https://github.com/beyondsoft-kr/pstmacro/pull/248)
	- 2026-06-18 · **MERGED** · [#251 fix(adb): 비행기모드 토글 상태확인·대기 3초 + IP 변경 결과 토스트·알림 표시](https://github.com/beyondsoft-kr/pstmacro/pull/251)
	- 2026-06-22 · **MERGED** · [#255 feat(forum): 종목토론방 '특정 게시글' 댓글을 종목선택 없이 글 URL에 직접 단다](https://github.com/beyondsoft-kr/pstmacro/pull/255)
	- 2026-06-22 · **MERGED** · [#256 fix(forum): 특정 게시글 댓글 성공 시 '게시내용'에 단 글의 링크도 표시](https://github.com/beyondsoft-kr/pstmacro/pull/256)
	- 2026-06-22 · **MERGED** · [#258 feat(band): 밴드 '특정 게시글' 댓글을 글 URL의 그 게시물에 직접 단다](https://github.com/beyondsoft-kr/pstmacro/pull/258)
	- 2026-06-22 · **MERGED** · [#260 feat(posts): 특정 게시글 댓글에 대상 링크 여러 개(댓글 갯수) 추가](https://github.com/beyondsoft-kr/pstmacro/pull/260)
	- 2026-06-23 · **MERGED** · [#262 feat(cafe): 여러 게시판 글을 4초 간격으로 모두 게시](https://github.com/beyondsoft-kr/pstmacro/pull/262)
	- 2026-06-23 · **MERGED** · [#263 fix(cafe): 여러 게시판 글 간격을 4초에서 11초로 조정](https://github.com/beyondsoft-kr/pstmacro/pull/263)
	- 2026-06-23 · **MERGED** · [#264 fix(cafe): 여러 게시판 글 사이 간격 기능 제거](https://github.com/beyondsoft-kr/pstmacro/pull/264)
	- 2026-06-24 · **MERGED** · [#268 feat: 원격제어 준비 전 14건 기능/버그 수정 묶음](https://github.com/beyondsoft-kr/pstmacro/pull/268)
	- 2026-06-24 · **MERGED** · [#269 docs(auth): 로그인 추가 인증 즉시 실패 안내 문구 정리](https://github.com/beyondsoft-kr/pstmacro/pull/269)
	- 2026-06-24 · **MERGED** · [#270 fix(posts): 줄바꿈·대기상태·알림·로그인 후속 수정 (#267)](https://github.com/beyondsoft-kr/pstmacro/pull/270)
	- 2026-06-24 · **MERGED** · [#272 fix(auth): 로그인 캡차 회귀 — IP 회전 후 연결 안정화 대기 복구(1→3초)](https://github.com/beyondsoft-kr/pstmacro/pull/272)
	- 2026-06-24 · **MERGED** · [#273 chore: .husky/_ 추적 해제 (clone 시 훅 깨짐 수정)](https://github.com/beyondsoft-kr/pstmacro/pull/273)
	- 2026-06-24 · **MERGED** · [#274 feat(blog): 네이버블로그 플랫폼 추가 — 댓글 게시(로그인·카페 흐름 재사용)](https://github.com/beyondsoft-kr/pstmacro/pull/274)
	- 2026-06-24 · **MERGED** · [#275 fix(auth): 로그인 대기를 ADB 확인·DOM 기반으로 단축 (사수 지시)](https://github.com/beyondsoft-kr/pstmacro/pull/275)
	- 2026-06-24 · **MERGED** · [#276 fix(adb): 비행기모드 ON 3초 hold 복원 — IP 미변경 회귀 수정](https://github.com/beyondsoft-kr/pstmacro/pull/276)
	- 2026-06-24 · **MERGED** · [#277 fix(adb): 비행기모드 토글 맥락별 분기 — 버튼 3초·로그인 ADB확인 즉시](https://github.com/beyondsoft-kr/pstmacro/pull/277)
	- 2026-06-24 · **MERGED** · [#278 fix(adb): 로그인 IP 회전을 실제 상태 기반으로 (끊김·IP변경 확인)](https://github.com/beyondsoft-kr/pstmacro/pull/278)
	- 2026-06-24 · **MERGED** · [#280 feat(blog): '최신 N개 글에 댓글' 모드 + 글 부족 시 즉시 취소](https://github.com/beyondsoft-kr/pstmacro/pull/280)
	- 2026-06-24 · **MERGED** · [#281 fix(blog): 글 목록 응답의 무효 escape(') 정리 후 파싱](https://github.com/beyondsoft-kr/pstmacro/pull/281)
	- 2026-06-24 · **MERGED** · [#283 refactor(posts): '나눠서 즉시 게시'를 계정 1개당 큐 1개로 분리](https://github.com/beyondsoft-kr/pstmacro/pull/283)
	- 2026-06-24 · **MERGED** · [#285 feat(queue): 동시 작업 수 무제한화 + 최대 작동가능 작업 수 설정](https://github.com/beyondsoft-kr/pstmacro/pull/285)
	- 2026-06-24 · **MERGED** · [#289 feat: 원격제어 준비 전 후속 수정 6건 (큐 가시성·도중차단·로그인 DOM·게시모달·캡차 보류)](https://github.com/beyondsoft-kr/pstmacro/pull/289)
	- 2026-06-24 · **MERGED** · [#290 fix(posts): publish-modal NUL 바이트 손상 복구 (#289 회귀)](https://github.com/beyondsoft-kr/pstmacro/pull/290)
	- 2026-06-25 · **MERGED** · [#291 fix(auth): 로그인 결과 폴링도 DOM 전부 로드 후 판정 (사수 지시)](https://github.com/beyondsoft-kr/pstmacro/pull/291)
	- 2026-06-25 · **MERGED** · [#292 fix(posts): 차단·대기초과 계정도 게시 계정 목록에서 숨김 (대기와 동일)](https://github.com/beyondsoft-kr/pstmacro/pull/292)
	- 2026-06-25 · **MERGED** · [#293 feat(accounts): 네이버블로그도 선택 로그인 지원 (종토방 흐름 재사용)](https://github.com/beyondsoft-kr/pstmacro/pull/293)
	- 2026-06-25 · **MERGED** · [#294 fix: 로그인 '돔 전부 로드' 엄격 게이트 + 게시큐 결과카드 제거(알림에서 확인)](https://github.com/beyondsoft-kr/pstmacro/pull/294)
	- 2026-06-25 · **MERGED** · [#295 chore(queue): 게시큐 완료/결과 카드 렌더 코드 완전 제거](https://github.com/beyondsoft-kr/pstmacro/pull/295)
	- 2026-06-25 · **MERGED** · [#296 fix(auth): 캡차 첫=즉시 보류 / 보류 재로그인=직접입력(120초), 비번·차단 즉시](https://github.com/beyondsoft-kr/pstmacro/pull/296)
	- 2026-06-25 · **MERGED** · [#297 docs(auth): decide_loop_step 주석 갱신(옛 latch 설명 제거)](https://github.com/beyondsoft-kr/pstmacro/pull/297)
	- 2026-06-25 · **MERGED** · [#298 fix(auth): 비밀번호 자동입력 검증 race — 긴 비번이 '빈 칸'으로 오판되던 문제](https://github.com/beyondsoft-kr/pstmacro/pull/298)
	- 2026-06-25 · **MERGED** · [#299 fix(auth): 로그인 폼 DOM 시간초과 없이 끝까지 대기 + 0.18초 검증대기 제거 (사수 지시)](https://github.com/beyondsoft-kr/pstmacro/pull/299)
	- 2026-06-25 · **MERGED** · [#300 fix(blog): groupId 파싱 견고화 — 댓글 게시 실패 완화](https://github.com/beyondsoft-kr/pstmacro/pull/300)
	- 2026-06-25 · **MERGED** · [#301 feat(blog): 게시 UI를 카페처럼 '링크 1개'로 — 특정글/최신N개 토글·갯수 제거](https://github.com/beyondsoft-kr/pstmacro/pull/301)
	- 2026-06-25 · **MERGED** · [#302 feat(auth): 로그인 본인확인(휴대전화 번호) 화면 자동 처리 + 보류 전환](https://github.com/beyondsoft-kr/pstmacro/pull/302)
	- 2026-06-25 · **MERGED** · [#303 fix(posts): 블로그 게시 화면에서 고정된 '최신 3개' 갯수/모드 문구 제거](https://github.com/beyondsoft-kr/pstmacro/pull/303)
	- 2026-06-25 · **MERGED** · [#304 fix(auth): 로그인 폼 자동입력 실패에 백트레이스·원인 진단 배선](https://github.com/beyondsoft-kr/pstmacro/pull/304)
	- 2026-06-25 · **MERGED** · [#305 fix(auth): 로그인 실패 trace 누락 마저 배선(밴드 graceful Error + 쿠키무효)](https://github.com/beyondsoft-kr/pstmacro/pull/305)
	- 2026-06-25 · **MERGED** · [#306 fix(net): 전송(reqwest) 오류에 진짜 원인 + 백트레이스 노출](https://github.com/beyondsoft-kr/pstmacro/pull/306)
	- 2026-06-25 · **MERGED** · [#307 fix(auth): 로그인 폼 게이트에 keydown 후킹 실제 설치 확인 추가(조건 ④)](https://github.com/beyondsoft-kr/pstmacro/pull/307)
	- 2026-06-25 · **MERGED** · [#308 fix(auth): 결과 폴링 게이트도 리소스 정착까지 대칭 강화](https://github.com/beyondsoft-kr/pstmacro/pull/308)
	- 2026-06-25 · **MERGED** · [#309 fix(auth): keydown 후킹 게이트(④)를 관측 전용으로 강등](https://github.com/beyondsoft-kr/pstmacro/pull/309)
	- 2026-06-25 · **MERGED** · [#310 fix(auth): keydown 후킹을 게이트 조건 ④로 승격 (관측→하드 게이트)](https://github.com/beyondsoft-kr/pstmacro/pull/310)
	- 2026-06-25 · **MERGED** · [#311 fix(blog): 댓글 groupId를 blogNo에서 추출 — 댓글 미등록 근본 원인 해결](https://github.com/beyondsoft-kr/pstmacro/pull/311)
	- 2026-06-26 · **MERGED** · [#312 fix(blog): 글 조회·댓글 요청에 브라우저 위장 헤더 추가 — 봇차단으로 댓글 안 달리던 문제 해결](https://github.com/beyondsoft-kr/pstmacro/pull/312)
	- 2026-06-26 · **MERGED** · [#313 fix(blog): 댓글 등록 실패 시 네이버 응답 code·message 노출](https://github.com/beyondsoft-kr/pstmacro/pull/313)
	- 2026-06-26 · **MERGED** · [#314 chore(queue): 되돌린 'Done 카드' 기능의 죽은 코드 제거](https://github.com/beyondsoft-kr/pstmacro/pull/314)
	- 2026-06-26 · **MERGED** · [#315 fix(blog): 자세히보기 사유 노출 + 댓글 3초 간격 + 블로그 전용 로고](https://github.com/beyondsoft-kr/pstmacro/pull/315)
	- 2026-06-26 · **MERGED** · [#316 fix(blog): 댓글 도배방지 텀 3초 → 10초](https://github.com/beyondsoft-kr/pstmacro/pull/316)
	- 2026-06-26 · **MERGED** · [#317 fix(adb): IP 변경 버튼을 실제 IP 변경 확인까지 대기로 통일(고정 3초 제거)](https://github.com/beyondsoft-kr/pstmacro/pull/317)
	- 2026-06-26 · **MERGED** · [#318 feat(clip): 네이버 클립 댓글 게시 + 선택 로그인 (패킷 기반)](https://github.com/beyondsoft-kr/pstmacro/pull/318)
	- 2026-06-29 · **MERGED** · [#319 fix(auth): 로그인 자동입력 진단에 readOnly·preventDefault 관측 추가](https://github.com/beyondsoft-kr/pstmacro/pull/319)
	- 2026-06-29 · **MERGED** · \[#320 fix(auth): \[진단\] 로그인 게이트 OPEN 순간 네트워크/안티봇 스냅샷 로그\]([https://github.com/beyondsoft-kr/pstmacro/pull/320](https://github.com/beyondsoft-kr/pstmacro/pull/320))
	- 2026-06-29 · **MERGED** · \[#321 fix(auth): \[진단\] 로그인 게이트 OPEN 순간 자동화/CDP 지문 로그\]([https://github.com/beyondsoft-kr/pstmacro/pull/321](https://github.com/beyondsoft-kr/pstmacro/pull/321))
	- 2026-06-29 · **MERGED** · [#322 fix(auth): 스텔스를 Navigator.prototype에 정의 — webdriver own-property 탐지 흔적 제거](https://github.com/beyondsoft-kr/pstmacro/pull/322)
	- 2026-06-29 · **MERGED** · \[#323 fix(auth): \[진단\] 수동입력 모드 (PSTMACRO_LOGIN_MANUAL) — 환경 vs 합성입력 격리\]([https://github.com/beyondsoft-kr/pstmacro/pull/323](https://github.com/beyondsoft-kr/pstmacro/pull/323))
	- 2026-06-29 · **MERGED** · [#324 feat(admin): Admin–하위 원격제어 시스템(중앙서버·에이전트·Admin웹) — 계정분배·로그인·게시명령·예약](https://github.com/beyondsoft-kr/pstmacro/pull/324)
	- 2026-06-29 · **MERGED** · [#325 fix(posts): 게시 안정화 — 미분류 실패 에러처리·약관 자동동의·대기초과 재시도·큐 가시성](https://github.com/beyondsoft-kr/pstmacro/pull/325)
	- 2026-06-29 · **MERGED** · [#326 fix(posts): 대기초과 재시도를 '차단 감지까지 끈질기게'로 확대](https://github.com/beyondsoft-kr/pstmacro/pull/326)
	- 2026-06-29 · **MERGED** · [#327 fix(posts): 안 되던 게시 기능을 실제로 되게 — 쿠키/디스크IO/동의하기](https://github.com/beyondsoft-kr/pstmacro/pull/327)
	- 2026-06-29 · **MERGED** · [#328 fix(posts): 동의하기 버튼 추측-클릭 폴백 제거 (오클릭 위험 R1)](https://github.com/beyondsoft-kr/pstmacro/pull/328)
	- 2026-06-29 · **MERGED** · [#329 fix(posts): 동의하기를 금융서비스 가입 URL 이동으로(패킷 분석) — 체크박스 폴백 보존](https://github.com/beyondsoft-kr/pstmacro/pull/329)
	- 2026-06-29 · **MERGED** · [#330 fix(auth): 계정 잠금 감지 문구 교정 — 현행 '보호(잠금) 조치중' 페이지 못 잡던 버그](https://github.com/beyondsoft-kr/pstmacro/pull/330)
	- 2026-06-30 · **MERGED** · [#331 fix(posts): getProfile 전송 실패를 '로그인·잠금' 아닌 '네트워크 끊김'으로 — 재시도+source 노출](https://github.com/beyondsoft-kr/pstmacro/pull/331)
	- 2026-06-30 · **MERGED** · [#332 chore: 실수로 커밋된 Zone.Identifier 메타파일 제거 + gitignore](https://github.com/beyondsoft-kr/pstmacro/pull/332)
	- 2026-06-30 · **MERGED** · [#333 test(posts): 전송 재시도 루프·네트워크 분류 테스트 backfill (#331 후속)](https://github.com/beyondsoft-kr/pstmacro/pull/333)
	- 2026-06-30 · **MERGED** · [#334 fix(posts): 프로필 상태 조회 500은 재시도 말고 즉시 실패](https://github.com/beyondsoft-kr/pstmacro/pull/334)
	- 2026-06-30 · **MERGED** · [#335 fix(posts): 종토 재시도 30→9회 + 프로필상태 500도 재시도 대상으로](https://github.com/beyondsoft-kr/pstmacro/pull/335)
	- 2026-06-30 · **MERGED** · [#336 fix(posts): 일시적 네트워크 끊김(10060/10053/10054)도 9회 재시도 대상으로](https://github.com/beyondsoft-kr/pstmacro/pull/336)
	- 2026-06-30 · **MERGED** · [#337 fix(auth): CDP 소켓 중단(10053/10054/10060) 시 재접속 후 재시도](https://github.com/beyondsoft-kr/pstmacro/pull/337)
	- 2026-06-30 · **MERGED** · [#338 fix(posts): 종토 게시 계정별 시작 로그 추가 (안 보이던 계정 추적)](https://github.com/beyondsoft-kr/pstmacro/pull/338)
	- 2026-06-30 · **MERGED** · [#339 fix(queue): 종토 계정 병렬 수를 하드코딩(4) 대신 사용자 '최대 작동 가능 작업 수'로](https://github.com/beyondsoft-kr/pstmacro/pull/339)
	- 2026-06-30 · **MERGED** · [#340 fix(notifications): 알림 패널 폴링 2초→400ms (결과 즉시화)](https://github.com/beyondsoft-kr/pstmacro/pull/340)
	- 2026-06-30 · **MERGED** · [#341 fix(queue): 종토 재시도 중 UI에 '재시도중 N/M' 표시](https://github.com/beyondsoft-kr/pstmacro/pull/341)
	- 2026-06-30 · **MERGED** · [#342 fix(posts): 종토 HTTP 500/서버오류는 재시도 말고 빨리 실패](https://github.com/beyondsoft-kr/pstmacro/pull/342)
	- 2026-06-30 · **MERGED** · [#343 fix(queue): 종토방 미시도 계정 큐에서 빼지 말고 차례까지 재대기](https://github.com/beyondsoft-kr/pstmacro/pull/343)
	- 2026-06-30 · **MERGED** · [#344 fix(post): 종토방 게시 실제 API 호출·브라우저 네트워크 진단 로그 추가](https://github.com/beyondsoft-kr/pstmacro/pull/344)
	- 2026-06-30 · **MERGED** · [#349 fix(queue): 나눠서 게시 시 큐 ID 충돌로 계정·종목이 증발하던 버그 수정](https://github.com/beyondsoft-kr/pstmacro/pull/349)
	- 2026-06-30 · **MERGED** · [#350 fix(post): 종토방 "동의하기"(네이버페이 가입) 패킷 복원 + 죽은 코드 정리](https://github.com/beyondsoft-kr/pstmacro/pull/350)
	- 2026-06-30 · **MERGED** · [#351 fix(auth): 로그인 시 웹 컨텐츠로 포커스 이동해 캡차 빈도 완화](https://github.com/beyondsoft-kr/pstmacro/pull/351)
	- 2026-06-30 · **MERGED** · [#352 test(posts): 종토방 특정 게시글 댓글 다중 URL 회귀 테스트](https://github.com/beyondsoft-kr/pstmacro/pull/352)
</details>
<details>
<summary>2026-07 개인 PR 전체</summary>
	- 2026-07-01 · **MERGED** · [#354 feat(posts): 글 관리 '좋아요' 버튼 — 특정 게시글에 선택 계정들이 API로 좋아요](https://github.com/beyondsoft-kr/pstmacro/pull/354)
	- 2026-07-01 · **MERGED** · [#355 chore: 로컬 대화기록·프롬프트·요청 드롭파일 gitignore](https://github.com/beyondsoft-kr/pstmacro/pull/355)
	- 2026-07-01 · **MERGED** · [#356 fix(post): 네이버페이 가입 '완료' 오판 수정 (로그인/약관 페이지로 튕기면 미완료)](https://github.com/beyondsoft-kr/pstmacro/pull/356)
	- 2026-07-01 · **MERGED** · [#357 feat(posts): 좋아요 — 링크 여러 개(칩 입력) + 완료 토스트](https://github.com/beyondsoft-kr/pstmacro/pull/357)
	- 2026-07-01 · **MERGED** · [#358 fix(posts): 특정 게시글 댓글 링크 여러 개가 1개로 줄던 버그 (백엔드 저장 누락)](https://github.com/beyondsoft-kr/pstmacro/pull/358)
	- 2026-07-01 · **MERGED** · [#359 fix(queue): 안 눌러도 펼쳐지고 하나 누르면 같은 것들이 다 펼쳐지던 큐 상세 버그](https://github.com/beyondsoft-kr/pstmacro/pull/359)
	- 2026-07-01 · **MERGED** · [#360 fix(post): 종토방 댓글 계정별 3초 간격 스로틀 (도배방지·In process 회피)](https://github.com/beyondsoft-kr/pstmacro/pull/360)
	- 2026-07-01 · **MERGED** · [#361 fix(posts): 좋아요도 npay 금융서비스 가입(동의하기) 먼저 — 미가입 계정 반응 400 수정](https://github.com/beyondsoft-kr/pstmacro/pull/361)
	- 2026-07-01 · **MERGED** · [#362 fix(auth): chrome 종료 확인 근거를 로그에 노출 — kill 결과·wait ExitStatus·프로필 삭제](https://github.com/beyondsoft-kr/pstmacro/pull/362)
	- 2026-07-01 · **MERGED** · [#363 fix(post): npay 필수약관 콜백 따라가기 + 동의 Y + 프로필 상태 500 진단 프로브](https://github.com/beyondsoft-kr/pstmacro/pull/363)
	- 2026-07-01 · **MERGED** · [#364 fix(auth): 로그인 직후 브라우저로 npay 가입 완료 — 미가입 계정 프로필 상태 500 근본수정](https://github.com/beyondsoft-kr/pstmacro/pull/364)
	- 2026-07-01 · **MERGED** · [#365 fix(queue): 종토 429(요청 과다)를 오류가 아니라 대기초과(재시도)로 — 계정 안 죽이고 최종결과로 판단](https://github.com/beyondsoft-kr/pstmacro/pull/365)
	- 2026-07-01 · **MERGED** · [#366 fix(post): 프로필 상태 500을 상황별로 구분 — 계정 보호조치 vs npay 미완료 vs 그 외](https://github.com/beyondsoft-kr/pstmacro/pull/366)
	- 2026-07-01 · **MERGED** · [#367 fix(logging): 게시·로그인 네이버 원문 로그 전면화 + 좋아요 알림 + 로그인 npay 제거](https://github.com/beyondsoft-kr/pstmacro/pull/367)
	- 2026-07-01 · **MERGED** · [#368 fix(post): 미가입 계정 npay 약관동의를 패킷 쿠키 누적으로 완료 — 프로필 500 근본수정](https://github.com/beyondsoft-kr/pstmacro/pull/368)
	- 2026-07-01 · **MERGED** · [#369 fix(post): 종토 요청을 브라우저와 일치시켜 네이버 봇탐지(UMON) 403/500 해소](https://github.com/beyondsoft-kr/pstmacro/pull/369)
	- 2026-07-01 · **MERGED** · [#371 refactor(post): 불필요한 API 호출 축소](https://github.com/beyondsoft-kr/pstmacro/pull/371)
	- 2026-07-01 · **MERGED** · [#372 refactor(post): 불필요한 API 호출 및 패킷과 1대1 대조](https://github.com/beyondsoft-kr/pstmacro/pull/372)
	- 2026-07-01 · **MERGED** · [#373 fix(auth,post): 전체 패킷과 1대1 매치](https://github.com/beyondsoft-kr/pstmacro/pull/373)
	- 2026-07-01 · **MERGED** · [#374 fix(post): 모든 요청 헤더를 성공 패킷과 1대1 매치](https://github.com/beyondsoft-kr/pstmacro/pull/374)
	- 2026-07-01 · **MERGED** · [#375 fix(post): npay 가입 후 회전된 세션 쿠키를 반영해 프로필 status 500 해결](https://github.com/beyondsoft-kr/pstmacro/pull/375)
	- 2026-07-02 · **MERGED** · [#376 fix(login): hidden 창 키드롭 수정 + 키/네트워크 진단 + CDP 트레이스](https://github.com/beyondsoft-kr/pstmacro/pull/376)
	- 2026-07-02 · **MERGED** · [#377 fix(admin): 계정 분배 즉시 반영 + 통신로그 스크롤바·위치 유지](https://github.com/beyondsoft-kr/pstmacro/pull/377)
	- 2026-07-02 · **MERGED** · [#378 feat(login): 수동추가 — 사람이 직접 로그인, 쿠키는 자동로그인과 동일 저장 + 계정 자동 추가](https://github.com/beyondsoft-kr/pstmacro/pull/378)
	- 2026-07-02 · **MERGED** · [#379 feat(admin): 게시 명령 화면 설계서 + UI 구현(종토 확정, 카페/블로그/밴드 버튼)](https://github.com/beyondsoft-kr/pstmacro/pull/379)
	- 2026-07-02 · **MERGED** · [#380 feat: ADB 선택화 + Chrome 프로세스 트리 종료/게시완료 신호 + 쿠키만료 열](https://github.com/beyondsoft-kr/pstmacro/pull/380)
	- 2026-07-02 · **MERGED** · [#381 feat(post): 게시 패킷 요청+응답 전체 원문 와이어 트레이스(기본 ON)](https://github.com/beyondsoft-kr/pstmacro/pull/381)
	- 2026-07-03 · **MERGED** · [#382 fix(like): 좋아요 전용 흐름 분리 — npay 쿠키 훼손 회귀 수정](https://github.com/beyondsoft-kr/pstmacro/pull/382)
	- 2026-07-03 · **MERGED** · [#383 fix(like): 좋아요 실패를 차단(비활성)/세션만료(재로그인)로 구분 + 계정 상태 전환](https://github.com/beyondsoft-kr/pstmacro/pull/383)
	- 2026-07-03 · **MERGED** · [#384 fix(like): 차단/만료를 form 원문으로 판정 + 상태 반영(loginId 매칭)](https://github.com/beyondsoft-kr/pstmacro/pull/384)
	- 2026-07-03 · **MERGED** · [#385 feat(login): 수동추가 시 ADB 연결되면 IP 한 번 회전 후 로그인창](https://github.com/beyondsoft-kr/pstmacro/pull/385)
	- 2026-07-03 · **CLOSED** · [#386 feat(admin): 게시 명령 화면 UI (종토 확정, 카페/블로그/밴드 버튼만)](https://github.com/beyondsoft-kr/pstmacro/pull/386)
	- 2026-07-03 · **MERGED** · [#387 fix(post): 실명인증 미완 계정 프로필 생성 실패 시 사유 명확화](https://github.com/beyondsoft-kr/pstmacro/pull/387)
	- 2026-07-03 · **MERGED** · [#388 fix(post): 실명인증 realNameCheck 확정 + 차단 원문 + 죽은계정 쿠키숨김](https://github.com/beyondsoft-kr/pstmacro/pull/388)
	- 2026-07-07 · **MERGED** · [#389 fix(post): 게시 요청 UA에서 HeadlessChrome 제거](https://github.com/beyondsoft-kr/pstmacro/pull/389)
	- 2026-07-07 · **MERGED** · [#390 chore(auth): 로그인 CDP 네트워크 원문 로깅 옵트인 게이트](https://github.com/beyondsoft-kr/pstmacro/pull/390)
	- 2026-07-07 · **MERGED** · [#392 feat(posts): 종목토론방 싫어요 기능 추가](https://github.com/beyondsoft-kr/pstmacro/pull/392)
	- 2026-07-07 · **MERGED** · [#393 fix(auth): 로그인 Accept-Language를 navigator.languages 위조본과 일치](https://github.com/beyondsoft-kr/pstmacro/pull/393)
	- 2026-07-07 · **CLOSED** · [#394 fix(auth): 로그인 스텔스 webdriver/languages override 제거(실험 B)](https://github.com/beyondsoft-kr/pstmacro/pull/394)
	- 2026-07-07 · **CLOSED** · [#395 fix(auth): 로그인 IP 회전에 dwell·앞대역(/16) 검증·재토글(실험 A)](https://github.com/beyondsoft-kr/pstmacro/pull/395)
	- 2026-07-08 · **CLOSED** · [#396 feat(auth): 로그인 UA·Client Hints 판마다 최신 실존 크롬으로 일관 로테이션(캡차 실험)](https://github.com/beyondsoft-kr/pstmacro/pull/396)
	- 2026-07-08 · **MERGED** · [#397 feat(auth): 로그인 UA 로테이션 + wasm 차단 실험 스위치](https://github.com/beyondsoft-kr/pstmacro/pull/397)
	- 2026-07-09 · **MERGED** · [#399 feat(login): 로그인 wasm 차단 기본 활성화 + 계정 보류 수동 설정](https://github.com/beyondsoft-kr/pstmacro/pull/399)
	- 2026-07-09 · **MERGED** · [#401 feat(posts): 조회수 부스트 — 시크릿창 껐다켰다 반복](https://github.com/beyondsoft-kr/pstmacro/pull/401)
	- 2026-07-09 · **MERGED** · [#402 fix(posts): 조회수 부스트 — 창이 완전 로딩 전에 닫히던 문제 수정](https://github.com/beyondsoft-kr/pstmacro/pull/402)
	- 2026-07-09 · **CLOSED** · [#404 feat(posts): 종토 특정글 전체 댓글 게시 + 나눠서 게시(댓글 분배)](https://github.com/beyondsoft-kr/pstmacro/pull/404)
	- 2026-07-09 · **MERGED** · [#406 feat(login): 로그인 지문·행동 재료 트레이스(PSTMACRO_FP_TRACE, 기본 OFF)](https://github.com/beyondsoft-kr/pstmacro/pull/406)
	- 2026-07-10 · **MERGED** · [#407 feat(forum): 종토 게시 코어 기능 일괄 반영 (닉네임 랜덤·글 수정·나눠서 게시·크롬 제거)](https://github.com/beyondsoft-kr/pstmacro/pull/407)
	- 2026-07-10 · **MERGED** · [#408 fix(forum): 나눠서게시 댓글 전량 분배 + 내용변경 제목입력 흰화면 수정](https://github.com/beyondsoft-kr/pstmacro/pull/408)
	- 2026-07-10 · **MERGED** · [#409 feat(forum): 내용변경 edit 백그라운드 분리 + 원글/수정 2단계 알림·토스트](https://github.com/beyondsoft-kr/pstmacro/pull/409)
	- 2026-07-13 · **MERGED** · [#411 feat(pstmacro): 세션 개선 — 내용변경 흰화면·edit실패 알림·raw 로그·닉네임회전·잔여횟수·forum-only 게이트](https://github.com/beyondsoft-kr/pstmacro/pull/411)
	- 2026-07-13 · **MERGED** · [#412 chore: ignore local Design_Doc/, error_log/, logo/ folders](https://github.com/beyondsoft-kr/pstmacro/pull/412)
	- 2026-07-13 · **MERGED** · [#413 fix(band): 밴드 로그인 네이버 OAuth 재작성 + 게시 안정화](https://github.com/beyondsoft-kr/pstmacro/pull/413)
	- 2026-07-14 · **MERGED** · [#414 feat(report): 종목토론방 글 신고하기 (링크n×계정m, 비차단)](https://github.com/beyondsoft-kr/pstmacro/pull/414)
	- 2026-07-14 · **MERGED** · [#415 fix(build): admin.html 빌드 엔트리를 조건부로 — master 빌드 복구](https://github.com/beyondsoft-kr/pstmacro/pull/415)
	- 2026-07-14 · **MERGED** · [#416 feat(blog): 네이버 블로그 발행 기반 이식 (write_client·작성기·커맨드)](https://github.com/beyondsoft-kr/pstmacro/pull/416)
	- 2026-07-14 · **MERGED** · [#417 feat(blog): 네이버 편집기식 툴바·블록 편집기 (사진·스티커·링크·파일·일정·소스코드·장소 + 서식)](https://github.com/beyondsoft-kr/pstmacro/pull/417)
	- 2026-07-14 · **MERGED** · [#418 fix(report): 신고 경로에 와이어샤크식 원문 로그 전면 배선](https://github.com/beyondsoft-kr/pstmacro/pull/418)
	- 2026-07-14 · **MERGED** · [#419 fix(blog): 발행 존재확인 게이트 제거·SeOptions referer 수정](https://github.com/beyondsoft-kr/pstmacro/pull/419)
	- 2026-07-14 · **MERGED** · [#420 fix(blog): 편집기 세션·업로드 실측 수정 + 편집기 빈칸 UX·발행 알림](https://github.com/beyondsoft-kr/pstmacro/pull/420)
	- 2026-07-14 · **MERGED** · [#421 fix(report): by-item 필수 파라미터 누락(400) 수정](https://github.com/beyondsoft-kr/pstmacro/pull/421)
	- 2026-07-14 · **MERGED** · [#422 fix(report): 토큰 크롬에 계정 로그인 쿠키 주입(로그인 페이지 리다이렉트 방지)](https://github.com/beyondsoft-kr/pstmacro/pull/422)
	- 2026-07-14 · **MERGED** · [#423 fix(blog): 발행 'not acceptable' 수정(text 정렬 스키마) + 실제 사진 업로드(upphoto)](https://github.com/beyondsoft-kr/pstmacro/pull/423)
	- 2026-07-14 · **MERGED** · [#424 fix(blog): 링크 발행 'not acceptable'(서명 URL) + 발행 결과 알림창 기록](https://github.com/beyondsoft-kr/pstmacro/pull/424)
	- 2026-07-14 · **MERGED** · [#425 fix(blog): 공개범위 무시·비공개 강제 수정 (editorSource 누락)](https://github.com/beyondsoft-kr/pstmacro/pull/425)
	- 2026-07-14 · **MERGED** · [#426 feat(admin): 게시 종류에 '블로그 새글' 추가(→제목/본문 작성기)](https://github.com/beyondsoft-kr/pstmacro/pull/426)
	- 2026-07-14 · **MERGED** · [#427 fix(report): ncaptcha 토큰 스크랩 대신 브라우저가 직접 제출(WASM 캡차 대응)](https://github.com/beyondsoft-kr/pstmacro/pull/427)
	- 2026-07-14 · **MERGED** · [#428 fix(report): 제출 버튼 오클릭 수정(btn_submit '신고하기' 정확 클릭)](https://github.com/beyondsoft-kr/pstmacro/pull/428)
	- 2026-07-14 · **MERGED** · [#429 fix(report): 선택한 신고 사유가 웹에 반영(React 라디오 확실 선택+순서 폴백)](https://github.com/beyondsoft-kr/pstmacro/pull/429)
	- 2026-07-15 · **MERGED** · [#432 fix(report): 신고 단건조회·크롬 자동종료·사유 자동선택 + 블로그 openType 전체공개 교정 + Admin 원격 미디어 + 서버 Cloud Run 준비](https://github.com/beyondsoft-kr/pstmacro/pull/432)
	- 2026-07-15 · **MERGED** · [#433 ci: add cloud run deploy workflow and gcp setup scripts](https://github.com/beyondsoft-kr/pstmacro/pull/433)
	- 2026-07-15 · **MERGED** · [#434 fix(clip): 프로필 생성을 죽은 graphql → creatorhub REST로 교체 (404 수정)](https://github.com/beyondsoft-kr/pstmacro/pull/434)
	- 2026-07-15 · **MERGED** · [#435 fix(cafe): 배포 후 버그 — 최신 댓글이 지정 게시판(menu) 무시하고 카페 전체에 달리던 문제](https://github.com/beyondsoft-kr/pstmacro/pull/435)
	- 2026-07-16 · **MERGED** · [#436 fix(login): 네이버 v4 로그인 폼 대응 — 셀렉터만 갱신(내부 불변)](https://github.com/beyondsoft-kr/pstmacro/pull/436)
	- 2026-07-16 · **MERGED** · [#437 fix: 원격제어 분배 지연 + Admin UX(분배 버튼·미리보기 제거·계정상태 다기기)](https://github.com/beyondsoft-kr/pstmacro/pull/437)
	- 2026-07-16 · **MERGED** · [#438 fix(band): 로그인 폼 네이버 재사용(v4 멈춤 해결)+동의 보강 / feat(admin): 계정상태 하위별 색구분](https://github.com/beyondsoft-kr/pstmacro/pull/438)
	- 2026-07-20 · **MERGED** · [#440 feat(admin): 통신 로그 일자별 필터 — 3단 종속 목록형(컴퓨터→날짜)](https://github.com/beyondsoft-kr/pstmacro/pull/440)
	- 2026-07-20 · **MERGED** · [#442 feat(remote-control): 하위 기기 안정 식별 — 같은 PC=같은 기기(machine_id upsert)](https://github.com/beyondsoft-kr/pstmacro/pull/442)
	- 2026-07-20 · **MERGED** · [#443 fix(admin): 통신 로그 초기 화면 비움(대량 로그 즉시 렌더 방지)](https://github.com/beyondsoft-kr/pstmacro/pull/443)
	- 2026-07-20 · **MERGED** · [#445 feat(admin): 통신로그 기기 2단 필터 — 하위com → 등록 이력(이름·날짜순)](https://github.com/beyondsoft-kr/pstmacro/pull/445)
	- 2026-07-20 · **MERGED** · [#447 feat(admin): 게시명령 종토 종목 자동/수동 선택 분기 — 데스크톱 종목선택 모달 공유 재사용](https://github.com/beyondsoft-kr/pstmacro/pull/447)
	- 2026-07-20 · **MERGED** · [#448 fix(server): 결과보고에서 삭제된 기기의 고아 리포트 숨김](https://github.com/beyondsoft-kr/pstmacro/pull/448)
</details>
