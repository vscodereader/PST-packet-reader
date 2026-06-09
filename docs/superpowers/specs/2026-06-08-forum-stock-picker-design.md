# 종목토론방 종목 선택 화면 재디자인 (네이버 모바일 priceTop 복제)

- 이슈: #157
- 날짜: 2026-06-08
- 상태: 승인됨 (구현 계획 대기)

## 1. 문제

종목토론방(forum) 게시 흐름에서 종목을 고르는 화면(`src/features/posts/stock-crawl-modal.tsx`)이
원하는 종목을 찾지 못한다. 예: 검색창에 `KO`를 쳐도 `KODEX`가 안 나온다.

### 근본 원인

`src-tauri/src/discussion_batch.rs::search_naver_stocks`(현재 구현)는:

1. 데스크탑 `stock.naver.com` API **4개**(거래량 top80 / 상승 top80 / 하락 top80 / 인기토론 top80)를 호출,
2. 합쳐서 dedupe 후 **`unique.truncate(80)`** 로 최대 ~80개 풀을 만들고,
3. 검색어를 **그 80개 풀 안에서만** `contains`로 필터링한다(`discussion_batch.rs:277-282`).

따라서 풀 밖 종목은 검색어가 일치해도 절대 나오지 않는다. KODEX가 안 나오는 이유는
"KODEX라서"가 아니라 "이미 받아둔 80개 밖이라서"다. 검색이 전체 종목 대상이 아닌 구조적 결함.

## 2. 목표

`https://m.stock.naver.com/domestic/home/priceTop/total` **모바일** 페이지를 그대로 복제한
종목 선택 화면을 만든다. 데이터 페치는 **전부 Rust/Tauri 백엔드**가 담당하고, 프론트는 렌더만 한다.

### 요구사항 (확정)

- **카테고리 탭(6개, 순서 고정)**: 토론 · 거래대금 · 인기 종목 · 상승 · 하락 · 거래량
  - 시가총액, 코스피/코스닥/코스피100/200/선물/밸류업 등 지수 칩은 **제외**.
- **거래소 선택**: 탭 옆 `NXT ⌄` → 누르면 **딤 배경 + "거래소 선택" 소형 모달** → `KRX | NXT` 택1.
  - KRX/NXT 모두 동일한 6개 카테고리 제공.
- **카테고리 목록**: 해당 정렬 1위부터 순차. 하단 **더보기**로 다음 페이지 로드(끝까지 가능).
- **🔥 불 아이콘**: 지금 토론이 활발한 종목에 표시.
- **검색**: 검색어가 들어간 **국내 종목 전부** 반환(80개 상한 제거). 비우면 카테고리 화면으로 복귀.
- **선택/적용**: 종목 행 체크박스 → `적용(N)`. 기존 계약과 동일.

### 명시적 비목표 (절대 변경 금지)

로그인/계정/쿠키, 네이버 카페, 밴드, 큐, 패킷 게시 엔진(`run_forum_publish_now` /
`run_naver_discussion_batch`), 기존 `search_stocks` 커맨드, `Stock`/`DiscussionStock` 타입,
종목 게시 실행 계약. 이 변경은 **종목 선택 화면(데이터 소스 + 모달 UI)만** 건드린다.

## 3. 네이버 모바일 API (패킷 분석 확정, 2026-06-08)

호스트 `m.stock.naver.com`, GET, JSON. 공통 래퍼 `{ isSuccess, detailCode, message, result }`.

### 3.1 카테고리 목록 — 거래대금/거래량/상승/하락/인기

```
GET /front-api/domestic/stock/list
    ?sortType={priceTop|quantTop|up|down|searchTop}
    &category=all
    &domesticStockExchangeType={KRX|NXT}
    &page=N&pageSize=50
```

`result`: `{ stockListSortType, totalCount(예 KRX 4396), page, pageSize, stocks:[ ... ] }`
각 `stocks[i]`:
`{ id, name, itemCode, stockEndType(stock|etf), stockExchangeType(KOSPI|KOSDAQ),
   currentPrice, fluctuationsType(RISING|FALLING|EVEN), fluctuations, fluctuationsRatio,
   accumulatedTradingVolume, accumulatedTradingValue, marketValue, isNxt }`

카테고리 ↔ sortType 매핑:

| 탭        | sortType    |
| --------- | ----------- |
| 거래대금  | `priceTop`  |
| 거래량    | `quantTop`  |
| 상승      | `up`        |
| 하락      | `down`      |
| 인기 종목 | `searchTop` |

### 3.2 토론 카테고리

```
GET /front-api/discussion/ranking/list/price
    ?nationType=KOR&size=50&stockExchangeType={KRX|NXT}&page=1
```

`result`: `{ rankTime, totalCount(100), hasNextPage, itemCodes:[순위순 코드], contents:[{posts:[...]}] }`
→ `itemCodes`가 토론 랭킹 순서. 종목 이름/가격 메타는 이 응답에 없으므로 보강이 필요하다(§4.2).

### 3.3 🔥 불 아이콘 소스

```
GET /front-api/discussion/rankings/itemCodes
```

`result`: `{ itemCodes:[약 100개 코드] }` — "지금 토론 활발한 종목코드 집합".
목록 종목의 `code`가 이 집합에 포함되면 🔥 표시.

> 참고: 사용자가 관찰한 "1분 전 토론글" 의미를 직접 주는 필드는 패킷에 없다. 이 itemCodes가
> 모바일 페이지가 실제로 🔥 판단에 쓰는 신호이며, 본 설계는 이를 🔥의 정의로 채택한다(승인됨).

### 3.4 전체 검색 (검색어 포함 종목 전부)

```
GET /front-api/search?q={query}&size=20&target=stock,index,marketindicator,coin,ipo,fund&page=N
```

`result`: `{ query, totalCount(예 "ko"=1161), items:[ ... ] }`
각 `items[i]`:
`{ code, name, typeCode(KOSPI|KOSDAQ|NYSE...), typeName, url(/domestic/stock/{code}/total),
   nationCode(KOR|USA), category(stock|index|coin...), hasDiscussion(bool|null) }`

→ **국내 필터**: `nationCode == "KOR" && category == "stock"`.
→ `page`를 증가시켜 `totalCount`까지 끌어오면 검색어 포함 국내 종목을 빠짐없이 확보(80개 상한 없음).

### 3.5 배치 메타 (토론 탭 종목 이름/가격 보강)

```
GET /front-api/realTime/marketPrice?itemCodes={code1,code2,...}&endType=stock&stockType=domestic
```

`result.datas[i]`:
`{ itemCode, stockName, stockExchangeType.nameKor(코스피|코스닥), closePrice(포맷된 문자열),
   compareToPreviousPrice.name(RISING|FALLING|EVEN), fluctuationsRatio }`
→ **여러 itemCode를 한 번에** 받아 이름/거래소/현재가/등락을 돌려준다. 토론 탭의 itemCodes(최대 100개)
메타 보강에 사용한다(필요 시 코드를 청크로 나눠 호출).

### 3.6 종목 코드 형태 주의

ETF/특수 종목 코드는 **영숫자 6자리**(예: `0193T0`, `0183J0`). 현재
`looks_like_stock_code`는 6자리 **전부 숫자**만 허용하므로 이런 코드를 누락한다 → 검증을 완화한다.
게시 대상 URL은 기존과 동일: `https://stock.naver.com/domestic/stock/{code}/discussion?chip=all`.

## 4. 설계

### 4.1 아키텍처 결정

백엔드가 "목록 + 🔥 itemCodes"를 **서버측에서 병합**해 종목별 `is_hot_discussion`까지 채운
완성형 데이터를 IPC로 내려준다. 프론트는 받아서 그리기만 한다(데이터 페치를 프론트로 넘기지 않음).

### 4.2 백엔드 (Rust)

신규 모듈 `src-tauri/src/forum_stocks.rs`(또는 `discussion_batch.rs` 내 구획). **기존
`search_naver_stocks` / `search_stocks` 커맨드는 그대로 둔다**(호환성). 신규 IPC 커맨드 2개를 추가:

```rust
#[tauri::command]
fn list_forum_stocks(category: ForumStockCategory, exchange: StockExchange, page: u32)
    -> Result<ForumStockPage, String>;

#[tauri::command]
fn search_forum_stocks(query: String, page: u32)
    -> Result<ForumStockPage, String>;
```

타입(ts-rs `#[derive(TS)]`로 `src/shared/bindings/` 자동 생성):

```rust
enum ForumStockCategory { Discussion, TradingValue, Popular, Rising, Falling, Volume }
enum StockExchange { Krx, Nxt }

struct ForumStock {
    code: String,
    name: String,
    exchange: String,          // KOSPI/KOSDAQ (표시용)
    price: String,             // 현재가(포맷된 문자열)
    change_rate: String,       // fluctuationsRatio
    change_type: String,       // rising | falling | even (색상용)
    is_hot_discussion: bool,   // 🔥
}
struct ForumStockPage {
    stocks: Vec<ForumStock>,
    total_count: u32,
    page: u32,
    has_next: bool,
}
```

동작:

- `list_forum_stocks(category != Discussion)`: §3.1 호출(sortType 매핑) + §3.3 itemCodes 호출 →
  각 종목 `is_hot_discussion` 채워 반환. `has_next = page * pageSize < total_count`.
- `list_forum_stocks(Discussion)`: §3.2의 `itemCodes`(순위순) → 이름/거래소/현재가/등락 메타를
  **§3.5 배치 메타**(`realTime/marketPrice?itemCodes={해당 페이지 코드들}`)로 보강해 순위 순서대로
  반환. itemCodes 자체가 토론 활발 신호이므로 `is_hot_discussion = true`. `has_next`는 §3.2
  `hasNextPage` 사용. (페이지당 size=50 기준으로 itemCodes를 슬라이스해 메타 호출.)
- `search_forum_stocks(query, page)`: §3.4 호출 → `nationCode=="KOR" && category=="stock"` 필터 →
  `ForumStock`로 매핑(검색 응답엔 가격이 없으므로 `price`/`change_rate`는 빈 값 허용,
  🔥는 itemCodes 집합으로 채움). `has_next`는 `page*size < total_count`.
- 코드 검증: 영숫자 6자리 허용(전부 숫자 강제 폐기).
- HTTP 헤더/타임아웃은 기존 `search_naver_stocks` 패턴 재사용(referer/user-agent/accept).

### 4.3 프론트 (`stock-crawl-modal.tsx` 재디자인)

**Props 계약 유지**: `{ open, preselected, onClose, onConfirm(StockCandidate[]) }`.
→ `publish-modal.tsx`는 호출부를 바꾸지 않는다.

UI 구성(모바일 페이지 복제):

- 상단 **카테고리 탭** 6개(토론/거래대금/인기 종목/상승/하락/거래량).
- 탭 우측 **`{exchange} ⌄`** 버튼 → 클릭 시 Mantine `Modal`(딤 오버레이) "거래소 선택" →
  `KRX | NXT` 선택 → 닫고 현재 카테고리 재로드.
- **검색창**: 입력값이 있으면 `search_forum_stocks`, 비면 `list_forum_stocks`로 전환.
  입력은 250ms 디바운스(기존 패턴 유지), 페이지는 1로 리셋.
- **목록 행**: `[체크박스] {🔥?} {종목명} {코드(monospace)} {현재가} {등락률(rising=red/falling=blue/even=gray)}`.
- **더보기**: `has_next`일 때 하단 버튼(또는 sentinel 무한스크롤) → `page++` 후 결과 append.
- 하단: `N개 선택됨` · `취소` · `적용(N)`. 선택 상태는 code 기준 누적(카테고리/거래소/검색 전환에도 유지).

`onConfirm`은 선택된 code들을 `StockCandidate { code, name, link }`로 매핑해 반환(이름은 누적 보관,
link는 `https://stock.naver.com/domestic/stock/{code}/discussion?chip=all`). 기존과 동일.

### 4.4 `publish-modal.tsx` 최소 보강

선택 칩 이름이 시드 `stocks` 목록(`ipc.stocks.list()`)에만 의존해, 시드 밖 종목은 코드로만 보이는
회귀를 막기 위해, `onConfirm`이 돌려준 `StockCandidate`의 name을 우선 사용하도록 칩 렌더 lookup만
보강한다(선택 결과에 name이 이미 있으므로 추가 IPC 불필요). 그 외 로직 불변.

## 5. 에러 처리

- 각 네이버 호출 실패 시: 해당 페이지 빈 결과 + 사용자 메시지(기존 토스트 패턴). 모달은 닫히지 않음.
- itemCodes(🔥) 호출 실패는 치명적이지 않음 → `is_hot_discussion`만 전부 false로 두고 목록은 정상 표시.
- 검색 결과 0건: "검색 결과가 없습니다" 빈 상태 표시.

## 6. 테스트 (TDD, 커버리지 게이트 준수: 프론트 93/93/90/80, Rust 83/82)

### Rust (wiremock)

- sortType 매핑: 6개 카테고리 → 올바른 `sortType`/엔드포인트 호출.
- 🔥 병합: itemCodes 집합에 든 종목만 `is_hot_discussion=true`.
- 토론 카테고리: itemCodes 순서 보존 + 메타 보강.
- 검색 국내 필터: `nationCode!=KOR` 또는 `category!=stock` 제외, 영숫자 코드 포함, 페이지네이션 `has_next`.
- itemCodes 호출 실패 시 목록은 살아있고 🔥만 false.

### 프론트 (IPC 목킹, 인접 `*.test.tsx` 필수 규칙 충족)

- 카테고리 탭 전환 시 해당 `category`로 `list_forum_stocks` 호출.
- 거래소 모달 열림/선택 → `exchange` 바뀌어 재호출.
- 검색어 입력 → `search_forum_stocks`로 전환, 비우면 카테고리 복귀.
- 더보기 → `page++` append, `has_next=false`면 버튼 숨김.
- 🔥 아이콘 조건부 렌더, 등락률 색상 분기.
- 선택 누적 + `적용` → `onConfirm`이 올바른 `StockCandidate[]` 반환.

## 7. 영향 파일 요약

- 신규: `src-tauri/src/forum_stocks.rs`, `src/shared/bindings/ForumStock*.ts`(자동 생성),
  프론트/러스트 테스트 파일.
- 수정: `src-tauri/src/lib.rs`(커맨드 등록), `src/shared/ipc/index.ts`(커맨드 노출),
  `src/features/posts/stock-crawl-modal.tsx`(재디자인), `src/features/posts/publish-modal.tsx`(칩 이름 보강).
- 불변: 로그인/카페/밴드/큐/패킷 게시 엔진, `search_stocks`, `Stock`/`DiscussionStock`.
