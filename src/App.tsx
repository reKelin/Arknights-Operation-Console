import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import appIcon from "../assets/app-icon-small.png";
import {
  type AppSettings,
  type CommandError,
  commands,
  type DraftDirection,
  type DraftEvent,
  type DraftKind,
  events,
  type GameWindowCandidate,
  type ObservedBattleState,
  type RunnerSnapshot,
  type StageCatalogEntry,
  type UpdateEventInput,
} from "./generated/bindings";
import Timeline from "./Timeline";
import { clampViewFrames } from "./timelineMath";

type TypedResult<T> =
  | { status: "ok"; data: T }
  | { status: "error"; error: CommandError };
type Mode = "live" | "video" | "proxy";
type Page = "work" | "settings" | "editor";

const KIND_LABELS: Record<DraftKind, string> = {
  bookmark: "待分类",
  deploy: "部署",
  skill: "技能",
  retreat: "撤退",
};
const STATE_LABELS: Record<ObservedBattleState, string> = {
  unknown: "状态未知",
  notInBattle: "关卡外",
  battleBegin: "正在进入关卡",
  oneXRunning: "1× 运行",
  twoXRunning: "2× 运行",
  pointTwoXRunning: "0.2× 运行",
  paused: "暂停",
  deployingOperator: "部署中",
  adjustingOperatorFacing: "调整方向",
};

function unwrap<T>(result: TypedResult<T>): T {
  if (result.status === "error") throw result.error;
  return result.data;
}

function messageOf(error: unknown): string {
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    return error.message;
  }
  return String(error);
}

function frameTime(frame: number, denominator = 30): string {
  const seconds = Math.floor(frame / denominator);
  return `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}:${String(frame % denominator).padStart(2, "0")}`;
}

function rangeLabel(frames: number): string {
  if (frames < 30) return `${frames} 帧`;
  const seconds = Math.round(frames / 30);
  return seconds < 60 ? `${seconds} 秒` : `${Math.round(seconds / 60)} 分钟`;
}

export default function App() {
  const [snapshot, setSnapshot] = useState<RunnerSnapshot | null>(null);
  const [mode, setMode] = useState<Mode>("live");
  const [page, setPage] = useState<Page>("work");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [viewFrames, setViewFrames] = useState(900);
  const [tracePreviewFrame, setTracePreviewFrame] = useState<number | null>(
    null,
  );
  const [recordingSegmentIndex, setRecordingSegmentIndex] = useState(0);
  const [windowPickerOpen, setWindowPickerOpen] = useState(false);
  const [gameWindows, setGameWindows] = useState<GameWindowCandidate[]>([]);
  const [stagePickerOpen, setStagePickerOpen] = useState(false);
  const [stageCandidates, setStageCandidates] = useState<StageCatalogEntry[]>(
    [],
  );
  const [stageSearching, setStageSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const lastNoticeSequence = useRef(0);

  const selected = useMemo(
    () =>
      snapshot?.axis.events.find((event) => event.id === selectedId) ?? null,
    [selectedId, snapshot],
  );

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    commands
      .getSnapshot()
      .then((result) => {
        if (!disposed) setSnapshot(unwrap(result));
      })
      .catch((reason) => setError(messageOf(reason)));
    events.runnerSnapshot
      .listen((event) => {
        if (!disposed) setSnapshot(event.payload);
      })
      .then((stop) => {
        unlisten = stop;
      })
      .catch((reason) => setError(messageOf(reason)));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    events.openBookmarkList
      .listen(() => setPage("editor"))
      .then((stop) => {
        unlisten = stop;
      })
      .catch((reason) => setError(messageOf(reason)));
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    if (snapshot)
      document.documentElement.dataset.theme = snapshot.settings.theme;
  }, [snapshot]);

  useEffect(() => {
    const notices =
      snapshot?.notices.filter(
        (notice) => notice.sequence > lastNoticeSequence.current,
      ) ?? [];
    const audible = notices.filter((notice) => notice.kind === "notify");
    if (audible.length) {
      const context = new AudioContext();
      audible.forEach((_, index) => {
        const startsAt = context.currentTime + index * 0.12;
        const oscillator = context.createOscillator();
        const gain = context.createGain();
        oscillator.frequency.value = 880;
        gain.gain.setValueAtTime(0.12, startsAt);
        gain.gain.exponentialRampToValueAtTime(0.001, startsAt + 0.1);
        oscillator.connect(gain);
        gain.connect(context.destination);
        oscillator.start(startsAt);
        oscillator.stop(startsAt + 0.1);
      });
      window.setTimeout(() => context.close(), audible.length * 120 + 100);
    }
    if (snapshot?.notices.length) {
      lastNoticeSequence.current = Math.max(
        lastNoticeSequence.current,
        ...snapshot.notices.map((notice) => notice.sequence),
      );
    }
  }, [snapshot?.notices]);

  async function run(
    operation: () => Promise<TypedResult<RunnerSnapshot>>,
  ): Promise<boolean> {
    try {
      setSnapshot(unwrap(await operation()));
      setError(null);
      return true;
    } catch (reason) {
      setError(messageOf(reason));
      return false;
    }
  }

  async function runVoid(operation: () => Promise<TypedResult<null>>) {
    try {
      unwrap(await operation());
      setError(null);
    } catch (reason) {
      setError(messageOf(reason));
    }
  }

  async function importAxis() {
    const path = await open({
      multiple: false,
      filters: [{ name: "AxisLink JSON", extensions: ["json"] }],
    });
    if (
      typeof path === "string" &&
      (await run(() => commands.importAxis(path)))
    ) {
      setSelectedId(null);
    }
  }

  async function exportAxis() {
    if (!snapshot) return;
    const path = await save({
      defaultPath: `${snapshot.axis.title || "未命名轴"}.axis.json`,
      filters: [{ name: "AxisLink JSON", extensions: ["json"] }],
    });
    if (path) await run(() => commands.exportAxis(path));
  }

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const editing = ["INPUT", "SELECT", "TEXTAREA"].includes(
        (event.target as HTMLElement).tagName,
      );
      if (event.ctrlKey && event.key.toLowerCase() === "s") {
        event.preventDefault();
        exportAxis();
      } else if (!editing && event.key.toLowerCase() === "h") {
        event.preventDefault();
        setPage("editor");
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  async function scanGameWindows() {
    try {
      setGameWindows(unwrap(await commands.listGameWindows()));
      setWindowPickerOpen(true);
      setError(null);
    } catch (reason) {
      setError(messageOf(reason));
    }
  }

  async function searchStages(query: string) {
    setStageSearching(true);
    try {
      const candidates = unwrap(await commands.listStages(query));
      setStageCandidates(candidates);
      setError(null);
      return candidates;
    } catch (reason) {
      setError(messageOf(reason));
      return [];
    } finally {
      setStageSearching(false);
    }
  }

  async function chooseRecording() {
    const path = await open({
      multiple: false,
      filters: [{ name: "游戏录屏", extensions: ["mkv", "mp4"] }],
    });
    if (typeof path === "string") {
      setMode("video");
      await run(() => commands.analyzeRecording(path));
    }
  }

  if (!snapshot) {
    return (
      <main className="loading-shell">
        <span>正在启动 Arknights Operation Console…</span>
        {error && <span className="error-text">{error}</span>}
      </main>
    );
  }

  const recordingSegment =
    snapshot.monitor.recordingSegments[recordingSegmentIndex] ?? null;
  const visibleTracePoints = recordingSegment
    ? snapshot.monitor.tracePoints.filter(
        (point) =>
          point.sourceFrame >= recordingSegment.sourceStartFrame &&
          point.sourceFrame <= recordingSegment.sourceEndFrame,
      )
    : snapshot.monitor.tracePoints;
  const traceDurationFrames =
    recordingSegment?.gameDurationFrames ??
    snapshot.monitor.traceDurationFrames;
  const displayedFrame =
    mode === "video" && tracePreviewFrame !== null
      ? tracePreviewFrame
      : snapshot.frame;
  const displayedTime =
    mode === "video" && tracePreviewFrame !== null
      ? frameTime(displayedFrame)
      : snapshot.time;

  return (
    <main className={`app-shell page--${page}`}>
      <header
        className="titlebar"
        onPointerDown={(event) => {
          if (
            event.button === 0 &&
            !(event.target as HTMLElement).closest("button, input, select")
          ) {
            getCurrentWindow()
              .startDragging()
              .catch((reason) => setError(messageOf(reason)));
          }
        }}
      >
        <div className="brand">
          <img alt="" className="brand-mark" src={appIcon} />
          <strong>Arknights Operation Console</strong>
        </div>
        {page === "work" && (
          <div aria-label="工作模式" className="mode-tabs" role="tablist">
            {(
              [
                ["live", "人工录轴"],
                ["video", "录屏分析"],
                ["proxy", "代理指挥"],
              ] as const
            ).map(([value, label]) => (
              <button
                aria-selected={mode === value}
                key={value}
                onClick={() => setMode(value)}
                role="tab"
                type="button"
              >
                {label}
              </button>
            ))}
          </div>
        )}
        <span
          className="source-status"
          title={snapshot.monitor.sourceName ?? ""}
        >
          {snapshot.monitor.sourceName
            ? `${snapshot.monitor.sourceName} · ${STATE_LABELS[snapshot.monitor.battleState]}`
            : "未选择游戏窗口"}
        </span>
        <button
          aria-label={page === "settings" ? "返回工作台" : "打开设置"}
          className="icon-button"
          onClick={() => setPage(page === "settings" ? "work" : "settings")}
          type="button"
        >
          {page === "settings" ? "←" : "⚙"}
        </button>
        <button
          aria-label="最小化到托盘"
          className="window-button"
          onClick={() => runVoid(() => commands.hideToTray())}
          type="button"
        >
          —
        </button>
        <button
          aria-label="关闭"
          className="window-button window-button--close"
          onClick={() =>
            commands.closeApp().catch((reason) => setError(messageOf(reason)))
          }
          type="button"
        >
          ×
        </button>
      </header>

      {page === "settings" ? (
        <SettingsPage
          snapshot={snapshot}
          onBack={() => setPage("work")}
          onRun={run}
          onScanWindows={scanGameWindows}
          onSelectForeground={() =>
            commands
              .foregroundGameWindow()
              .then((result) => commands.selectGameWindow(unwrap(result).id))
              .then((result) => setSnapshot(unwrap(result)))
              .catch((reason) => setError(messageOf(reason)))
          }
          onSelectStage={async () => {
            setStagePickerOpen(true);
            await searchStages("");
          }}
        />
      ) : page === "editor" ? (
        <AxisEditor
          selectedId={selectedId}
          snapshot={snapshot}
          onBack={() => setPage("work")}
          onRun={run}
          onSearchStages={searchStages}
          onSelect={setSelectedId}
        />
      ) : (
        <section className="work-page">
          <ModeHero
            displayedFrame={displayedFrame}
            displayedTime={displayedTime}
            mode={mode}
            onChooseRecording={chooseRecording}
            onEdit={() => setPage("editor")}
            onToggleRecording={() =>
              run(() => commands.setRecording(!snapshot.recording))
            }
            snapshot={snapshot}
          />

          <div className="axis-heading">
            <button
              className="axis-title"
              onClick={() => setPage("editor")}
              type="button"
            >
              <strong>{snapshot.axis.title}</strong>
              <span>{snapshot.axis.events.length} 个操作</span>
            </button>
            <div className="axis-actions">
              <button onClick={importAxis} type="button">
                导入
              </button>
              <button onClick={exportAxis} type="button">
                导出 <kbd>Ctrl S</kbd>
              </button>
              <button onClick={() => setPage("editor")} type="button">
                整理 <kbd>H</kbd>
              </button>
              <button
                aria-label="缩小时间轴"
                onClick={() => setViewFrames(clampViewFrames(viewFrames * 1.5))}
                type="button"
              >
                −
              </button>
              <span>{rangeLabel(viewFrames)}</span>
              <button
                aria-label="放大时间轴"
                onClick={() => setViewFrames(clampViewFrames(viewFrames / 1.5))}
                type="button"
              >
                +
              </button>
            </div>
          </div>

          <Timeline
            currentFrame={displayedFrame}
            events={snapshot.axis.events}
            onCreate={(frame, kind) =>
              run(() => commands.addEvent({ frame, kind }))
            }
            onEdit={(event) => {
              setSelectedId(event.id);
              setPage("editor");
            }}
            onMove={(id, frame) => run(() => commands.moveEvent(id, frame))}
            onSelect={setSelectedId}
            onViewFrames={setViewFrames}
            selectedId={selectedId}
            traceDurationFrames={mode === "video" ? traceDurationFrames : null}
            tracePoints={mode === "video" ? visibleTracePoints : []}
            viewFrames={viewFrames}
          />

          <div className="timeline-footer">
            <span>
              {selected
                ? `${KIND_LABELS[selected.kind]} · ${selected.label || selected.id} · F${selected.frame}${selected.complete ? "" : " · 待补全"}`
                : snapshot.lastMessage || "右键操作点编辑；双击轨道新增操作"}
            </span>
            {mode === "video" && traceDurationFrames !== null && (
              <label className="trace-control">
                {snapshot.monitor.recordingSegments.length > 1 && (
                  <select
                    aria-label="录屏关卡区段"
                    onChange={(event) => {
                      setRecordingSegmentIndex(Number(event.target.value));
                      setTracePreviewFrame(0);
                    }}
                    value={recordingSegmentIndex}
                  >
                    {snapshot.monitor.recordingSegments.map((segment) => (
                      <option key={segment.index} value={segment.index}>
                        {segment.stageRecognition.stage?.code ??
                          `区段 ${segment.index + 1}`}
                      </option>
                    ))}
                  </select>
                )}
                <input
                  aria-label="录屏时间预览"
                  max={traceDurationFrames}
                  min="0"
                  onChange={(event) =>
                    setTracePreviewFrame(Number(event.target.value))
                  }
                  type="range"
                  value={tracePreviewFrame ?? 0}
                />
              </label>
            )}
          </div>
        </section>
      )}

      {windowPickerOpen && (
        <Picker
          title="选择明日方舟窗口"
          onClose={() => setWindowPickerOpen(false)}
        >
          {gameWindows.length ? (
            gameWindows.map((candidate) => (
              <button
                className="picker-row"
                key={candidate.id}
                onClick={async () => {
                  if (
                    await run(() => commands.selectGameWindow(candidate.id))
                  ) {
                    setWindowPickerOpen(false);
                  }
                }}
                type="button"
              >
                <strong>{candidate.title || "明日方舟"}</strong>
                <span>
                  {candidate.processName} · {candidate.width}×{candidate.height}
                </span>
                {candidate.warning && <em>{candidate.warning}</em>}
              </button>
            ))
          ) : (
            <p>未发现可捕获的 Arknights.exe 窗口。</p>
          )}
        </Picker>
      )}

      {stagePickerOpen && (
        <StagePicker
          candidates={stageCandidates}
          searching={stageSearching}
          onClose={() => setStagePickerOpen(false)}
          onSearch={searchStages}
          onSelect={async (id) => {
            if (await run(() => commands.setManualStage(id)))
              setStagePickerOpen(false);
          }}
        />
      )}

      {error && (
        <button
          className="error-toast"
          onClick={() => setError(null)}
          type="button"
        >
          {error}
        </button>
      )}
    </main>
  );
}

type ModeHeroProps = {
  mode: Mode;
  snapshot: RunnerSnapshot;
  displayedFrame: number;
  displayedTime: string;
  onToggleRecording: () => void;
  onChooseRecording: () => void;
  onEdit: () => void;
};

function ModeHero({
  mode,
  snapshot,
  displayedFrame,
  displayedTime,
  onToggleRecording,
  onChooseRecording,
  onEdit,
}: ModeHeroProps) {
  const stageWarning =
    snapshot.monitor.sourceKind !== "none" &&
    snapshot.stageSafety.status !== "matched"
      ? snapshot.stageSafety.status === "mismatched"
        ? "关卡不匹配"
        : "关卡未确认"
      : null;
  const statusLabel =
    stageWarning ?? STATE_LABELS[snapshot.monitor.battleState];
  const next = snapshot.nextEvent;

  return (
    <section className={`mode-hero mode-hero--${mode}`}>
      <div className="clock-block">
        <strong>{displayedTime}</strong>
        <span>F{displayedFrame.toString().padStart(5, "0")} · 30 Hz</span>
        <span className={snapshot.monitor.trusted ? "trust trusted" : "trust"}>
          {snapshot.monitor.sourceKind === "none"
            ? `等待监控源 · ±${snapshot.errorFrames} 帧`
            : `${statusLabel} · 可信度 ${snapshot.monitor.confidence}%`}
        </span>
      </div>

      <div className="mode-summary">
        {mode === "live" && (
          <>
            <span>下一操作</span>
            <strong>
              {next
                ? `${KIND_LABELS[next.kind]} · ${next.label || next.id}`
                : "暂无后续操作"}
            </strong>
            <small>
              {snapshot.recording
                ? "P 记录待分类操作；H 整理"
                : "进入关卡后自动计时"}
            </small>
          </>
        )}
        {mode === "video" && (
          <>
            <span>录屏时钟轨迹</span>
            <strong>
              {snapshot.monitor.connectionState === "analyzing"
                ? `正在分析 ${snapshot.monitor.recordingProgress ?? 0}%`
                : snapshot.monitor.sourceKind === "recording"
                  ? `${snapshot.monitor.recordingSegments.length || 1} 个关卡区段`
                  : "尚未选择录屏"}
            </strong>
            <small>当前版本只生成时间映射；操作候选将在 v0.1.2 接入</small>
          </>
        )}
        {mode === "proxy" && (
          <>
            <span>代理指挥</span>
            <strong>暂停执行事务尚未接入</strong>
            <small>完成键位确认、状态回执与接管续录后开放</small>
          </>
        )}
      </div>

      <div className="mode-actions">
        {mode === "live" && (
          <>
            <button
              className="button--primary"
              onClick={onToggleRecording}
              type="button"
            >
              {snapshot.recording ? "停止录轴" : "开始录轴"}
            </button>
            <button onClick={onEdit} type="button">
              整理操作
            </button>
          </>
        )}
        {mode === "video" && (
          <button
            className="button--primary"
            onClick={onChooseRecording}
            type="button"
          >
            选择录屏
          </button>
        )}
        {mode === "proxy" && (
          <button disabled title="v0.1.1 接入暂停执行事务后开放" type="button">
            代理执行尚不可用
          </button>
        )}
      </div>
    </section>
  );
}

type SettingsPageProps = {
  snapshot: RunnerSnapshot;
  onBack: () => void;
  onRun: (
    operation: () => Promise<TypedResult<RunnerSnapshot>>,
  ) => Promise<boolean>;
  onScanWindows: () => void;
  onSelectForeground: () => void;
  onSelectStage: () => void;
};

function SettingsPage({
  snapshot,
  onBack,
  onRun,
  onScanWindows,
  onSelectForeground,
  onSelectStage,
}: SettingsPageProps) {
  const [tab, setTab] = useState<
    "monitor" | "appearance" | "shortcuts" | "execution"
  >("monitor");
  const [framesPerCost, setFramesPerCost] = useState(
    String(snapshot.settings.framesPerCost),
  );
  const [gameUiScale, setGameUiScale] = useState(
    String(snapshot.settings.gameUiScale),
  );

  function updateSettings(input: Partial<AppSettings>) {
    return onRun(() =>
      commands.updateSettings({ ...snapshot.settings, ...input }),
    );
  }

  return (
    <section className="settings-page">
      <div className="page-heading">
        <div>
          <strong>设置</strong>
          <span>显示、监控与快捷键</span>
        </div>
        <button onClick={onBack} type="button">
          ← 返回
        </button>
      </div>
      <div className="settings-tabs" role="tablist">
        {(
          [
            ["monitor", "监控"],
            ["appearance", "外观"],
            ["shortcuts", "快捷键"],
            ["execution", "执行"],
          ] as const
        ).map(([value, label]) => (
          <button
            aria-selected={tab === value}
            key={value}
            onClick={() => setTab(value)}
            role="tab"
            type="button"
          >
            {label}
          </button>
        ))}
      </div>
      <div className="settings-content">
        {tab === "monitor" && (
          <>
            <SettingRow
              label="当前监控源"
              note="进入关卡后自动计时，暂停与倍速跟随游戏"
            >
              <span>{snapshot.monitor.sourceName ?? "未选择"}</span>
            </SettingRow>
            <SettingRow label="游戏窗口">
              <div className="inline-actions">
                <button onClick={onScanWindows} type="button">
                  扫描窗口
                </button>
                <button onClick={onSelectForeground} type="button">
                  使用前台窗口
                </button>
                <button
                  onClick={() => onRun(() => commands.stopMonitor())}
                  type="button"
                >
                  停止监控
                </button>
              </div>
            </SettingRow>
            <SettingRow label="关卡确认">
              <button onClick={onSelectStage} type="button">
                手动选择关卡
              </button>
            </SettingRow>
          </>
        )}
        {tab === "appearance" && (
          <>
            <SettingRow label="主题">
              <select
                onChange={(event) =>
                  updateSettings({
                    theme: event.target.value as AppSettings["theme"],
                  })
                }
                value={snapshot.settings.theme}
              >
                <option value="dark">深色</option>
                <option value="light">浅色</option>
              </select>
            </SettingRow>
            <SettingRow label="窗口置顶">
              <input
                checked={snapshot.alwaysOnTop}
                onChange={(event) =>
                  onRun(() => commands.setAlwaysOnTop(event.target.checked))
                }
                type="checkbox"
              />
            </SettingRow>
            <SettingRow label="费用帧分母" note="事件时间仍固定为 30 Hz">
              <input
                max="150"
                min="15"
                onBlur={() =>
                  updateSettings({ framesPerCost: Number(framesPerCost) })
                }
                onChange={(event) => setFramesPerCost(event.target.value)}
                type="number"
                value={framesPerCost}
              />
            </SettingRow>
            <SettingRow label="游戏 UI 比例">
              <input
                max="100"
                min="0"
                onBlur={() =>
                  updateSettings({ gameUiScale: Number(gameUiScale) })
                }
                onChange={(event) => setGameUiScale(event.target.value)}
                type="number"
                value={gameUiScale}
              />
            </SettingRow>
          </>
        )}
        {tab === "shortcuts" && (
          <>
            <SettingRow label="记录待分类操作" note="游戏窗口位于前台时">
              <kbd>P</kbd>
            </SettingRow>
            <SettingRow label="整理操作" note="Console 位于前台时">
              <kbd>H</kbd>
            </SettingRow>
            <SettingRow label="导出作战轴" note="Console 位于前台时">
              <kbd>Ctrl S</kbd>
            </SettingRow>
          </>
        )}
        {tab === "execution" && (
          <>
            <SettingRow label="操作提醒">
              <button
                onClick={() => onRun(() => commands.setStrategy("notify"))}
                type="button"
              >
                {snapshot.strategy === "notify" ? "已启用" : "启用"}
              </button>
            </SettingRow>
            <SettingRow
              label="代理执行"
              note="等待暂停事务、键位确认与结果回执接入"
            >
              <button disabled type="button">
                尚不可用
              </button>
            </SettingRow>
          </>
        )}
      </div>
    </section>
  );
}

function SettingRow({
  label,
  note,
  children,
}: {
  label: string;
  note?: string;
  children: ReactNode;
}) {
  return (
    <div className="setting-row">
      <span>
        <strong>{label}</strong>
        {note && <small>{note}</small>}
      </span>
      {children}
    </div>
  );
}

type AxisEditorProps = {
  snapshot: RunnerSnapshot;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onBack: () => void;
  onRun: (
    operation: () => Promise<TypedResult<RunnerSnapshot>>,
  ) => Promise<boolean>;
  onSearchStages: (query: string) => Promise<StageCatalogEntry[]>;
};

function AxisEditor({
  snapshot,
  selectedId,
  onSelect,
  onBack,
  onRun,
  onSearchStages,
}: AxisEditorProps) {
  const selected =
    snapshot.axis.events.find((event) => event.id === selectedId) ??
    snapshot.axis.events[0] ??
    null;
  const [filter, setFilter] = useState<"all" | DraftKind>("all");
  const [query, setQuery] = useState("");
  const visible = snapshot.axis.events.filter(
    (event) =>
      (filter === "all" || event.kind === filter) &&
      (!query ||
        `${event.id} ${event.label ?? ""}`
          .toLowerCase()
          .includes(query.toLowerCase())),
  );

  return (
    <section className="editor-page">
      <div className="page-heading">
        <div>
          <strong>整理作战轴</strong>
          <span>
            {snapshot.axis.events.length} 个操作 · 草稿仅保存在本次会话
          </span>
        </div>
        <button onClick={onBack} type="button">
          ← 返回
        </button>
      </div>
      <AxisMetadata
        snapshot={snapshot}
        onRun={onRun}
        onSearchStages={onSearchStages}
      />
      <div className="editor-toolbar">
        <input
          aria-label="搜索操作"
          onChange={(event) => setQuery(event.target.value)}
          placeholder="搜索名称或 ID"
          value={query}
        />
        <select
          aria-label="筛选操作类型"
          onChange={(event) =>
            setFilter(event.target.value as "all" | DraftKind)
          }
          value={filter}
        >
          <option value="all">全部类型</option>
          {(["bookmark", "deploy", "skill", "retreat"] as DraftKind[]).map(
            (kind) => (
              <option key={kind} value={kind}>
                {KIND_LABELS[kind]}
              </option>
            ),
          )}
        </select>
        <button
          onClick={() =>
            onRun(() =>
              commands.addEvent({ frame: snapshot.frame, kind: "bookmark" }),
            )
          }
          type="button"
        >
          新增待分类操作
        </button>
      </div>
      <div className="editor-layout">
        <div className="editor-list" role="listbox">
          {visible.map((event) => (
            <button
              aria-selected={event.id === selected?.id}
              key={event.id}
              onClick={() => onSelect(event.id)}
              role="option"
              type="button"
            >
              <span>{frameTime(event.frame)}</span>
              <strong>{event.label || KIND_LABELS[event.kind]}</strong>
              <em>{event.complete ? KIND_LABELS[event.kind] : "待补全"}</em>
            </button>
          ))}
          {!visible.length && <p>没有匹配的操作。</p>}
        </div>
        {selected ? (
          <EventForm
            event={selected}
            key={selected.id}
            onDelete={() => onRun(() => commands.deleteEvent(selected.id))}
            onSave={(input) => onRun(() => commands.updateEvent(input))}
          />
        ) : (
          <div className="editor-empty">选择一个操作以编辑参数和备注。</div>
        )}
      </div>
    </section>
  );
}

function AxisMetadata({
  snapshot,
  onRun,
  onSearchStages,
}: {
  snapshot: RunnerSnapshot;
  onRun: (
    operation: () => Promise<TypedResult<RunnerSnapshot>>,
  ) => Promise<boolean>;
  onSearchStages: (query: string) => Promise<StageCatalogEntry[]>;
}) {
  const [title, setTitle] = useState(snapshot.axis.title);
  const [stageId, setStageId] = useState(snapshot.axis.stageId ?? "");
  const [stages, setStages] = useState<StageCatalogEntry[]>([]);
  return (
    <form
      className="axis-metadata"
      onSubmit={(event) => {
        event.preventDefault();
        onRun(() =>
          commands.setAxisMetadata({ title, stageId: stageId || null }),
        );
      }}
    >
      <label>
        轴名称
        <input
          maxLength={128}
          onChange={(event) => setTitle(event.target.value)}
          required
          value={title}
        />
      </label>
      <label>
        关卡
        <input
          onChange={(event) => setStageId(event.target.value)}
          placeholder="关卡 ID"
          value={stageId}
        />
      </label>
      <button
        onClick={async () => setStages(await onSearchStages(stageId))}
        type="button"
      >
        查找关卡
      </button>
      {stages.length > 0 && (
        <select
          aria-label="关卡搜索结果"
          onChange={(event) => setStageId(event.target.value)}
          value={stages.some((stage) => stage.id === stageId) ? stageId : ""}
        >
          <option disabled value="">
            选择关卡
          </option>
          {stages.map((stage) => (
            <option key={stage.id} value={stage.id}>
              {stage.code} · {stage.name}
            </option>
          ))}
        </select>
      )}
      <button type="submit">保存轴信息</button>
    </form>
  );
}

function EventForm({
  event,
  onSave,
  onDelete,
}: {
  event: DraftEvent;
  onSave: (input: UpdateEventInput) => void;
  onDelete: () => void;
}) {
  const [frame, setFrame] = useState(String(event.frame));
  const [kind, setKind] = useState<DraftKind>(event.kind);
  const [operator, setOperator] = useState(event.operator ?? "");
  const [tile, setTile] = useState(event.tile ?? "");
  const [direction, setDirection] = useState<DraftDirection>(
    event.direction ?? "right",
  );
  const [label, setLabel] = useState(event.label ?? "");
  return (
    <form
      className="event-form"
      onSubmit={(submitEvent) => {
        submitEvent.preventDefault();
        onSave({
          id: event.id,
          frame: Number(frame),
          kind,
          operator: kind === "deploy" ? operator || null : null,
          tile: kind === "bookmark" ? null : tile.trim().toUpperCase() || null,
          direction: kind === "deploy" ? direction : null,
          label: label || null,
        });
      }}
    >
      <h2>操作参数</h2>
      <div className="form-grid">
        <label>
          类型
          <select
            onChange={(event) => setKind(event.target.value as DraftKind)}
            value={kind}
          >
            {(["bookmark", "deploy", "skill", "retreat"] as DraftKind[]).map(
              (value) => (
                <option key={value} value={value}>
                  {KIND_LABELS[value]}
                </option>
              ),
            )}
          </select>
        </label>
        <label>
          帧
          <input
            min="0"
            onChange={(event) => setFrame(event.target.value)}
            required
            type="number"
            value={frame}
          />
        </label>
        {kind === "deploy" && (
          <label>
            干员 ID
            <input
              onChange={(event) => setOperator(event.target.value)}
              placeholder="char_002_amiya"
              required
              value={operator}
            />
          </label>
        )}
        {kind !== "bookmark" && (
          <label>
            格子
            <input
              maxLength={3}
              onChange={(event) => setTile(event.target.value.toUpperCase())}
              pattern="[A-I](?:[1-9]|[12][0-9]|3[0-6])"
              placeholder="C5"
              required
              value={tile}
            />
          </label>
        )}
        {kind === "deploy" && (
          <label>
            朝向
            <select
              onChange={(event) =>
                setDirection(event.target.value as DraftDirection)
              }
              value={direction}
            >
              <option value="up">上</option>
              <option value="right">右</option>
              <option value="down">下</option>
              <option value="left">左</option>
            </select>
          </label>
        )}
        <label className="note-field">
          备注（可选）
          <textarea
            maxLength={120}
            onChange={(event) => setLabel(event.target.value)}
            placeholder="补充说明，不影响执行"
            value={label}
          />
        </label>
      </div>
      <footer>
        <button className="danger-button" onClick={onDelete} type="button">
          删除
        </button>
        <span />
        <button className="button--primary" type="submit">
          保存更改
        </button>
      </footer>
    </form>
  );
}

function Picker({
  title,
  onClose,
  children,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
}) {
  return (
    <div className="modal-backdrop">
      <section className="compact-dialog">
        <header>
          <h2>{title}</h2>
          <button aria-label="关闭" onClick={onClose} type="button">
            ×
          </button>
        </header>
        {children}
      </section>
    </div>
  );
}

function StagePicker({
  candidates,
  searching,
  onSearch,
  onSelect,
  onClose,
}: {
  candidates: StageCatalogEntry[];
  searching: boolean;
  onSearch: (query: string) => void;
  onSelect: (id: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  return (
    <Picker title="手动确认当前关卡" onClose={onClose}>
      <form
        className="picker-search"
        onSubmit={(event) => {
          event.preventDefault();
          onSearch(query);
        }}
      >
        <input
          onChange={(event) => setQuery(event.target.value)}
          placeholder="关卡代码、名称或 ID"
          value={query}
        />
        <button type="submit">{searching ? "搜索中…" : "搜索"}</button>
      </form>
      <div className="picker-list">
        {candidates.map((stage) => (
          <button
            className="picker-row"
            key={stage.id}
            onClick={() => onSelect(stage.id)}
            type="button"
          >
            <strong>
              {stage.code} · {stage.name}
            </strong>
            <span>{stage.id}</span>
          </button>
        ))}
        {!candidates.length && <p>没有匹配的关卡。</p>}
      </div>
    </Picker>
  );
}
