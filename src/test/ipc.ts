import { vi } from "vitest";

import type { PostJob } from "@/shared/bindings/PostJob";
import type { PublishOutcome } from "@/shared/bindings/PublishOutcome";
import type {
  Account,
  ActivityItem,
  Band,
  Board,
  Cafe,
  DashStat,
  LibraryPost,
  LogBatch,
  QueueNowItem,
  QueueScheduledItem,
  Stock,
} from "@/shared/data/types";

/**
 * In-memory Tauri-IPC backend for tests.
 *
 * Production code no longer ships any mock/seed data — the IPC wrappers always
 * `invoke`. In jsdom there is no Tauri runtime, so component and wrapper tests
 * mock `@tauri-apps/api/core` with this module's {@link invoke}, which serves
 * the fixtures below and emulates the Rust commands (including mutations that
 * return the full updated list). Call {@link resetIpc} in `beforeEach` to start
 * each test from the pristine dataset.
 */

const SEED_ACCOUNTS: Account[] = [
  {
    id: "a1",
    platform: "forum",
    loginId: "invest_king7",
    pw: "ik7!naver22",
    status: "active",
    last: "12분 전",
    tags: ["대형주", "반도체"],
  },
  {
    id: "a2",
    platform: "forum",
    loginId: "value_pick",
    pw: "vp@2024kr",
    status: "active",
    last: "30분 전",
    tags: ["반도체"],
  },
  {
    id: "a3",
    platform: "forum",
    loginId: "hot_trend22",
    pw: "trend#2211",
    status: "active",
    last: "1시간 전",
    tags: ["2차전지"],
  },
  {
    id: "a4",
    platform: "forum",
    loginId: "chart_master",
    pw: "cm!chart99",
    status: "active",
    last: "2시간 전",
    tags: ["2차전지", "대형주"],
  },
  {
    id: "a5",
    platform: "naver",
    loginId: "money_lab",
    pw: "mlab2024!!",
    status: "active",
    last: "3시간 전",
    tags: ["분석방"],
  },
  {
    id: "a6",
    platform: "naver",
    loginId: "stock_daily",
    pw: "daily#stock1",
    status: "new",
    last: "—",
    tags: [],
  },
  {
    id: "a7",
    platform: "band",
    loginId: "value_invest",
    pw: "band!value7",
    status: "active",
    last: "2시간 전",
    tags: ["모임"],
  },
  {
    id: "a8",
    platform: "forum",
    loginId: "day_trader_x",
    pw: "dtx@2024",
    status: "error",
    last: "1일 전",
    tags: ["단타"],
  },
  {
    id: "a9",
    platform: "forum",
    loginId: "long_holder",
    pw: "hold#long55",
    status: "new",
    last: "—",
    tags: [],
  },
  {
    id: "a10",
    platform: "naver",
    loginId: "insight_note",
    pw: "note!2024",
    status: "active",
    last: "4시간 전",
    tags: ["분석방", "대형주"],
  },
  {
    id: "a11",
    platform: "forum",
    loginId: "swing_trader",
    pw: "swing#88",
    status: "active",
    last: "6시간 전",
    tags: ["테마주"],
  },
  {
    id: "a12",
    platform: "band",
    loginId: "stock_study",
    pw: "study!band2",
    status: "active",
    last: "어제",
    tags: ["모임", "장기투자"],
  },
  {
    id: "a13",
    platform: "forum",
    loginId: "bio_hunter",
    pw: "bio@2024kr",
    status: "new",
    last: "—",
    tags: ["테마주"],
  },
  {
    id: "a14",
    platform: "naver",
    loginId: "cafe_master9",
    pw: "cm9!naver",
    status: "active",
    last: "2일 전",
    tags: [],
  },
  {
    id: "a15",
    platform: "forum",
    loginId: "trend_rider",
    pw: "ride#trend7",
    status: "active",
    last: "7시간 전",
    tags: ["2차전지", "테마주"],
  },
];

const SEED_STOCKS: Stock[] = [
  {
    code: "005930",
    name: "삼성전자",
    market: "KOSPI",
    posts: "12,480",
    price: "78,400",
    chg: 1.2,
  },
  {
    code: "000660",
    name: "SK하이닉스",
    market: "KOSPI",
    posts: "8,210",
    price: "189,500",
    chg: 2.8,
  },
  {
    code: "035720",
    name: "카카오",
    market: "KOSPI",
    posts: "9,640",
    price: "41,250",
    chg: -0.6,
  },
  {
    code: "035420",
    name: "NAVER",
    market: "KOSPI",
    posts: "6,330",
    price: "172,800",
    chg: 0.4,
  },
  {
    code: "086520",
    name: "에코프로",
    market: "KOSDAQ",
    posts: "15,720",
    price: "98,700",
    chg: -3.1,
  },
  {
    code: "247540",
    name: "에코프로비엠",
    market: "KOSDAQ",
    posts: "11,090",
    price: "172,300",
    chg: -2.4,
  },
  {
    code: "373220",
    name: "LG에너지솔루션",
    market: "KOSPI",
    posts: "7,450",
    price: "367,000",
    chg: 1.7,
  },
  {
    code: "005490",
    name: "POSCO홀딩스",
    market: "KOSPI",
    posts: "10,210",
    price: "412,500",
    chg: 3.3,
  },
  {
    code: "207940",
    name: "삼성바이오로직스",
    market: "KOSPI",
    posts: "3,180",
    price: "789,000",
    chg: 0.9,
  },
  {
    code: "068270",
    name: "셀트리온",
    market: "KOSPI",
    posts: "5,940",
    price: "182,400",
    chg: -1.1,
  },
  {
    code: "323410",
    name: "카카오뱅크",
    market: "KOSPI",
    posts: "4,720",
    price: "23,150",
    chg: 0.2,
  },
  {
    code: "042700",
    name: "한미반도체",
    market: "KOSPI",
    posts: "6,880",
    price: "118,900",
    chg: 4.6,
  },
];

const board = (name: string, menuId: number, boardType = "L"): Board => ({
  name,
  menuId,
  boardType,
});

const SEED_CAFES: Cafe[] = [
  {
    name: "주식투자연구소 카페",
    cafeRef: "cafe.naver.com/stocklab",
    cafeId: 11111111,
    boards: [
      board("종목분석", 1),
      board("자유게시판", 2),
      board("질문/답변", 3),
    ],
  },
  {
    name: "개미투자 카페",
    cafeRef: "cafe.naver.com/antinvest",
    cafeId: 22222222,
    boards: [
      board("자유게시판", 1),
      board("정보 공유", 2),
      board("종목추천", 3),
    ],
  },
];

const SEED_BANDS: Band[] = [
  { name: "가치투자모임 BAND" },
  { name: "단타클럽 BAND" },
  { name: "주식스터디 BAND" },
];

const SEED_STATS: DashStat[] = [
  {
    key: "accounts",
    label: "운영 계정",
    value: 11,
    sub: "전체 15개 · 오류 1",
    icon: "users",
    color: "blue",
  },
  {
    key: "scheduled",
    label: "예약 대기",
    value: 6,
    sub: "다음 게시 1시간 후",
    icon: "clock",
    color: "yellow",
  },
  {
    key: "today",
    label: "오늘 게시 완료",
    value: 34,
    sub: "글 9 · 댓글 25",
    icon: "send",
    color: "green",
  },
  {
    key: "rate",
    label: "게시 성공률",
    value: "97.4%",
    sub: "최근 7일",
    icon: "checkCircle",
    color: "forum",
  },
];

const SEED_ACTIVITY: ActivityItem[] = [
  {
    id: "ac1",
    type: "success",
    text: "‘삼성전자 4분기 실적 기대’ 글이 종목토론방에 게시되었습니다",
    time: "12분 전",
  },
  {
    id: "ac2",
    type: "success",
    text: "반도체 코멘트 10종이 2개 계정에 분산 게시되었습니다",
    time: "1시간 전",
  },
  {
    id: "ac3",
    type: "error",
    text: "한미반도체 토론방 계정 게시 실패 — 로그인 세션 만료",
    time: "2시간 전",
  },
  {
    id: "ac4",
    type: "info",
    text: "종목토론방 12개를 크롤링해 가져왔습니다",
    time: "3시간 전",
  },
  {
    id: "ac5",
    type: "info",
    text: "엑셀에서 계정 4건을 가져왔습니다",
    time: "어제",
  },
];

const SEED_QUEUE_NOW: QueueNowItem[] = [
  {
    id: "q1",
    title: "삼성전자 4분기 실적 기대 — 매수 관점 정리",
    kind: "post",
    state: "running",
    batchId: "b0",
    progress: [2, 3],
    locs: [
      { p: "forum", name: "삼성전자", code: "005930" },
      { p: "forum", name: "SK하이닉스", code: "000660" },
      { p: "naver", name: "주식투자연구소 카페" },
    ],
  },
  {
    id: "q2",
    title: "반도체 흐름 코멘트 10종",
    kind: "comment",
    state: "waiting",
    locs: [
      { p: "forum", name: "SK하이닉스", code: "000660" },
      { p: "forum", name: "한미반도체", code: "042700" },
    ],
  },
  {
    id: "q3",
    title: "오늘의 특징주 정리 — 장 마감 요약",
    kind: "post",
    state: "waiting",
    locs: [
      { p: "naver", name: "개미투자 카페" },
      { p: "band", name: "가치투자모임 BAND" },
    ],
  },
  {
    id: "q4",
    title: "2차전지 섹터 기대감 코멘트 세트",
    kind: "comment",
    state: "waiting",
    locs: [{ p: "forum", name: "POSCO홀딩스", code: "005490" }],
  },
  {
    id: "q5",
    title: "카카오 반등 시그널 분석",
    kind: "post",
    state: "waiting",
    locs: [{ p: "forum", name: "카카오", code: "035720" }],
  },
];

const SEED_QUEUE_SCHEDULED: QueueScheduledItem[] = [
  {
    id: "qs1",
    title: "에코프로 조정 구간 대응 전략",
    kind: "both",
    when: "오늘 18:30",
    rel: "5시간 후",
    locs: [{ p: "forum", name: "에코프로", code: "086520" }],
  },
  {
    id: "qs2",
    title: "이번 주 시장 브리핑 정리",
    kind: "post",
    when: "내일 09:00",
    rel: "내일",
    locs: [
      { p: "naver", name: "주식투자연구소 카페" },
      { p: "band", name: "가치투자모임 BAND" },
    ],
  },
  {
    id: "qs3",
    title: "HBM 관련 기대 코멘트",
    kind: "comment",
    when: "5/31 20:00",
    rel: "모레",
    locs: [{ p: "forum", name: "한미반도체", code: "042700" }],
  },
];

const SEED_LIBRARY: LibraryPost[] = [
  {
    id: "l1",
    title: "#{종목명} 4분기 실적 기대 — 매수 관점 정리",
    kind: "post",
    updated: "방금 전",
    words: 280,
    status: "ready",
    excerpt:
      "#{종목명}에 외국인 순매수가 다시 들어오고 있습니다. 4분기 업황 회복 관점에서 비중을 늘려가도 좋다고 봅니다.",
    body: "<p>#{종목명}에 외국인 순매수가 다시 들어오고 있습니다. 4분기 업황 회복 관점에서 비중을 늘려가도 좋다고 봅니다.</p><p>실적 발표 전까지는 분할로 모아가는 전략이 유효해 보입니다.</p><p>관련 시세는 여기서 확인하세요 → #{링크}</p>",
  },
  {
    id: "l2",
    title: "반도체 흐름 코멘트 모음 (10종)",
    kind: "comment",
    updated: "30분 전",
    words: 120,
    status: "ready",
    excerpt:
      "‘오늘 흐름 좋네요’, ‘저도 추가 매수했습니다’ 등 자연스러운 댓글 10종.",
    comments: [
      "오늘 흐름 좋네요 👍",
      "저도 오전에 추가 매수했습니다",
      "이 종목 계속 보고 있었는데 슬슬 들어가야겠네요",
      "관심종목 추가요",
      "반도체 사이클 이제 시작이라고 봅니다",
      "장기로 가져갑니다",
      "거래량 붙는 거 보니 기대되네요",
      "조정 오면 더 담을 생각입니다",
      "외인 수급 좋네요",
      "오늘도 화이팅입니다",
    ],
  },
  {
    id: "l3",
    title: "에코프로 조정 구간 대응 전략",
    kind: "both",
    updated: "방금 전",
    words: 842,
    status: "draft",
    excerpt:
      "단기 변동성이 커진 구간입니다. 분할 매수 관점과 손절 라인을 정리했습니다.",
    body: "<p>단기 변동성이 커진 구간입니다. 무리한 추격보다는 분할 매수 관점으로 접근하는 것이 좋겠습니다.</p><blockquote>손절 라인은 직전 저점 기준으로 잡는 것을 추천합니다.</blockquote>",
    comments: [
      "분할 매수 관점 동의합니다",
      "손절 라인 참고할게요",
      "좋은 정리 감사합니다",
    ],
  },
  {
    id: "l4",
    title: "이번 주 시장 브리핑 정리",
    kind: "post",
    updated: "10분 전",
    words: 520,
    status: "draft",
    excerpt: "금리, 환율, 주요 일정까지 이번 주 시장을 한눈에 정리했습니다.",
    body: "<p>금리, 환율, 주요 일정까지 이번 주 시장을 한눈에 정리했습니다.</p>",
  },
  {
    id: "l5",
    title: "2차전지 섹터 코멘트 세트",
    kind: "comment",
    updated: "어제",
    words: 90,
    status: "published",
    excerpt: "소재주 중심으로 자연스럽게 분위기를 띄우는 댓글 세트입니다.",
    comments: [
      "소재주 흐름 좋네요",
      "장기적으로 봅니다",
      "이 섹터 다시 보고 있습니다",
    ],
  },
  {
    id: "l6",
    title: "카카오 반등 시그널 분석",
    kind: "post",
    updated: "2시간 전",
    words: 470,
    status: "ready",
    excerpt: "거래량과 차트 관점에서 단기 반등 가능성을 점검했습니다.",
    body: "<p>거래량과 차트 관점에서 단기 반등 가능성을 점검했습니다.</p>",
  },
  {
    id: "l7",
    title: "HBM 관련 기대감 코멘트",
    kind: "comment",
    updated: "3시간 전",
    words: 60,
    status: "draft",
    excerpt: "HBM 수요 관련 긍정적 분위기를 만드는 댓글 모음.",
    comments: [
      "HBM 기대됩니다",
      "수요 계속 늘어날 듯",
      "관련주 같이 보는 중입니다",
    ],
  },
  {
    id: "l8",
    title: "장 마감 요약 — 오늘의 특징주",
    kind: "post",
    updated: "3일 전",
    words: 360,
    status: "published",
    excerpt: "오늘 시장에서 눈에 띈 종목들을 간단히 정리했습니다.",
    body: "<p>오늘 시장에서 눈에 띈 종목들을 간단히 정리했습니다.</p>",
  },
  {
    id: "l9",
    title: "금리 인하 기대감과 성장주 흐름",
    kind: "post",
    updated: "4시간 전",
    words: 430,
    status: "ready",
    excerpt: "연준 기조 변화에 따른 성장주 대응 전략을 정리했습니다.",
    body: "<p>연준 기조 변화에 따른 성장주 대응 전략을 정리했습니다.</p>",
  },
  {
    id: "l10",
    title: "관심종목 응원 코멘트 세트",
    kind: "comment",
    updated: "5시간 전",
    words: 70,
    status: "ready",
    excerpt: "‘오늘도 화이팅’, ‘가즈아’ 등 가벼운 응원 댓글 모음.",
    comments: [
      "오늘도 화이팅입니다",
      "가즈아!",
      "끝까지 홀딩합니다",
      "내일도 기대돼요",
      "좋은 흐름 이어가길",
    ],
  },
  {
    id: "l11",
    title: "실적 발표 D-1 점검 + 댓글",
    kind: "both",
    updated: "6시간 전",
    words: 520,
    status: "draft",
    excerpt: "실적 발표 전 체크포인트를 정리하고 분위기 댓글까지 준비했습니다.",
    body: "<p>실적 발표 전 체크포인트를 정리했습니다. 가이던스와 마진 추이를 주목하세요.</p>",
    comments: [
      "내일 실적 기대됩니다",
      "가이던스가 관건이네요",
      "미리 정리 감사합니다",
    ],
  },
  {
    id: "l12",
    title: "배당주 시즌 정리 — 고배당 리스트",
    kind: "post",
    updated: "어제",
    words: 610,
    status: "ready",
    excerpt: "배당 시즌을 앞두고 눈여겨볼 고배당 종목을 정리했습니다.",
    body: "<p>배당 시즌을 앞두고 눈여겨볼 고배당 종목을 정리했습니다.</p>",
  },
  {
    id: "l13",
    title: "급등주 추격 자제 코멘트",
    kind: "comment",
    updated: "어제",
    words: 80,
    status: "published",
    excerpt: "과열 구간 추격을 경계하는 차분한 댓글 세트.",
    comments: ["추격은 신중하게요", "분할로 접근합시다", "조정 기다려봅니다"],
  },
  {
    id: "l14",
    title: "주간 포트폴리오 리밸런싱 메모",
    kind: "post",
    updated: "2일 전",
    words: 380,
    status: "draft",
    excerpt: "이번 주 비중 조정 계획을 간단히 메모했습니다.",
    body: "<p>이번 주 비중 조정 계획을 간단히 메모했습니다.</p>",
  },
];

const SEED_LOG_BATCHES: LogBatch[] = [
  {
    id: "b0",
    title: "삼성전자 4분기 실적 기대 — 매수 관점 정리",
    kind: "post",
    time: "방금 전",
    state: "running",
    items: [
      {
        platform: "forum",
        target: "삼성전자",
        code: "005930",
        loginId: "invest_king7",
        status: "success",
        msg: "게시 완료",
      },
      {
        platform: "forum",
        target: "SK하이닉스",
        code: "000660",
        loginId: "value_pick",
        status: "success",
        msg: "게시 완료",
      },
      {
        platform: "naver",
        target: "주식투자연구소 카페",
        board: "종목분석",
        loginId: "money_lab",
        status: "running",
        msg: "게시 중…",
      },
    ],
  },
  {
    id: "b1",
    title: "5월 이벤트 결과 발표",
    kind: "post",
    time: "오늘 13:48",
    items: [
      {
        platform: "forum",
        target: "삼성전자",
        code: "005930",
        loginId: "invest_king7",
        status: "success",
        msg: "게시 완료",
      },
      {
        platform: "naver",
        target: "주식투자연구소 카페",
        board: "종목분석",
        loginId: "money_lab",
        status: "success",
        msg: "게시 완료",
      },
      {
        platform: "band",
        target: "가치투자모임 BAND",
        loginId: "value_invest",
        status: "success",
        msg: "게시 완료",
      },
    ],
  },
  {
    id: "b2",
    title: "반도체 흐름 코멘트 10종",
    kind: "comment",
    time: "오늘 13:42",
    items: [
      {
        platform: "forum",
        target: "SK하이닉스",
        code: "000660",
        loginId: "value_pick",
        status: "success",
        msg: "댓글 3건 게시",
      },
      {
        platform: "forum",
        target: "한미반도체",
        code: "042700",
        loginId: "day_trader_x",
        status: "fail",
        msg: "로그인 세션 만료",
        trace:
          "NaverAuthError: session expired (HTTP 302 → /login)\n    at AuthClient.ensureSession (auth.js:88:13)\n    at async CommentJob.run (jobs/comment.js:142:5)\n    at async Queue.process (queue/runner.js:51:9)\n  hint: 계정 재로그인 후 자동 재시도됩니다.",
      },
    ],
  },
  {
    id: "b3",
    title: "오늘의 특징주 정리",
    kind: "post",
    time: "오늘 12:15",
    items: [
      {
        platform: "naver",
        target: "개미투자 카페",
        board: "자유게시판",
        loginId: "stock_daily",
        status: "success",
        msg: "게시 완료",
      },
    ],
  },
  {
    id: "b4",
    title: "장중 코멘트 세트",
    kind: "comment",
    time: "오늘 11:30",
    items: [
      {
        platform: "forum",
        target: "POSCO홀딩스",
        code: "005490",
        loginId: "chart_master",
        status: "success",
        msg: "댓글 5건 게시",
      },
      {
        platform: "forum",
        target: "LG에너지솔루션",
        code: "373220",
        loginId: "chart_master",
        status: "success",
        msg: "댓글 5건 게시",
      },
    ],
  },
  {
    id: "b5",
    title: "관심 종목 코멘트",
    kind: "comment",
    time: "어제 19:02",
    items: [
      {
        platform: "naver",
        target: "주식투자연구소 카페",
        board: "종목분석",
        loginId: "money_lab",
        status: "fail",
        msg: "도배 방지 차단",
        trace:
          "RateLimitError: 작성 간격 제한 (cool-down 300s)\n    at SpamGuard.check (guard.js:30:11)\n    at async CommentJob.run (jobs/comment.js:120:5)\n  hint: 게시 간격을 늘리거나 잠시 후 재시도하세요.",
      },
      {
        platform: "forum",
        target: "셀트리온",
        code: "068270",
        loginId: "hot_trend22",
        status: "success",
        msg: "댓글 게시 완료",
      },
    ],
  },
  {
    id: "b6",
    title: "차트 관점 분석",
    kind: "post",
    time: "어제 20:40",
    items: [
      {
        platform: "forum",
        target: "카카오",
        code: "035720",
        loginId: "invest_king7",
        status: "success",
        msg: "게시 완료",
      },
    ],
  },
  {
    id: "b7",
    title: "주간 시장 브리핑",
    kind: "post",
    time: "5/27 22:30",
    items: [
      {
        platform: "naver",
        target: "개미투자 카페",
        board: "정보 공유",
        loginId: "stock_daily",
        status: "success",
        msg: "게시 완료",
      },
      {
        platform: "band",
        target: "가치투자모임 BAND",
        loginId: "value_invest",
        status: "success",
        msg: "게시 완료",
      },
    ],
  },
];

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

interface IpcState {
  accounts: Account[];
  posts: LibraryPost[];
  queueNow: QueueNowItem[];
  queueScheduled: QueueScheduledItem[];
  cafes: Cafe[];
}

let state: IpcState;

/** Re-seed the in-memory backend to the pristine dataset. Call in `beforeEach`. */
export function resetIpc(): void {
  state = {
    accounts: clone(SEED_ACCOUNTS),
    posts: clone(SEED_LIBRARY),
    queueNow: clone(SEED_QUEUE_NOW),
    queueScheduled: clone(SEED_QUEUE_SCHEDULED),
    cafes: clone(SEED_CAFES),
  };
}

resetIpc();

/** Drop-in replacement for `@tauri-apps/api/core`'s `invoke`, backed by fixtures. */
export const invoke = vi.fn(
  async (cmd: string, args?: Record<string, unknown>): Promise<unknown> => {
    switch (cmd) {
      // --- read-only domains -------------------------------------------------
      case "list_stocks":
        return clone(SEED_STOCKS);
      case "list_activity":
        return clone(SEED_ACTIVITY);
      case "list_stats":
        return clone(SEED_STATS);
      case "list_log_batches":
        return clone(SEED_LOG_BATCHES);
      case "list_cafes":
        return clone(state.cafes);
      case "resolve_cafe": {
        const input = args!.input as string;
        const resolved: Cafe = {
          name: `해석된 카페 (${input})`,
          cafeRef: input,
          cafeId: 31732304,
          boards: [board("자유게시판", 1), board("공지사항", 2)],
        };
        return clone(resolved);
      }
      case "upsert_cafe": {
        const cafe = args!.cafe as Cafe;
        const i = state.cafes.findIndex((c) => c.cafeId === cafe.cafeId);
        if (i >= 0) state.cafes[i] = cafe;
        else state.cafes = [cafe, ...state.cafes];
        return clone(state.cafes);
      }
      case "run_post_jobs": {
        const jobs = args!.jobs as PostJob[];
        const outcomes: PublishOutcome[] = jobs.map((j, i) => ({
          accountId: j.accountId,
          cafe: j.cafe,
          menuId: j.menuId,
          success: true,
          articleId: 1000 + i,
        }));
        return clone(outcomes);
      }
      case "list_bands":
        return clone(SEED_BANDS);

      // --- accounts (stateful) ----------------------------------------------
      case "list_accounts":
        return clone(state.accounts);
      case "add_account":
        state.accounts = [...state.accounts, args!.account as Account];
        return clone(state.accounts);
      case "update_account": {
        const acc = args!.account as Account;
        state.accounts = state.accounts.map((a) => (a.id === acc.id ? acc : a));
        return clone(state.accounts);
      }
      case "delete_accounts": {
        const ids = args!.ids as string[];
        state.accounts = state.accounts.filter((a) => !ids.includes(a.id));
        return clone(state.accounts);
      }

      // --- posts (stateful) --------------------------------------------------
      case "list_posts":
        return clone(state.posts);
      case "upsert_post": {
        const post = args!.post as LibraryPost;
        const i = state.posts.findIndex((p) => p.id === post.id);
        state.posts =
          i < 0
            ? [post, ...state.posts]
            : state.posts.map((p) => (p.id === post.id ? post : p));
        return clone(state.posts);
      }
      case "delete_post":
        state.posts = state.posts.filter((p) => p.id !== (args!.id as string));
        return clone(state.posts);

      // --- queue (stateful) --------------------------------------------------
      case "list_queue_now":
        return clone(state.queueNow);
      case "list_queue_scheduled":
        return clone(state.queueScheduled);
      case "cancel_queue_now":
        state.queueNow = state.queueNow.filter(
          (q) => q.id !== (args!.id as string),
        );
        return clone(state.queueNow);
      case "cancel_queue_scheduled":
        state.queueScheduled = state.queueScheduled.filter(
          (q) => q.id !== (args!.id as string),
        );
        return clone(state.queueScheduled);
      case "add_queue_scheduled":
        state.queueScheduled = [
          ...state.queueScheduled,
          args!.item as QueueScheduledItem,
        ];
        return clone(state.queueScheduled);
      case "promote_queue_scheduled": {
        const id = args!.id as string;
        const item = state.queueScheduled.find((q) => q.id === id);
        state.queueScheduled = state.queueScheduled.filter((q) => q.id !== id);
        if (item) {
          state.queueNow = [
            ...state.queueNow,
            {
              id: item.id,
              title: item.title,
              kind: item.kind,
              state: "waiting",
              locs: item.locs,
            },
          ];
        }
        return clone(state.queueNow);
      }

      default:
        throw new Error(`test ipc: unhandled command "${cmd}"`);
    }
  },
);
