import { vi } from "vitest";

import type { Article } from "@/shared/bindings/Article";
import type { ArticleListResponse } from "@/shared/bindings/ArticleListResponse";
import type { CommentDistributionRequest } from "@/shared/bindings/CommentDistributionRequest";
import type { CommentPublishOutcome } from "@/shared/bindings/CommentPublishOutcome";
import type { EnvironmentStatus } from "@/shared/bindings/EnvironmentStatus";
import type { JoinedCafe } from "@/shared/bindings/JoinedCafe";
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

// Fixed base timestamp for deterministic seed data (2023-11-14T22:13:20.000Z).
const NOW_BASE = 1_700_000_000_000;

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

const joinedCafe = (
  cafeId: number,
  cafeName: string,
  cafeUrl: string,
  levelname = "정회원",
): JoinedCafe => ({
  cafeId,
  cafeName,
  cafeUrl,
  memberNickname: "회원",
  memberLevelname: levelname,
  managingCafe: false,
  dormantCafe: false,
});

// Joined cafes per naver account — keyed by accountId so the publish modal's
// account-driven loader returns a different list for each account.
// 백엔드 계약상 가입 카페는 계정의 `loginId`(쿠키 파일 키)로 조회된다 — UI 내부
// 고유 id(a5 등)가 아니다. 시드도 loginId로 키한다.
const SEED_JOINED: Record<string, JoinedCafe[]> = {
  money_lab: [
    joinedCafe(11111111, "주식투자연구소 카페", "stocklab", "카페매니저"),
    joinedCafe(22222222, "개미투자 카페", "antinvest"),
  ],
  insight_note: [joinedCafe(33333333, "가치투자 모임", "valueclub")],
  cafe_master9: [joinedCafe(44444444, "차트분석 카페", "chartlab")],
};

const article = (articleId: number, subject: string, menuId = 1): Article => ({
  articleId,
  subject,
  writerNickname: "회원",
  menuId,
  menuName: "자유게시판",
  commentCount: 0,
  readCount: 100,
  likeCount: 5,
  writeDateTimestamp: 1_700_000_000_000 + articleId,
});

// Latest/popular article fixtures per cafeId (keyed by the string id the modal
// passes). 10 entries so tests can exercise N=1/3/5/10 top-N extraction; the
// `popular` sort just reverses the order so latest≠popular is observable.
const SEED_ARTICLES_DEFAULT: Article[] = Array.from({ length: 10 }, (_, i) =>
  article(9000 + i, `게시글 ${i + 1}`),
);

const SEED_ARTICLES: Record<string, Article[]> = {
  "11111111": Array.from({ length: 10 }, (_, i) =>
    article(8000 + i, `주식투자연구소 글 ${i + 1}`),
  ),
  // 게시글이 2건뿐인 카페 — N보다 적을 때의 폴백 검증용.
  "22222222": [article(7000, "개미투자 글 1"), article(7001, "개미투자 글 2")],
};

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
    text: "’삼성전자 4분기 실적 기대’ 글이 종목토론방에 게시되었습니다",
    at: NOW_BASE - 12 * 60_000,
  },
  {
    id: "ac2",
    type: "success",
    text: "반도체 코멘트 10종이 2개 계정에 분산 게시되었습니다",
    at: NOW_BASE - 3_600_000,
  },
  {
    id: "ac3",
    type: "error",
    text: "한미반도체 토론방 계정 게시 실패 — 로그인 세션 만료",
    at: NOW_BASE - 2 * 3_600_000,
  },
  {
    id: "ac4",
    type: "info",
    text: "종목토론방 12개를 크롤링해 가져왔습니다",
    at: NOW_BASE - 3 * 3_600_000,
  },
  {
    id: "ac5",
    type: "info",
    text: "엑셀에서 계정 4건을 가져왔습니다",
    at: NOW_BASE - 26 * 3_600_000,
  },
];

const SEED_ENV_STATUS: EnvironmentStatus = {
  chrome: {
    installed: true,
    path: "/mnt/c/Program Files/Google/Chrome/Application/chrome.exe",
    version: "125.0.6422.142",
    error: null,
  },
  adb: { connected: true, error: null },
};

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
    at: NOW_BASE - 2 * 60_000,
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
    body: "<p>5월 이벤트 결과를 정리했습니다. 많은 참여 감사드립니다.</p>",
    comment: "이벤트 참여 감사합니다 🙌",
    kind: "post",
    at: NOW_BASE - 4 * 3_600_000,
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
    at: NOW_BASE - 5 * 3_600_000,
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
    at: NOW_BASE - 7 * 3_600_000,
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
    at: NOW_BASE - 8 * 3_600_000,
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
    at: NOW_BASE - 25 * 3_600_000,
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
    at: NOW_BASE - 26 * 3_600_000,
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
    at: NOW_BASE - 8 * 24 * 3_600_000,
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
  activity: ActivityItem[];
  cafes: Cafe[];
}

let state: IpcState;
// 로그인 큐에 enqueue된 계정 id (get_queue_status가 같은 id로 잡을 돌려주도록 보관).
let loginJobIds: string[] = [];
// 밴드 로그인 큐에 enqueue된 계정 id (get_band_queue_status용, 네이버 큐와 분리).
let bandLoginJobIds: string[] = [];
// 테스트에서 특정 계정의 로그인 결과를 실패 등으로 시뮬레이션하기 위한 오버라이드.
// accountId → { status, message }. 미지정 계정은 success로 본다.
let loginOutcomes: Record<string, { status: string; message: string }> = {};
// 테스트에서 특정 카페의 최신/인기글 목록 조회를 실패시키기 위한 cafeId(문자열) 집합.
let articleListFailures = new Set<string>();

/**
 * Override login-queue outcomes for specific accounts (e.g. simulate a failure).
 * Unset accounts keep the default `success`. Cleared by `resetIpc`.
 */
export function setLoginOutcomes(
  outcomes: Record<string, { status: string; message: string }>,
): void {
  loginOutcomes = outcomes;
}

/**
 * Make `list_cafe_articles` reject for the given cafeIds (string form, as the
 * modal passes them) to simulate a list-fetch failure. Cleared by `resetIpc`.
 */
export function setArticleListFailures(cafeIds: string[]): void {
  articleListFailures = new Set(cafeIds);
}

/** Re-seed the in-memory backend to the pristine dataset. Call in `beforeEach`. */
export function resetIpc(): void {
  state = {
    accounts: clone(SEED_ACCOUNTS),
    posts: clone(SEED_LIBRARY),
    queueNow: clone(SEED_QUEUE_NOW),
    queueScheduled: clone(SEED_QUEUE_SCHEDULED),
    activity: clone(SEED_ACTIVITY),
    cafes: clone(SEED_CAFES),
  };
  loginJobIds = [];
  bandLoginJobIds = [];
  loginOutcomes = {};
  articleListFailures = new Set();
}

resetIpc();

/** Login-queue status mirroring the auth queue: every enqueued account succeeds by default. */
function loginQueueStatus() {
  return {
    isRunning: false,
    currentAccountId: null,
    jobs: loginJobIds.map((accountId) => {
      const outcome = loginOutcomes[accountId];
      return {
        accountId,
        status: outcome?.status ?? "success",
        message: outcome?.message ?? "success",
      };
    }),
  };
}

// 밴드 로그인 큐 상태(네이버와 동일 형태, 별도 job 목록). loginOutcomes 오버라이드 공유.
function bandQueueStatus() {
  return {
    isRunning: false,
    currentAccountId: null,
    jobs: bandLoginJobIds.map((accountId) => {
      const outcome = loginOutcomes[accountId];
      return {
        accountId,
        status: outcome?.status ?? "success",
        message: outcome?.message ?? "success",
      };
    }),
  };
}

/** Drop-in replacement for `@tauri-apps/api/core`'s `invoke`, backed by fixtures. */
export const invoke = vi.fn(
  async (cmd: string, args?: Record<string, unknown>): Promise<unknown> => {
    switch (cmd) {
      // --- read-only domains -------------------------------------------------
      case "list_stocks":
        return clone(SEED_STOCKS);
      case "search_stocks": {
        const query = ((args?.query as string | undefined) ?? "").trim();
        const candidates = SEED_STOCKS.map((s) => ({
          name: s.name,
          code: s.code,
          link: "",
        }));
        return clone(
          query
            ? candidates.filter(
                (c) => c.name.includes(query) || c.code.includes(query),
              )
            : candidates,
        );
      }
      case "list_activity":
        return clone(state.activity);
      case "append_activity": {
        state.activity.unshift({
          id: `ac-${state.activity.length}`,
          type: args!.kind as "success" | "error" | "info",
          text: args!.text as string,
          at: NOW_BASE,
        });
        return undefined;
      }
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
      case "list_joined_cafes": {
        const accountId = args!.accountId as string;
        return clone(SEED_JOINED[accountId] ?? []);
      }
      case "list_cafe_articles": {
        // 최신글/인기글 목록. cafeId(문자열)·sortBy로 키해 시드를 돌려주고,
        // 미지정 카페는 기본 목록을 쓴다. 테스트에서 N개 추출/폴백을 검증할 수 있게
        // 충분한 건수를 둔다.
        const cafeId = args!.cafeId as string;
        const sortBy = args!.sortBy as string;
        if (articleListFailures.has(cafeId)) {
          throw new Error(`목록 조회 실패 (cafe ${cafeId})`);
        }
        const seed = SEED_ARTICLES[cafeId] ?? SEED_ARTICLES_DEFAULT;
        const articles = sortBy === "popular" ? [...seed].reverse() : seed;
        const response: ArticleListResponse = {
          articles: clone(articles),
        };
        return clone(response);
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
      case "run_comment_jobs": {
        // 백엔드가 댓글 풀을 분배하므로(이슈 #98) 목은 타깃마다 성공 1건만 만든다.
        const req = args!.req as CommentDistributionRequest;
        const outcomes: CommentPublishOutcome[] = req.targets.map((t, i) => ({
          accountId: t.accountId,
          cafeId: t.cafeId,
          articleId: t.articleId,
          success: true,
          commentId: 2000 + i,
        }));
        return clone(outcomes);
      }
      case "list_bands":
        return clone(SEED_BANDS);
      case "band_publish": {
        // 밴드 가입+게시 목: 링크에서 band_no를 뽑아 성공 결과를 만든다.
        const link = String(args!.bandLink ?? "");
        const m = link.match(/\/band\/(\d+)|^(\d+)$/);
        const bandNo = m ? (m[1] ?? m[2]) : "0";
        return clone({
          joined: true,
          postNo: 1,
          webUrl: `https://band.us/band/${bandNo}/post/1`,
          commented: Boolean(String(args!.comment ?? "").trim()),
          bandName: `밴드 ${bandNo}`,
        });
      }
      case "get_environment_status":
        return clone(SEED_ENV_STATUS);
      case "open_chrome_download":
        // 브라우저 열기는 사이드이펙트뿐 — 목에서는 성공(void)으로 처리.
        return undefined;

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
      case "reorder_queue_now": {
        const orderedIds = args!.orderedIds as string[];
        const running = state.queueNow.filter((q) => q.state === "running");
        const rest = state.queueNow.filter((q) => q.state !== "running");
        const ordered: QueueNowItem[] = [];
        for (const id of orderedIds) {
          const idx = rest.findIndex((q) => q.id === id);
          if (idx >= 0) ordered.push(rest.splice(idx, 1)[0]!);
        }
        ordered.push(...rest); // 알 수 없는/누락 id는 원래 순서로 보존
        state.queueNow = [...running, ...ordered];
        return clone(state.queueNow);
      }

      // --- forum 게시 엔드포인트 (백엔드 소유) -----------------------------
      case "forum_endpoint":
        return { host: "127.0.0.1", port: 9222 };

      // --- forum 즉시 게시 (엔진 호출 모킹) ---------------------------------
      // 실제 백엔드는 패킷 게시를 수행한다. 테스트에서는 기존 목업과 동일하게
      // Math.random으로 성공/실패를 정하고, "게시하는 중" 상태가 보이도록 약간 지연한다.
      case "run_forum_publish_now": {
        const req = args!.request as {
          stocks: { code: string; name: string }[];
        };
        return new Promise((resolve) =>
          setTimeout(
            () =>
              resolve(
                req.stocks.map((s) => {
                  const ok = Math.random() > 0.1;
                  return {
                    code: s.code,
                    name: s.name,
                    ok,
                    message: ok ? "게시 완료" : "게시 실패 — 잠시 후 재시도",
                  };
                }),
              ),
            800,
          ),
        );
      }

      // --- 엑셀 내보내기 (모킹 — 실제 파일 쓰기 없이 성공 반환) -----------
      case "export_accounts_xlsx":
      case "export_activity_xlsx":
        return undefined;

      // --- 엑셀 가져오기 (모킹 — canned summary 반환) ----------------------
      case "import_accounts_xlsx":
      case "import_posts_xlsx":
        return { imported: 2, skipped: 0, errors: [] };

      // --- 네이버 로그인 자동화 (모킹) -------------------------------------
      case "bootstrap_runtime":
        return {};
      case "save_accounts":
        return clone(args!.accounts);
      case "enqueue_cookie_refresh":
        loginJobIds = (args?.accountIds as string[] | undefined) ?? [];
        return loginQueueStatus();
      case "get_queue_status":
        return loginQueueStatus();
      case "enqueue_band_login":
        bandLoginJobIds = (args?.accountIds as string[] | undefined) ?? [];
        return bandQueueStatus();
      case "get_band_queue_status":
        return bandQueueStatus();

      default:
        throw new Error(`test ipc: unhandled command "${cmd}"`);
    }
  },
);
