import {
  Alert,
  Badge,
  Button,
  Checkbox,
  Group,
  Stack,
  Text,
  Textarea,
  TextInput,
} from "@mantine/core";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useRef, useState } from "react";

type PickMode = "random" | "sequential" | "single";

type TemplateColumns = {
  titles: string[];
  bodies: string[];
  comments: string[];
};

type StockCandidate = {
  name: string;
  code: string;
  link: string;
};

type SavedBatchConfig = {
  stocks: StockCandidate[];
  runPost: boolean;
  runComment: boolean;
  titles: string[];
  bodies: string[];
  comments: string[];
  titleMode: PickMode;
  bodyMode: PickMode;
  commentMode: PickMode;
  count: 3 | 5;
  host: string;
  port: number;
  // 로그인 자동화로 저장된 계정 ID(선택). 지정하면 그 계정의 쿠키로 글/댓글을 작성합니다.
  accountId?: string;
};

// 실행 환경에 따라 Chrome DevTools 기본 접속값을 고르는 함수입니다.
function defaultDevtoolsEndpoint() {
  const platform = window.navigator.platform.toLowerCase();

  if (platform.includes("win")) {
    return { host: "127.0.0.1", port: 9222 };
  }

  return { host: "172.24.32.1", port: 9223 };
}

// 목록의 모든 항목 index를 선택 상태로 만드는 함수입니다.
function allEntryIndexes(entries: string[]) {
  return entries.map((_, index) => index);
}

// 선택된 index 목록을 실제 실행할 텍스트 목록으로 변환하는 함수입니다.
function entriesBySelection(entries: string[], indexes: number[]) {
  return indexes
    .filter((index) => index >= 0 && index < entries.length)
    .map((index) => entries[index]?.trim() ?? "")
    .filter(Boolean);
}

// 선택 모드 값을 화면에 표시할 한글 label로 바꾸는 함수입니다.
function modeLabel(mode: PickMode) {
  if (mode === "random") return "랜덤";
  if (mode === "sequential") return "순차";
  return "1개만";
}

// 제목, 내용, 댓글내용의 선택 모드가 실행 가능한 상태인지 검증하는 함수입니다.
function validateMode(entries: string[], mode: PickMode, label: string) {
  if (entries.length === 0) {
    return `${label}을 하나 이상 선택하세요. CSV를 가져온 뒤 목록에서 사용할 항목을 클릭하세요.`;
  }

  if (mode === "single" && entries.length !== 1) {
    return `${label}의 1개만은 선택된 값이 정확히 1개일 때만 선택할 수 있습니다.`;
  }

  return "";
}

// CSV 항목 목록을 표시하고 클릭 선택, 더블클릭 수정을 제공하는 컴포넌트입니다.
function EntryListEditor({
  entries,
  emptyLabel,
  label,
  onEntriesChange,
  onSelectionChange,
  selectedIndexes,
}: {
  entries: string[];
  emptyLabel: string;
  label: string;
  onEntriesChange: (entries: string[]) => void;
  onSelectionChange: (indexes: number[]) => void;
  selectedIndexes: number[];
}) {
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [editingValue, setEditingValue] = useState("");

  // 목록 항목을 클릭했을 때 선택/해제를 토글하는 함수입니다.
  function toggleSelection(index: number) {
    onSelectionChange(
      selectedIndexes.includes(index)
        ? selectedIndexes.filter((value) => value !== index)
        : [...selectedIndexes, index].sort((a, b) => a - b),
    );
  }

  // 목록 항목을 더블클릭했을 때 수정 모드로 전환하는 함수입니다.
  function beginEdit(index: number) {
    setEditingIndex(index);
    setEditingValue(entries[index] ?? "");
  }

  // 수정 모드에서 입력한 값을 목록에 저장하는 함수입니다.
  function saveEdit() {
    if (editingIndex === null) return;

    const nextEntries = [...entries];
    nextEntries[editingIndex] = editingValue.trim();
    onEntriesChange(nextEntries.filter(Boolean));
    setEditingIndex(null);
    setEditingValue("");
  }

  // 수정 모드를 취소하고 기존 값을 유지하는 함수입니다.
  function cancelEdit() {
    setEditingIndex(null);
    setEditingValue("");
  }

  return (
    <Stack gap={6}>
      <Text fw={700}>{label}</Text>
      <div
        className="macro-editor-entry-list"
        role="listbox"
        aria-label={label}
      >
        {entries.length === 0 ? (
          <Text c="dimmed" size="sm" className="macro-editor-entry-empty">
            {emptyLabel}
          </Text>
        ) : (
          entries.map((entry, index) => {
            const selected = selectedIndexes.includes(index);
            const editing = editingIndex === index;

            return (
              <div
                key={`${label}-${index}`}
                role="option"
                aria-selected={selected}
                tabIndex={0}
                className={`macro-editor-entry-item ${
                  selected ? "macro-editor-entry-item-selected" : ""
                }`}
                onClick={() => {
                  if (!editing) toggleSelection(index);
                }}
                onDoubleClick={() => beginEdit(index)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    toggleSelection(index);
                  }
                }}
              >
                {editing ? (
                  <div
                    className="macro-editor-entry-edit"
                    onClick={(event) => event.stopPropagation()}
                  >
                    <Textarea
                      autosize
                      minRows={1}
                      value={editingValue}
                      onChange={(event) =>
                        setEditingValue(event.currentTarget.value)
                      }
                    />
                    <Group gap="xs" justify="end">
                      <Button size="xs" onClick={saveEdit}>
                        저장
                      </Button>
                      <Button size="xs" variant="light" onClick={cancelEdit}>
                        취소
                      </Button>
                    </Group>
                  </div>
                ) : (
                  <>
                    <span className="macro-editor-entry-index">
                      {index + 1}
                    </span>
                    <span className="macro-editor-entry-text">{entry}</span>
                  </>
                )}
              </div>
            );
          })
        )}
      </div>
      <Text size="xs" c="dimmed" className="macro-editor-field-guide">
        CSV에서 가져온 항목은 목록으로 표시됩니다. 클릭하면 사용할 항목을
        선택하거나 해제할 수 있고, 더블클릭하면 내용을 수정할 수 있습니다. 한
        번에 최대 5개까지 보이며, 더 많은 항목은 목록 안에서 스크롤해서
        확인하세요.
      </Text>
    </Stack>
  );
}

// 랜덤, 순차, 1개만 선택 UI를 그리는 컴포넌트입니다.
function ModeSelector({
  entries,
  label,
  mode,
  onChange,
}: {
  entries: string[];
  label: string;
  mode: PickMode;
  onChange: (mode: PickMode) => void;
}) {
  const singleDisabled = entries.length !== 1;

  return (
    <Group gap="md" className="macro-editor-mode-row">
      {(["random", "sequential", "single"] as PickMode[]).map((value) => (
        <Checkbox
          key={value}
          checked={mode === value}
          disabled={value === "single" && singleDisabled}
          label={modeLabel(value)}
          onChange={() => onChange(value)}
          aria-label={`${label} ${modeLabel(value)}`}
        />
      ))}
    </Group>
  );
}

// CSV 가져오기, 종목 선택, batch 저장/실행을 담당하는 화면 컴포넌트입니다.
export function StockBatchPanel() {
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const defaultEndpoint = useMemo(defaultDevtoolsEndpoint, []);
  const [stockQuery, setStockQuery] = useState("");
  const [stockOptions, setStockOptions] = useState<StockCandidate[]>([]);
  const [selectedStocks, setSelectedStocks] = useState<StockCandidate[]>([]);
  const [showStockList, setShowStockList] = useState(false);
  const [fileLoaded, setFileLoaded] = useState(false);
  const [fileName, setFileName] = useState("");
  const [titleEntries, setTitleEntries] = useState<string[]>([]);
  const [bodyEntries, setBodyEntries] = useState<string[]>([]);
  const [commentEntries, setCommentEntries] = useState<string[]>([]);
  const [selectedTitleIndexes, setSelectedTitleIndexes] = useState<number[]>(
    [],
  );
  const [selectedBodyIndexes, setSelectedBodyIndexes] = useState<number[]>([]);
  const [selectedCommentIndexes, setSelectedCommentIndexes] = useState<
    number[]
  >([]);
  const [titleMode, setTitleMode] = useState<PickMode>("random");
  const [bodyMode, setBodyMode] = useState<PickMode>("random");
  const [commentMode, setCommentMode] = useState<PickMode>("random");
  const [runPost, setRunPost] = useState(false);
  const [runComment, setRunComment] = useState(false);
  const [count, setCount] = useState<3 | 5>(3);
  const [accountId, setAccountId] = useState("");
  const [savedConfig, setSavedConfig] = useState<SavedBatchConfig | null>(null);
  const [chromeOpening, setChromeOpening] = useState(false);
  const [running, setRunning] = useState(false);
  const [waitSecondsLeft, setWaitSecondsLeft] = useState<number | null>(null);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  const selectedTitles = useMemo(
    () => entriesBySelection(titleEntries, selectedTitleIndexes),
    [selectedTitleIndexes, titleEntries],
  );
  const selectedBodies = useMemo(
    () => entriesBySelection(bodyEntries, selectedBodyIndexes),
    [bodyEntries, selectedBodyIndexes],
  );
  const selectedComments = useMemo(
    () => entriesBySelection(commentEntries, selectedCommentIndexes),
    [commentEntries, selectedCommentIndexes],
  );
  const actionInvalid = Boolean(error) && !runPost && !runComment;
  const showPostPanel = runPost;
  const showCommentPanel = runComment;
  const showCountSelector = runPost || runComment;

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void loadStocks(stockQuery);
    }, 250);

    return () => window.clearTimeout(timer);
  }, [stockQuery]);

  // Rust가 sleep 직전 emit한 "batch-wait-start" 이벤트를 수신해 카운트다운 타이머를 시작합니다.
  useEffect(() => {
    let unlisten: (() => void) | null = null;

    void listen<{ seconds: number }>("batch-wait-start", (event) => {
      setWaitSecondsLeft(event.payload.seconds);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      unlisten?.();
    };
  }, []);

  // waitSecondsLeft가 양수인 동안 매초 1씩 감소시켜 카운트다운합니다.
  useEffect(() => {
    if (waitSecondsLeft === null || waitSecondsLeft <= 0) return;

    const timer = window.setTimeout(() => {
      setWaitSecondsLeft((current) =>
        current !== null && current > 1 ? current - 1 : null,
      );
    }, 1000);

    return () => window.clearTimeout(timer);
  }, [waitSecondsLeft]);

  // 네이버 증권 API 후보를 Rust command로 검색해서 종목 목록에 반영하는 함수입니다.
  async function loadStocks(query: string) {
    try {
      const stocks = await invoke<StockCandidate[]>("search_stocks", {
        query,
      });
      setStockOptions(stocks ?? []);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    }
  }

  // 일반 사용자가 IP/포트를 몰라도 되도록 시크릿 Chrome을 Rust에서 여는 함수입니다.
  async function openIncognitoChrome() {
    setError("");
    setMessage("");
    setChromeOpening(true);

    try {
      const result = await invoke<string>("open_incognito_chrome");
      setMessage(result);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      setChromeOpening(false);
    }
  }

  // 사용자가 선택한 CSV 파일을 읽고 Rust CSV parser 결과를 목록 UI에 반영하는 함수입니다.
  async function importTemplate(file: File) {
    setError("");
    setMessage("");

    if (!file.name.toLowerCase().endsWith(".csv")) {
      setError(
        "현재 가져오기는 CSV 파일만 지원합니다. 엑셀에서 CSV UTF-8 형식으로 저장한 뒤 가져오세요.",
      );
      return;
    }

    const csvText = await file.text();
    const parsed = await invoke<TemplateColumns>("parse_template_csv", {
      csvText,
    });

    setTitleEntries(parsed.titles);
    setBodyEntries(parsed.bodies);
    setCommentEntries(parsed.comments);
    setSelectedTitleIndexes(allEntryIndexes(parsed.titles));
    setSelectedBodyIndexes(allEntryIndexes(parsed.bodies));
    setSelectedCommentIndexes(allEntryIndexes(parsed.comments));
    setFileLoaded(true);
    setFileName(file.name);
    setMessage(
      `${file.name}에서 제목 ${parsed.titles.length}개, 내용 ${parsed.bodies.length}개, 댓글내용 ${parsed.comments.length}개를 가져왔습니다.`,
    );
  }

  // 검색 결과에서 고른 종목을 선택 목록에 추가하는 함수입니다.
  function addStock(stock: StockCandidate) {
    setSelectedStocks((current) =>
      current.some((item) => item.code === stock.code)
        ? current
        : [...current, stock],
    );
    setStockQuery("");
  }

  // 선택된 종목 chip을 눌렀을 때 해당 종목을 목록에서 제거하는 함수입니다.
  function removeStock(code: string) {
    setSelectedStocks((current) =>
      current.filter((stock) => stock.code !== code),
    );
  }

  // 현재 화면 값을 저장/실행 가능한 batch 설정 객체로 만드는 함수입니다.
  function buildConfig(): SavedBatchConfig | null {
    if (!fileLoaded) {
      setError("CSV 파일을 먼저 가져오세요.");
      return null;
    }

    if (selectedStocks.length === 0) {
      setError("종목을 하나 이상 선택하세요.");
      return null;
    }

    if (!runPost && !runComment) {
      setError("행동을 선택하세요.");
      return null;
    }

    const validations = [
      runPost ? validateMode(selectedTitles, titleMode, "제목") : "",
      runPost ? validateMode(selectedBodies, bodyMode, "내용") : "",
      runComment ? validateMode(selectedComments, commentMode, "댓글내용") : "",
    ].filter(Boolean);

    if (validations.length > 0) {
      setError(validations[0]!);
      return null;
    }

    return {
      bodies: selectedBodies,
      bodyMode,
      comments: selectedComments,
      commentMode,
      count,
      host: defaultEndpoint.host,
      port: defaultEndpoint.port,
      runComment,
      runPost,
      stocks: selectedStocks,
      titleMode,
      titles: selectedTitles,
      // 계정 ID가 입력된 경우에만 포함합니다(미입력이면 기존 수동 로그인 방식 사용).
      ...(accountId.trim() ? { accountId: accountId.trim() } : {}),
    };
  }

  // 현재 화면 값을 검증한 뒤 저장된 설정으로 보관하는 함수입니다.
  function saveConfig() {
    setError("");
    setMessage("");
    const config = buildConfig();

    if (!config) return;

    setSavedConfig(config);
    setMessage(
      "현재 화면의 종목, 제목, 내용, 댓글내용, 실행 설정을 저장했습니다.",
    );
  }

  // 저장된 설정 또는 현재 화면 설정을 Rust batch 실행 command로 전달하는 함수입니다.
  async function executeConfig() {
    setError("");
    setMessage("");
    setWaitSecondsLeft(null);
    const config = savedConfig ?? buildConfig();

    if (!config) return;

    setRunning(true);

    try {
      const report = await invoke<{ completed: number; reports: unknown[] }>(
        "run_naver_discussion_batch",
        {
          request: config,
        },
      );
      setSavedConfig(config);
      setMessage(
        `실행 완료: ${report.completed}회 설정을 처리했고, 등록 결과 ${report.reports.length}건을 받았습니다.`,
      );
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      setRunning(false);
      setWaitSecondsLeft(null);
    }
  }

  return (
    <section className="macro-editor-batch">
      <Stack gap="md">
        <Group justify="space-between" align="center">
          <Text fw={800}>패킷 기반 종목/글/댓글 실행 설정</Text>
          <Badge color={fileLoaded ? "green" : "gray"}>
            {fileLoaded ? fileName : "CSV 필요"}
          </Badge>
        </Group>

        <input
          ref={fileInputRef}
          type="file"
          accept=".csv,text/csv"
          className="macro-editor-hidden-file"
          onChange={(event) => {
            const file = event.currentTarget.files?.[0];
            if (file) void importTemplate(file);
            event.currentTarget.value = "";
          }}
        />

        <div className="macro-editor-batch-top">
          <Button onClick={() => fileInputRef.current?.click()}>
            가져오기
          </Button>
          <Button
            variant="light"
            loading={chromeOpening}
            onClick={openIncognitoChrome}
          >
            시크릿 Chrome 열기
          </Button>

          <div className="macro-editor-stock-search">
            <TextInput
              label="종목"
              placeholder="종목명 또는 6자리 종목코드"
              value={stockQuery}
              onChange={(event) => {
                setStockQuery(event.currentTarget.value);
                setShowStockList(true);
              }}
            />
            <Button
              className="macro-editor-stock-toggle"
              variant="light"
              aria-label="종목 목록 열기"
              onClick={() => {
                setShowStockList((current) => !current);
                void loadStocks("");
              }}
            >
              ↓
            </Button>
          </div>
        </div>

        {showStockList ? (
          <div className="macro-editor-stock-options">
            {stockOptions.map((stock) => (
              <label key={stock.code} className="macro-editor-stock-option">
                <input
                  type="radio"
                  name="stock-option"
                  onChange={() => addStock(stock)}
                />
                <span>
                  <strong>{stock.name}</strong>
                  <small>{stock.code}</small>
                </span>
              </label>
            ))}
          </div>
        ) : null}

        <div className="macro-editor-selected-stocks">
          {selectedStocks.length === 0 ? (
            <Text c="dimmed">선택한 종목이 없습니다.</Text>
          ) : (
            selectedStocks.map((stock) => (
              <button
                key={stock.code}
                className="macro-editor-stock-chip"
                type="button"
                onClick={() => removeStock(stock.code)}
                title="클릭하면 선택에서 제거됩니다."
              >
                {stock.name} · {stock.code}
              </button>
            ))
          )}
        </div>

        <div className="macro-editor-stock-detail">
          <Text fw={700}>종목명 종목코드 링크</Text>
          {selectedStocks.map((stock) => (
            <Text key={stock.code}>
              {stock.name} / {stock.code} / {stock.link}
            </Text>
          ))}
        </div>

        <div className="macro-editor-account-row">
          <TextInput
            label="로그인 계정 ID (선택)"
            placeholder="비우면 Chrome에 직접 로그인한 세션을 사용합니다"
            value={accountId}
            onChange={(event) => setAccountId(event.currentTarget.value)}
          />
          <Text size="xs" c="dimmed" className="macro-editor-field-guide">
            로그인 자동화로 저장한 계정 ID를 입력하면, 그 계정의 쿠키를 Chrome에
            주입해 글·댓글을 작성합니다. 비워두면 시크릿 Chrome에서 직접
            로그인한 세션을 사용합니다.
          </Text>
        </div>

        <Group className={actionInvalid ? "macro-editor-action-invalid" : ""}>
          <Checkbox
            checked={runPost}
            label="글쓰기"
            onChange={(event) => setRunPost(event.currentTarget.checked)}
          />
          <Checkbox
            checked={runComment}
            label="댓글쓰기"
            onChange={(event) => setRunComment(event.currentTarget.checked)}
          />
        </Group>

        {!showPostPanel && !showCommentPanel ? (
          <Text c="dimmed" size="sm">
            글쓰기 또는 댓글쓰기를 선택하면 필요한 설정 목록이 표시됩니다.
          </Text>
        ) : null}

        {showPostPanel ? (
          <section className="macro-editor-action-panel">
            <Text fw={800}>글쓰기 설정</Text>
            <div className="macro-editor-template-grid macro-editor-template-grid-post">
              <Stack gap="xs">
                <EntryListEditor
                  entries={titleEntries}
                  emptyLabel="CSV 2행 1열부터 제목을 가져옵니다."
                  label="제목"
                  selectedIndexes={selectedTitleIndexes}
                  onEntriesChange={setTitleEntries}
                  onSelectionChange={setSelectedTitleIndexes}
                />
                <ModeSelector
                  entries={selectedTitles}
                  label="제목"
                  mode={titleMode}
                  onChange={setTitleMode}
                />
              </Stack>

              <Stack gap="xs">
                <EntryListEditor
                  entries={bodyEntries}
                  emptyLabel="CSV 2행 2열부터 내용을 가져옵니다."
                  label="내용"
                  selectedIndexes={selectedBodyIndexes}
                  onEntriesChange={setBodyEntries}
                  onSelectionChange={setSelectedBodyIndexes}
                />
                <ModeSelector
                  entries={selectedBodies}
                  label="내용"
                  mode={bodyMode}
                  onChange={setBodyMode}
                />
              </Stack>
            </div>
          </section>
        ) : null}

        {showCommentPanel ? (
          <section className="macro-editor-action-panel">
            <Text fw={800}>댓글쓰기 설정</Text>
            {runPost ? (
              <Text c="dimmed" size="sm">
                글쓰기와 댓글쓰기를 같이 선택하면 Rust가 먼저 글을 등록한 뒤,
                방금 등록한 글 URL에 댓글 패킷을 전송합니다.
              </Text>
            ) : (
              <Text c="dimmed" size="sm">
                댓글쓰기만 선택하면 선택한 종목의 토론글 중 하나를 패킷으로 고른
                뒤 댓글을 작성합니다.
              </Text>
            )}
            <Stack gap="xs">
              <EntryListEditor
                entries={commentEntries}
                emptyLabel="CSV 2행 3열부터 댓글내용을 가져옵니다."
                label="댓글 내용"
                selectedIndexes={selectedCommentIndexes}
                onEntriesChange={setCommentEntries}
                onSelectionChange={setSelectedCommentIndexes}
              />
              <ModeSelector
                entries={selectedComments}
                label="댓글"
                mode={commentMode}
                onChange={setCommentMode}
              />
            </Stack>
          </section>
        ) : null}

        {showCountSelector ? (
          <Group>
            <Text fw={700}>작성 개수</Text>
            <Checkbox
              checked={count === 3}
              label="3개"
              onChange={() => setCount(3)}
            />
            <Checkbox
              checked={count === 5}
              label="5개"
              onChange={() => setCount(5)}
            />
          </Group>
        ) : null}

        <div className="macro-editor-run-row">
          <Button variant="light" onClick={saveConfig}>
            설정 저장
          </Button>
          <Button loading={running} onClick={executeConfig}>
            실행
          </Button>
        </div>

        {waitSecondsLeft !== null ? (
          <div className="macro-editor-timer" data-testid="batch-timer">
            <Text size="sm" c="dimmed" ta="center">
              다음 실행 대기 중
            </Text>
            <Text fw={800} ta="center" className="macro-editor-timer-value">
              {String(Math.floor(waitSecondsLeft / 60)).padStart(2, "0")}:
              {String(waitSecondsLeft % 60).padStart(2, "0")}
            </Text>
          </div>
        ) : null}

        {error ? (
          <Alert color="red" title="확인 필요">
            <Text component="pre" className="macro-editor-status-text">
              {error}
            </Text>
          </Alert>
        ) : null}

        {message ? (
          <Alert color="green" title="상태">
            <Text component="pre" className="macro-editor-status-text">
              {message}
            </Text>
          </Alert>
        ) : null}
      </Stack>
    </section>
  );
}
