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
};

const entrySeparator = "\n---\n";

// 실행 환경에 따라 Chrome DevTools 기본 접속값을 고르는 함수입니다.
function defaultDevtoolsEndpoint() {
  const platform = window.navigator.platform.toLowerCase();

  if (platform.includes("win")) {
    return { host: "127.0.0.1", port: 9222 };
  }

  return { host: "172.24.32.1", port: 9223 };
}

// CSV에서 가져온 여러 항목을 텍스트창 표시용 문자열로 합치는 함수입니다.
function joinEntries(entries: string[]) {
  return entries.join(entrySeparator);
}

// 텍스트창 값을 실행 가능한 항목 목록으로 다시 나누는 함수입니다.
function splitEntries(value: string) {
  return value
    .split(/\n---\n/g)
    .map((entry) => entry.trim())
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
    return `${label}이 비어 있습니다. CSV를 가져오거나 텍스트창에 입력하세요.`;
  }

  if (mode === "single" && entries.length !== 1) {
    return `${label}의 1개만은 값이 정확히 1개일 때만 선택할 수 있습니다.`;
  }

  return "";
}

// CSV로 가져온 문구를 텍스트창에서 수정할 때 지켜야 할 안내를 그리는 컴포넌트입니다.
function EditGuide() {
  return (
    <Text size="xs" c="dimmed" className="macro-editor-field-guide">
      CSV로 가져온 여러 항목은 --- 줄로 구분됩니다. --- 줄은 지우거나 바꾸지
      마세요. 수정할 문장만 드래그해서 고친 뒤 설정 저장을 누르세요. 항목 사이에
      엔터를 추가하지 마세요.
    </Text>
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
  const [titleText, setTitleText] = useState("");
  const [bodyText, setBodyText] = useState("");
  const [commentText, setCommentText] = useState("");
  const [titleMode, setTitleMode] = useState<PickMode>("random");
  const [bodyMode, setBodyMode] = useState<PickMode>("random");
  const [commentMode, setCommentMode] = useState<PickMode>("random");
  const [runPost, setRunPost] = useState(false);
  const [runComment, setRunComment] = useState(false);
  const [count, setCount] = useState<3 | 5>(3);
  const [savedConfig, setSavedConfig] = useState<SavedBatchConfig | null>(null);
  const [chromeOpening, setChromeOpening] = useState(false);
  const [running, setRunning] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  const titles = useMemo(() => splitEntries(titleText), [titleText]);
  const bodies = useMemo(() => splitEntries(bodyText), [bodyText]);
  const comments = useMemo(() => splitEntries(commentText), [commentText]);
  const actionInvalid = Boolean(error) && !runPost && !runComment;

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void loadStocks(stockQuery);
    }, 250);

    return () => window.clearTimeout(timer);
  }, [stockQuery]);

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

  // 사용자가 선택한 CSV 파일을 읽고 Rust CSV parser 결과를 텍스트창에 반영하는 함수입니다.
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

    setTitleText(joinEntries(parsed.titles));
    setBodyText(joinEntries(parsed.bodies));
    setCommentText(joinEntries(parsed.comments));
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
      runPost ? validateMode(titles, titleMode, "제목") : "",
      runPost ? validateMode(bodies, bodyMode, "내용") : "",
      runComment ? validateMode(comments, commentMode, "댓글내용") : "",
    ].filter(Boolean);

    if (validations.length > 0) {
      setError(validations[0]!);
      return null;
    }

    return {
      bodies,
      bodyMode,
      comments,
      commentMode,
      count,
      host: defaultEndpoint.host,
      port: defaultEndpoint.port,
      runComment,
      runPost,
      stocks: selectedStocks,
      titleMode,
      titles,
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

        <div className="macro-editor-template-grid">
          <Stack gap="xs">
            <Textarea
              minRows={5}
              label="제목"
              value={titleText}
              onChange={(event) => setTitleText(event.currentTarget.value)}
              placeholder="CSV 2행 1열부터 가져옵니다. 여러 항목은 --- 줄로 구분됩니다."
            />
            <EditGuide />
            <ModeSelector
              entries={titles}
              label="제목"
              mode={titleMode}
              onChange={setTitleMode}
            />
          </Stack>

          <Stack gap="xs">
            <Textarea
              minRows={5}
              label="내용"
              value={bodyText}
              onChange={(event) => setBodyText(event.currentTarget.value)}
              placeholder="CSV 2행 2열부터 가져옵니다."
            />
            <EditGuide />
            <ModeSelector
              entries={bodies}
              label="내용"
              mode={bodyMode}
              onChange={setBodyMode}
            />
          </Stack>

          <Stack gap="xs">
            <Textarea
              minRows={5}
              label="댓글 내용"
              value={commentText}
              onChange={(event) => setCommentText(event.currentTarget.value)}
              placeholder="CSV 2행 3열부터 가져옵니다."
            />
            <EditGuide />
            <ModeSelector
              entries={comments}
              label="댓글"
              mode={commentMode}
              onChange={setCommentMode}
            />
          </Stack>
        </div>

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

        <div className="macro-editor-run-row">
          <Button variant="light" onClick={saveConfig}>
            설정 저장
          </Button>
          <Button loading={running} onClick={executeConfig}>
            실행
          </Button>
        </div>

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
