import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";
import appIcon from "../assets/app-icon-small.png";
import { version } from "../package.json";
import AxisEditor from "./AxisEditor";
import {
  CONSOLE_TO_MODE,
  frameTime,
  MODE_TO_CONSOLE,
  type Mode,
  messageOf,
  REVISION_SOURCE_LABELS,
  type TypedResult,
  unwrap,
} from "./console";
import Picker from "./Dialog";
import {
  commands,
  events,
  type GameWindowCandidate,
  type RecordingMergeInput,
  type RecordingMergePreview,
  type RunnerSnapshot,
  type StageCatalogEntry,
} from "./generated/bindings";
import Icon from "./Icon";
import ModeHero from "./ModeHero";
import RecordingContinuation from "./RecordingContinuation";
import SettingsPage from "./SettingsPage";
import Timeline from "./Timeline";
import {
  clampViewFrames,
  MAX_VIEW_FRAMES,
  MIN_VIEW_FRAMES,
} from "./timelineMath";

type Page = "work" | "settings" | "editor" | "analysis";

export default function App() {
  const [snapshot, setSnapshot] = useState<RunnerSnapshot | null>(null);
  const [page, setPage] = useState<Page>("work");
  const workHeight = useRef(280);
  const previousPage = useRef<Page>("work");
  const [pendingMode, setPendingMode] = useState<Mode | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [viewFrames, setViewFrames] = useState(5400);
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
  const commandQueue = useRef<Promise<boolean>>(Promise.resolve(true));
  const [proxyConfirmationOpen, setProxyConfirmationOpen] = useState(false);
  const [validationOpen, setValidationOpen] = useState(false);

  useEffect(() => {
    if (page === previousPage.current) return;
    if (previousPage.current === "work")
      workHeight.current = window.innerHeight;
    previousPage.current = page;
    const height =
      page === "work"
        ? workHeight.current
        : Math.max(window.innerHeight, page === "editor" ? 460 : 420);
    getCurrentWindow()
      .setSize(new LogicalSize(window.innerWidth, height))
      .catch((reason) => setError(messageOf(reason)));
  }, [page]);

  useEffect(() => {
    if (
      !pendingMode ||
      !snapshot ||
      ["cancelling", "awaitingPauseProof"].includes(
        snapshot.session.takeover.status,
      )
    )
      return;
    if (snapshot.session.takeover.status === "recording") {
      commands
        .setConsoleMode(MODE_TO_CONSOLE[pendingMode])
        .then((result) => setSnapshot(unwrap(result)))
        .catch((reason) => setError(messageOf(reason)));
    }
    setPendingMode(null);
  }, [pendingMode, snapshot]);

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

  function run(
    operation: () => Promise<TypedResult<RunnerSnapshot>>,
  ): Promise<boolean> {
    const result = commandQueue.current.then(async () => {
      try {
        setSnapshot(unwrap(await operation()));
        setError(null);
        return true;
      } catch (reason) {
        setError(messageOf(reason));
        return false;
      }
    });
    commandQueue.current = result;
    return result;
  }

  async function runVoid(operation: () => Promise<TypedResult<null>>) {
    try {
      unwrap(await operation());
      setError(null);
    } catch (reason) {
      setError(messageOf(reason));
    }
  }

  async function previewRecordingMerge(
    input: RecordingMergeInput,
  ): Promise<RecordingMergePreview | null> {
    try {
      const preview = unwrap(await commands.previewRecordingMerge(input));
      setError(null);
      return preview;
    } catch (reason) {
      setError(messageOf(reason));
      return null;
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
    (document.activeElement as HTMLElement | null)?.blur();
    await commandQueue.current;
    const path = await save({
      defaultPath: `${snapshot.axis.title || "未命名轴"}.axis.json`,
      filters: [{ name: "AxisLink JSON", extensions: ["json"] }],
    });
    if (path) await run(() => commands.exportAxis(path));
  }

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (document.querySelector("dialog[open]")) return;
      const editing =
        ["INPUT", "SELECT", "TEXTAREA"].includes(
          (event.target as HTMLElement).tagName,
        ) || (event.target as HTMLElement).isContentEditable;
      if (event.ctrlKey && event.key.toLowerCase() === "s") {
        event.preventDefault();
        exportAxis();
      } else if (!editing && event.key.toLowerCase() === "h") {
        event.preventDefault();
        openPage("editor");
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
      if (await run(() => commands.setConsoleMode("recordingAnalysis"))) {
        await run(() => commands.analyzeRecording(path));
      }
    }
  }

  async function openPage(nextPage: Page) {
    await commandQueue.current;
    if (
      nextPage !== "work" &&
      snapshot?.proxy.enabled &&
      !(await run(() => commands.emergencyStop()))
    )
      return;
    setPage(nextPage);
  }

  async function switchMode(nextMode: Mode) {
    if (!snapshot || nextMode === CONSOLE_TO_MODE[snapshot.consoleMode]) return;
    if (
      snapshot.proxy.enabled &&
      snapshot.proxy.runId &&
      nextMode !== "proxy"
    ) {
      if (!(await run(() => commands.takeoverNow()))) return;
      if (nextMode === "video") setPendingMode(nextMode);
      return;
    }
    await run(() => commands.setConsoleMode(MODE_TO_CONSOLE[nextMode]));
    setSelectedId(null);
  }

  async function selectRecordingSegment(index: number) {
    if (await run(() => commands.selectRecordingSegment(index))) {
      setRecordingSegmentIndex(index);
      setSelectedId(null);
      setTracePreviewFrame(0);
    }
  }

  async function prepareProxy() {
    if (!snapshot) return;
    const source = snapshot.session.revisions.find(
      (revision) => revision.id === snapshot.session.currentRevisionId,
    )?.recordingMerge;
    if (
      snapshot.consoleMode === "recordingAnalysis" &&
      (!snapshot.monitor.recordingAnalysisId ||
        source?.recordingAnalysisId !== snapshot.monitor.recordingAnalysisId)
    ) {
      setError("当前录屏尚未生成作战轴");
      return;
    }
    if (
      !snapshot.axis.events.length ||
      snapshot.axis.events.some(
        (event) => !event.complete || event.timeConfirmation === "unconfirmed",
      ) ||
      snapshot.stagedRecordingEvents.length
    ) {
      setValidationOpen(true);
      return;
    }
    await switchMode("proxy");
  }

  if (!snapshot) {
    return (
      <main className="loading-shell">
        <span>正在启动 Arknights Operation Console…</span>
        {error && <span className="error-text">{error}</span>}
      </main>
    );
  }

  const mode = CONSOLE_TO_MODE[snapshot.consoleMode];
  const currentRevision = snapshot.session.revisions.find(
    (revision) => revision.id === snapshot.session.currentRevisionId,
  );
  const recordingSource = currentRevision?.recordingMerge;
  const videoHasAxis =
    mode !== "video" ||
    (snapshot.monitor.recordingAnalysisId !== null &&
      recordingSource?.recordingAnalysisId ===
        snapshot.monitor.recordingAnalysisId);
  const axisEvents = videoHasAxis ? snapshot.axis.events : [];
  const currentSegmentIndex =
    recordingSource?.recordingAnalysisId ===
    snapshot.monitor.recordingAnalysisId
      ? recordingSource.segmentIndex
      : recordingSegmentIndex;
  const recordingSegment =
    snapshot.monitor.recordingSegments.find(
      (segment) => segment.index === currentSegmentIndex,
    ) ?? null;
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
  const revisionEditable =
    snapshot.session.currentRevisionId ===
    snapshot.session.activeRecordingRevisionId;
  const pendingReceipts = snapshot.proxy.receipts.filter(
    (receipt) =>
      receipt.status === "uncertain" &&
      (receipt.runId === snapshot.proxy.runId ||
        snapshot.session.takeover.uncertainReceiptSequences.includes(
          receipt.receiptSequence,
        )),
  );
  const displayedFrame =
    mode === "video"
      ? !videoHasAxis
        ? 0
        : (selected?.frame ?? tracePreviewFrame ?? axisEvents[0]?.frame ?? 0)
      : snapshot.frame;
  const displayedTime =
    mode === "video"
      ? frameTime(displayedFrame, snapshot.settings.framesPerCost)
      : frameTime(snapshot.frame, snapshot.settings.framesPerCost);
  const continuationRevisions = snapshot.session.revisions
    .map((revision) => ({
      id: revision.id,
      label: `v${revision.sequence} · ${REVISION_SOURCE_LABELS[revision.source]}`,
      attemptId: revision.attemptId,
      createdFrame: revision.createdFrame,
      eligible:
        revision.attemptId !== null &&
        (revision.source === "takeover" ||
          revision.source === "recordingMerge"),
    }))
    .sort((left, right) => {
      if (left.id === snapshot.session.currentRevisionId) return -1;
      if (right.id === snapshot.session.currentRevisionId) return 1;
      return 0;
    });
  const continuationSegments = snapshot.monitor.recordingSegments.map(
    (segment) => ({
      index: segment.index,
      label: `区段 ${segment.index + 1} · F0–F${segment.gameDurationFrames}`,
      candidateCount: [
        ...snapshot.axis.events,
        ...snapshot.stagedRecordingEvents,
      ].filter(
        (event) =>
          event.complete &&
          event.timeConfirmation !== "unconfirmed" &&
          event.sourceRecordingId === snapshot.monitor.recordingAnalysisId &&
          event.sourceSegmentIndex === segment.index,
      ).length,
    }),
  );

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
          <span className="version">v{version}</span>
        </div>
        <button
          aria-label={page === "settings" ? "返回工作台" : "打开设置"}
          className="icon-button"
          onClick={() => openPage(page === "settings" ? "work" : "settings")}
          type="button"
        >
          <Icon name={page === "settings" ? "back" : "settings"} />
        </button>
        <button
          aria-label="最小化到托盘"
          className="window-button"
          onClick={() => runVoid(() => commands.hideToTray())}
          type="button"
        >
          <Icon name="minus" />
        </button>
        <button
          aria-label="关闭"
          className="window-button window-button--close"
          onClick={() =>
            commands.closeApp().catch((reason) => setError(messageOf(reason)))
          }
          type="button"
        >
          <Icon name="close" />
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
      ) : page === "analysis" ? (
        <section className="analysis-page">
          <div className="page-heading">
            <strong>校对录屏操作</strong>
            <button onClick={() => setPage("work")} type="button">
              <Icon name="back" />
              返回
            </button>
          </div>
          <label className="analysis-segment">
            关卡区段
            <select
              aria-label="校对关卡区段"
              value={currentSegmentIndex}
              onChange={(event) =>
                selectRecordingSegment(Number(event.target.value))
              }
            >
              {snapshot.monitor.recordingSegments.map((segment) => (
                <option key={segment.index} value={segment.index}>
                  {segment.stageRecognition.stage?.code ?? "未确认关卡"} · 区段{" "}
                  {segment.index + 1}
                </option>
              ))}
            </select>
          </label>
          {pendingReceipts.map((receipt) => (
            <div className="receipt-confirmation" key={receipt.receiptSequence}>
              <span>
                {receipt.eventId} 的执行结果待确认 · {receipt.reason}
              </span>
              <button
                onClick={() =>
                  run(() =>
                    commands.resolveExecutionReceipt({
                      receiptSequence: receipt.receiptSequence,
                      confirmed: true,
                    }),
                  )
                }
                type="button"
              >
                已完成
              </button>
              <button
                onClick={() =>
                  run(() =>
                    commands.resolveExecutionReceipt({
                      receiptSequence: receipt.receiptSequence,
                      confirmed: false,
                    }),
                  )
                }
                type="button"
              >
                未完成
              </button>
            </div>
          ))}

          {snapshot.monitor.sourceKind === "recording" &&
            snapshot.monitor.connectionState === "ready" &&
            snapshot.monitor.recordingAnalysisId && (
              <RecordingContinuation
                onCreate={(input) =>
                  run(() => commands.createRecordingMergeRevision(input))
                }
                onPreview={previewRecordingMerge}
                recordingAnalysisId={snapshot.monitor.recordingAnalysisId}
                revisions={continuationRevisions}
                key={`${snapshot.monitor.recordingAnalysisId}:${currentSegmentIndex}`}
                segments={continuationSegments.filter(
                  (segment) => segment.index === currentSegmentIndex,
                )}
              />
            )}
        </section>
      ) : page === "editor" ? (
        <AxisEditor
          selectedId={selectedId}
          snapshot={snapshot}
          onBack={() => setPage("work")}
          onRun={run}
          onReviewRecording={() => openPage("analysis")}
          onSearchStages={searchStages}
          onSelect={async (id) => {
            await commandQueue.current;
            setSelectedId(id);
          }}
        />
      ) : (
        <section className="work-page">
          <nav className="modebar" aria-label="工作模式与文件操作">
            <div aria-label="工作模式" className="mode-tabs" role="tablist">
              {(
                [
                  ["live", "实时录轴"],
                  ["proxy", "代理指挥"],
                  ["video", "视频分析"],
                ] as const
              ).map(([value, label]) => (
                <button
                  aria-selected={mode === value}
                  key={value}
                  onClick={() => switchMode(value)}
                  role="tab"
                  type="button"
                >
                  {label}
                </button>
              ))}
            </div>

            <div className="toolbar">
              {mode === "video" ? (
                <button onClick={chooseRecording} type="button">
                  {snapshot.monitor.sourceKind === "recording"
                    ? "更换录屏"
                    : "选择录屏"}
                </button>
              ) : (
                <button onClick={importAxis} type="button">
                  导入
                </button>
              )}
              {(mode !== "video" ||
                snapshot.monitor.connectionState === "ready") && (
                <>
                  <button onClick={exportAxis} type="button" title="Ctrl+S">
                    导出
                  </button>
                  <button
                    onClick={() => openPage("editor")}
                    type="button"
                    title="H"
                  >
                    编辑轴
                  </button>
                </>
              )}
            </div>
          </nav>
          <ModeHero
            axisEvents={axisEvents}
            displayedFrame={displayedFrame}
            displayedTime={displayedTime}
            mode={mode}
            onChooseRecording={chooseRecording}
            selected={videoHasAxis ? selected : null}
            onEdit={() => openPage("editor")}
            onUseAxis={prepareProxy}
            onRequestProxy={async () => {
              if (await run(() => commands.requestProxyExecution()))
                setProxyConfirmationOpen(true);
            }}
            onSwitchVideo={() => switchMode("video")}
            onSelectEvent={(id) => {
              setSelectedId(id);
              setTracePreviewFrame(
                snapshot.axis.events.find((event) => event.id === id)?.frame ??
                  0,
              );
            }}
            recordingSegmentIndex={currentSegmentIndex}
            onSelectSegment={(index) => {
              selectRecordingSegment(index);
            }}
            onToggleRecording={() =>
              run(() => commands.setRecording(!snapshot.recording))
            }
            onRun={run}
            snapshot={snapshot}
          />

          <div className="axis-heading">
            <div className="axis-title-group">
              <button
                className="axis-title"
                onClick={() => openPage("editor")}
                type="button"
              >
                <strong>
                  {videoHasAxis ? snapshot.axis.title : "未发现可提取的操作"}
                </strong>
              </button>
              <select
                hidden={!videoHasAxis}
                className="revision-select"
                aria-label="轴版本"
                disabled={snapshot.proxy.enabled}
                onChange={(event) =>
                  run(() => commands.selectAxisRevision(event.target.value))
                }
                value={snapshot.session.currentRevisionId}
              >
                {snapshot.session.revisions.map((revision) => (
                  <option key={revision.id} value={revision.id}>
                    v{revision.sequence} ·{" "}
                    {REVISION_SOURCE_LABELS[revision.source]}
                    {revision.id === snapshot.session.activeRecordingRevisionId
                      ? " · 续录中"
                      : ""}
                  </option>
                ))}
              </select>
              {!revisionEditable && <em>旧版本只读</em>}
            </div>
            <div className="zoom-tools">
              <button
                aria-label="自适应轨道"
                title="自适应轨道"
                onClick={() =>
                  setViewFrames(
                    clampViewFrames(
                      Math.max(
                        40,
                        displayedFrame,
                        ...snapshot.axis.events.map((event) => event.frame),
                      ) * 1.05,
                    ),
                  )
                }
                type="button"
              >
                <Icon name="fit" />
              </button>
              <button
                aria-label="轨道缩小"
                title="轨道缩小"
                onClick={() => setViewFrames(clampViewFrames(viewFrames * 1.5))}
                type="button"
              >
                <Icon name="zoomOut" />
              </button>
              <input
                aria-label="轨道缩放"
                aria-valuetext={`视野 ${viewFrames} 帧`}
                type="range"
                min="0"
                max="1000"
                value={
                  (1000 * Math.log(MAX_VIEW_FRAMES / viewFrames)) /
                  Math.log(MAX_VIEW_FRAMES / MIN_VIEW_FRAMES)
                }
                onChange={(event) =>
                  setViewFrames(
                    clampViewFrames(
                      MAX_VIEW_FRAMES *
                        (MIN_VIEW_FRAMES / MAX_VIEW_FRAMES) **
                          (Number(event.target.value) / 1000),
                    ),
                  )
                }
              />
              <button
                aria-label="轨道放大"
                title="轨道放大"
                onClick={() => setViewFrames(clampViewFrames(viewFrames / 1.5))}
                type="button"
              >
                <Icon name="zoomIn" />
              </button>
            </div>
          </div>

          {mode === "video" &&
          (snapshot.monitor.sourceKind !== "recording" ||
            snapshot.monitor.connectionState !== "ready") ? (
            <div className="analysis-progress">
              <progress
                aria-label="录屏分析进度"
                max="100"
                value={snapshot.monitor.recordingProgress ?? 0}
              />
            </div>
          ) : (
            <Timeline
              currentFrame={displayedFrame}
              editable={revisionEditable}
              events={axisEvents}
              onCreate={(frame, kind) =>
                run(() => commands.addEvent({ frame, kind }))
              }
              onEdit={(event) => {
                setSelectedId(event.id);
                openPage("editor");
              }}
              onMove={async (id, frame) => {
                if (await run(() => commands.moveEvent(id, frame))) {
                  setSelectedId(id);
                  openPage("editor");
                }
              }}
              onSelect={(id) => {
                setSelectedId(id);
                if (mode === "video")
                  setTracePreviewFrame(
                    snapshot.axis.events.find((event) => event.id === id)
                      ?.frame ?? 0,
                  );
              }}
              onViewFrames={setViewFrames}
              selectedId={selectedId}
              traceDurationFrames={
                mode === "video" ? traceDurationFrames : null
              }
              tracePoints={mode === "video" ? visibleTracePoints : []}
              viewFrames={viewFrames}
            />
          )}
        </section>
      )}

      {proxyConfirmationOpen && (
        <Picker
          title="启用代理执行"
          onClose={() => setProxyConfirmationOpen(false)}
        >
          <p>{snapshot.proxy.message}</p>
          {error && (
            <p className="error-text" role="alert">
              {error}
            </p>
          )}
          <p>确认后等待下一局可信起点；K 接管或停止代理可立即终止输入。</p>
          <div className="dialog-actions">
            <button
              onClick={() => setProxyConfirmationOpen(false)}
              type="button"
            >
              取消
            </button>
            <button
              className="button--primary"
              onClick={async () => {
                let enabled = false;
                await run(async () => {
                  const result = await commands.requestProxyExecution();
                  enabled = result.status === "ok" && result.data.proxy.enabled;
                  return result;
                });
                if (enabled) setProxyConfirmationOpen(false);
              }}
              type="button"
            >
              确认启用
            </button>
          </div>
        </Picker>
      )}
      {validationOpen && (
        <Picker title="先整理操作" onClose={() => setValidationOpen(false)}>
          <p>
            轴已自动填入识别结果。请补全待校对参数并确认不确定的时间后再用于代理。
          </p>
          <div className="dialog-actions">
            <button onClick={() => setValidationOpen(false)} type="button">
              返回
            </button>
            <button
              className="button--primary"
              onClick={() => {
                setValidationOpen(false);
                setPage(
                  snapshot.stagedRecordingEvents.length ? "analysis" : "editor",
                );
              }}
              type="button"
            >
              整理操作
            </button>
          </div>
        </Picker>
      )}
      {page === "work" && pendingReceipts.length > 0 && (
        <Picker
          title="确认接管前的执行结果"
          onClose={() => openPage("analysis")}
        >
          {pendingReceipts.map((receipt) => (
            <div className="receipt-confirmation" key={receipt.receiptSequence}>
              <span>
                {receipt.eventId} · {receipt.reason}
              </span>
              <button
                type="button"
                onClick={() =>
                  run(() =>
                    commands.resolveExecutionReceipt({
                      receiptSequence: receipt.receiptSequence,
                      confirmed: true,
                    }),
                  )
                }
              >
                已完成
              </button>
              <button
                type="button"
                onClick={() =>
                  run(() =>
                    commands.resolveExecutionReceipt({
                      receiptSequence: receipt.receiptSequence,
                      confirmed: false,
                    }),
                  )
                }
              >
                未完成
              </button>
            </div>
          ))}
        </Picker>
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
