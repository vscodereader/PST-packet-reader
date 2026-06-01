# 예약 날짜·시간 피커 (Scheduled Date/Time Picker) — 설계

작성일: 2026-06-01

## 배경 / 목표

게시 설정 모달에서 "예약" 게시를 선택하면 현재는 브라우저 기본
`input[type=date]` + `input[type=time]`로 날짜·시간을 받는다. 이를 앱 디자인과
통일된 **커스텀 날짜·시간 피커**로 대체한다.

- 사용자가 트리거를 클릭하면 팝업(Popover)이 떠서, **월간 달력으로 날짜를**,
  **스테퍼+직접입력으로 시간을** 설정한다.
- 별도의 "예약 현황 캘린더"(큐 화면 월간 뷰)는 **이번 범위에서 제외**한다.

비목표(out of scope): 예약 현황 캘린더, 대시보드 데이터 연동, 반복 예약,
타임존 처리, 새 npm 의존성 추가, Rust/IPC 변경.

## 범위 & 위치

- 파일: `src/features/posts/publish-modal.tsx` — 예약 모드(`when === "schedule"`)
  에서만 노출되는 기존 두 입력(대략 821~832줄의 `input[type=date]`,
  `input[type=time]`)을 신규 `DateTimePicker` 하나로 대체.
- 데이터 모델 변경 없음: 모달은 계속 `date: "YYYY-MM-DD"`, `time: "HH:MM"`
  문자열 상태를 들고, 예약 요약 문구(`${date} ${time}에 자동 게시됩니다`)도
  기존 포맷을 그대로 유지한다.
- IPC/Rust/바인딩 변경 없음. 순수 프론트엔드 UI.

## 컴포넌트

모두 `src/shared/ui/` 아래에 두고 각각 인접 `*.test.tsx`를 둔다. 순수 날짜
계산만 사용하고 Mantine 프리미티브(`Popover`, `Box`, `Group`, `ActionIcon`,
`TextInput`, `Button`)로 구성한다. 외부 날짜 라이브러리(`@mantine/dates`,
`dayjs`)는 도입하지 않는다.

### 1. `MonthCalendar`

표준 월간 그리드.

- Props: `value: Date`(선택일), `onChange(date: Date)`, `minDate?: Date`(기본=오늘).
- 열은 일~토 7칸. 해당 월 첫 주의 선행 빈칸과 마지막 주 후행 빈칸은 흐리게
  표시(이전/다음 달 날짜 또는 빈 셀).
- `‹` `›`로 표시 월 이동(선택 값과 별개의 "표시 중인 월" 내부 상태).
- 오늘 날짜는 외곽선/점으로 표시, 선택일은 파란 채움.
- `minDate` 이전 날짜는 비활성(클릭 불가, 흐리게).

### 2. `TimeStepper`

시·분 두 칸.

- Props: `value: { h: number; m: number }`, `onChange(v: { h: number; m: number })`.
- 각 칸: 위 `▲` / 아래 `▼` 스테퍼 + 가운데 2자리 숫자 직접 입력.
- 시: 0–23, ▲▼는 ±1, 경계 순환(23▲→00, 00▼→23).
- 분: 0–59, ▲▼는 ±5(5분 스냅), 경계 순환(55▲→00, 00▼→55). 직접 입력은
  0–59 자유 값 허용.
- 직접 입력 보정: 숫자 외 입력 무시, 범위를 벗어나면 클램프(시 0–23, 분 0–59).
  빈 값/blur 시 마지막 유효값 유지.

### 3. `DateTimePicker`

위 둘을 합치는 컨트롤드 컴포넌트.

- Props: `date: string("YYYY-MM-DD")`, `time: string("HH:MM")`,
  `onChange(next: { date: string; time: string })`, `minDate?: Date`.
- 트리거: 현재 값을 사람이 읽기 쉬운 형식(예: `5월 29일 (금) 18:00`)으로 보여주는
  버튼. 캘린더 아이콘 포함.
- 클릭 시 `Popover`가 열리고 좌측에 `MonthCalendar`, 우측에 `TimeStepper`를 배치.
- 하단에 "확인" 버튼(또는 외부 클릭)으로 닫는다. 값 변경은 즉시 `onChange`로
  상위에 반영(확인은 닫기 용도).
- 내부에서 `date`/`time` 문자열 ↔ `Date`/`{h,m}` 변환을 담당. 상위(publish-modal)
  의 상태 형태는 바뀌지 않는다.

## 데이터 흐름

```
publish-modal (date, time 문자열 상태)
   │  date, time, onChange, minDate
   ▼
DateTimePicker  ──(Date)──▶ MonthCalendar
   │            ──({h,m})─▶ TimeStepper
   ▼
onChange({date, time})  →  publish-modal setDate/setTime
```

## 동작 / 엣지 케이스

- 과거 날짜 선택 불가(`minDate` 기본=오늘). 시간은 제약 없음(같은 날 과거 시각도
  허용 — PoC 단순화).
- 월 이동은 선택 값을 바꾸지 않는다(달력만 이동). 다른 달의 날짜를 선택하면
  선택 값이 그 달로 갱신.
- 잘못된/빈 직접 입력은 마지막 유효값으로 보정.
- 날짜 포맷은 로컬 기준(타임존 변환 없음). `YYYY-MM-DD`는 로컬 연·월·일로 구성.

## 테스트 (TDD)

`src/shared/ui/` 인접 테스트. 결정성을 위해 실시간 `new Date()`에 의존하지 않도록
`value`/`minDate`를 명시 주입한다.

- `MonthCalendar.test.tsx`: 지정 월 렌더, 일 클릭 시 `onChange` 호출, 과거 날짜
  비활성/클릭 무시, `‹``›` 월 이동.
- `TimeStepper.test.tsx`: ▲▼ 증감, 시 23→00·분 55→00 순환, 분 5분 스냅, 직접
  입력 클램프(예: "99"→59).
- `DateTimePicker.test.tsx`: 트리거가 현재 값 표시, 클릭 시 팝업 열림, 날짜·시간
  선택이 `onChange({date,time})`로 전파.
- `publish-modal.test.tsx`: 예약 흐름이 깨지지 않도록 갱신(기본 date/time input을
  찾던 단언이 있으면 새 피커 기준으로 수정).

커버리지 게이트(lines 93 / stmts 93 / funcs 90 / branches 80)를 충족하도록
분기(순환·클램프·비활성)를 테스트로 덮는다.

## 영향 범위

- 신규: `src/shared/ui/{month-calendar,time-stepper,date-time-picker}.tsx`(+테스트).
- 수정: `src/features/posts/publish-modal.tsx`(입력 대체), 필요 시
  `publish-modal.test.tsx`.
- mock.ts/IPC/Rust 변경 없음.
