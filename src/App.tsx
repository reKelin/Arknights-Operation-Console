import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import appIcon from "../assets/app-icon-small.png";
import {
  type AppSettings,
  type AppTheme,
  type CommandError,
  commands,
  type DraftDirection,
  type DraftEvent,
  type DraftKind,
  events,
  type GameWindowCandidate,
  type ObservedBattleState,
  type RunnerSnapshot,
  type RunStrategy,
  type StageCatalogEntry,
  type UpdateEventInput,
} from "./generated/bindings";
import Timeline from "./Timeline";

type TypedResult<T> =
  | { status: "ok"; data: T }
  | { status: "error"; error: CommandError };

type MenuName = "file" | "axis" | "monitor" | "view" | "help";

const KIND_LABELS: Record<DraftKind, string> = {
  deploy: "部署",
  skill: "技能",
  retreat: "撤退",
};

const STRATEGY_LABELS: Record<RunStrategy, string> = {
  notify: "提前提示",
  pause: "到点暂停请求",
  dryRun: "执行预演",
  proxy: "代理执行",
};

const OBSERVED_STATE_LABELS: Record<ObservedBattleState, string> = {
  unknown: "未知",
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
  if (result.status === "error") {
    throw result.error;
  }
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

function countdownText(frames: number | null): string {
  if (frames === null) {
    return "—";
  }
  return `−${(frames / 30).toFixed(2)}`;
}

function frameTime(frame: number, denominator: number): string {
  const totalSeconds = Math.floor(frame / denominator);
  return `${String(Math.floor(totalSeconds / 60)).padStart(2, "0")}:${String(
    totalSeconds % 60,
  ).padStart(
    2,
    "0",
  )}:${String(frame % denominator).padStart(2, "0")}/${denominator}`;
}

function eventSummary(event: DraftEvent | null): string {
  if (!event) {
    return "未选择操作点";
  }
  return `${KIND_LABELS[event.kind]} · ${event.label || event.id}`;
}

export default function App() {
  const [snapshot, setSnapshot] = useState<RunnerSnapshot | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [zoom, setZoom] = useState(1);
  const [activeMenu, setActiveMenu] = useState<MenuName | null>(null);
  const [editing, setEditing] = useState<DraftEvent | null>(null);
  const [metadataOpen, setMetadataOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [windowPickerOpen, setWindowPickerOpen] = useState(false);
  const [gameWindows, setGameWindows] = useState<GameWindowCandidate[]>([]);
  const [scanningWindows, setScanningWindows] = useState(false);
  const [stagePickerOpen, setStagePickerOpen] = useState(false);
  const [stageCandidates, setStageCandidates] = useState<StageCatalogEntry[]>(
    [],
  );
  const [stageSearching, setStageSearching] = useState(false);
  const [tracePreviewFrame, setTracePreviewFrame] = useState<number | null>(
    null,
  );
  const [recordingSegmentIndex, setRecordingSegmentIndex] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [aboutOpen, setAboutOpen] = useState(false);
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
        if (!disposed) {
          setSnapshot(unwrap(result));
        }
      })
      .catch((reason) => setError(messageOf(reason)));

    events.runnerSnapshot
      .listen((event) => {
        if (!disposed) {
          setSnapshot(event.payload);
        }
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
    const notices =
      snapshot?.notices.filter(
        (notice) => notice.sequence > lastNoticeSequence.current,
      ) ?? [];
    const audible = notices.filter((notice) => notice.kind === "notify");
    if (audible.length > 0) {
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

  useEffect(() => {
    if (snapshot) {
      document.documentElement.dataset.theme = snapshot.settings.theme;
    }
  }, [snapshot]);

  useEffect(() => {
    if (snapshot?.monitor.connectionState === "ready") {
      setRecordingSegmentIndex(0);
      setTracePreviewFrame(0);
    } else {
      setTracePreviewFrame(null);
    }
  }, [snapshot?.monitor.connectionState]);

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

  async function runVoid(
    operation: () => Promise<TypedResult<null>>,
  ): Promise<void> {
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
    if (typeof path === "string") {
      if (await run(() => commands.importAxis(path))) {
        setSelectedId(null);
      }
    }
  }

  async function exportAxis() {
    const path = await save({
      defaultPath: `${snapshot?.axis.title || "未命名轴"}.axis.json`,
      filters: [{ name: "AxisLink JSON", extensions: ["json"] }],
    });
    if (path) {
      await run(() => commands.exportAxis(path));
    }
  }

  async function scanGameWindows() {
    setScanningWindows(true);
    try {
      const candidates = unwrap(await commands.listGameWindows());
      setGameWindows(candidates);
      setWindowPickerOpen(true);
      setError(null);
    } catch (reason) {
      setError(messageOf(reason));
    } finally {
      setScanningWindows(false);
    }
  }

  async function searchStages(query: string): Promise<StageCatalogEntry[]> {
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

  async function openStagePicker() {
    setStagePickerOpen(true);
    await searchStages("");
  }

  async function chooseRecording() {
    const path = await open({
      multiple: false,
      filters: [{ name: "游戏录屏", extensions: ["mkv", "mp4"] }],
    });
    if (typeof path === "string") {
      await run(() => commands.analyzeRecording(path));
    }
  }

  if (!snapshot) {
    return (
      <main className="loading-shell">
        <span>正在启动 Runner…</span>
        {error && <span className="error-text">{error}</span>}
      </main>
    );
  }

  const statusLabel = {
    waiting: "等待进关",
    running: "战斗中 · 自动计时",
    paused: "计时已冻结",
    ended: "关卡结束",
  }[snapshot.status];
  const stageWarning =
    snapshot.monitor.sourceKind !== "none" &&
    snapshot.stageSafety.status !== "matched"
      ? snapshot.stageSafety.status === "mismatched"
        ? "关卡不匹配"
        : "关卡未确认"
      : null;
  const displayedFrame = tracePreviewFrame ?? snapshot.frame;
  const displayedTime =
    tracePreviewFrame === null
      ? snapshot.time
      : frameTime(tracePreviewFrame, snapshot.settings.framesPerCost);
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
  const lastProxyRecord = snapshot.proxy.records.at(-1) ?? null;

  return (
    <main className="app-shell">
      <header
        className="titlebar"
        onPointerDown={(event) => {
          if (
            event.button === 0 &&
            !(event.target as HTMLElement).closest(
              "button, nav, input, select, a",
            )
          ) {
            getCurrentWindow()
              .startDragging()
              .catch((reason) => setError(messageOf(reason)));
          }
        }}
      >
        <img alt="" className="brand-mark" src={appIcon} />
        <strong>Operation Runner</strong>
        <nav>
          <MenuButton
            active={activeMenu === "file"}
            label="文件"
            onClick={() => setActiveMenu(activeMenu === "file" ? null : "file")}
          >
            <MenuItem label="导入 AxisLink" onClick={importAxis} />
            <MenuItem label="导出 AxisLink" onClick={exportAxis} />
          </MenuButton>
          <MenuButton
            active={activeMenu === "axis"}
            label="轴"
            onClick={() => setActiveMenu(activeMenu === "axis" ? null : "axis")}
          >
            <MenuItem label="轴属性" onClick={() => setMetadataOpen(true)} />
            <MenuItem
              checked={snapshot.recording}
              label="实时录轴"
              onClick={() =>
                run(() => commands.setRecording(!snapshot.recording))
              }
            />
            {(["notify", "pause", "dryRun"] as RunStrategy[]).map(
              (strategy) => (
                <MenuItem
                  checked={snapshot.strategy === strategy}
                  key={strategy}
                  label={STRATEGY_LABELS[strategy]}
                  onClick={() => run(() => commands.setStrategy(strategy))}
                />
              ),
            )}
          </MenuButton>
          <MenuButton
            active={activeMenu === "monitor"}
            label="监控"
            onClick={() =>
              setActiveMenu(activeMenu === "monitor" ? null : "monitor")
            }
          >
            <MenuItem
              label={scanningWindows ? "正在扫描…" : "选择游戏窗口"}
              onClick={scanGameWindows}
            />
            <MenuItem label="分析游戏录屏" onClick={chooseRecording} />
            <MenuItem label="手动确认当前关卡" onClick={openStagePicker} />
            <MenuItem
              label="停止监控"
              onClick={() => run(() => commands.stopMonitor())}
            />
          </MenuButton>
          <MenuButton
            active={activeMenu === "view"}
            label="视图"
            onClick={() => setActiveMenu(activeMenu === "view" ? null : "view")}
          >
            <MenuItem
              checked={snapshot.alwaysOnTop}
              label="窗口置顶"
              onClick={() =>
                run(() => commands.setAlwaysOnTop(!snapshot.alwaysOnTop))
              }
            />
            <MenuItem
              label="显示与监控设置"
              onClick={() => setSettingsOpen(true)}
            />
            {(["dark", "light"] as AppTheme[]).map((theme) => (
              <MenuItem
                checked={snapshot.settings.theme === theme}
                key={theme}
                label={theme === "dark" ? "深色外观" : "浅色外观"}
                onClick={() =>
                  run(() =>
                    commands.updateSettings({
                      ...snapshot.settings,
                      theme,
                    }),
                  )
                }
              />
            ))}
          </MenuButton>
          <MenuButton
            active={activeMenu === "help"}
            label="帮助"
            onClick={() => setActiveMenu(activeMenu === "help" ? null : "help")}
          >
            <MenuItem label="关于" onClick={() => setAboutOpen(true)} />
          </MenuButton>
        </nav>
        <span className="connection-state">
          {snapshot.monitor.sourceName
            ? `${snapshot.monitor.sourceName} · ${
                snapshot.monitor.recordingProgress !== null &&
                snapshot.monitor.connectionState === "analyzing"
                  ? `分析 ${snapshot.monitor.recordingProgress}%`
                  : OBSERVED_STATE_LABELS[snapshot.monitor.battleState]
              }`
            : "未选择监控源"}
          {snapshot.stageSafety.observedStage
            ? ` · ${snapshot.stageSafety.observedStage.code} ${snapshot.stageSafety.observedStage.name}`
            : ""}
        </span>
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

      <section className="timer-strip">
        <div className="timer-primary">
          <strong>{displayedTime}</strong>
          <div>
            <span>F{displayedFrame.toString().padStart(5, "0")}</span>
            <span>30 Hz · 费用秒 {snapshot.settings.framesPerCost}f</span>
            <span
              className={`status status--${
                stageWarning ? "paused" : snapshot.status
              }`}
            >
              {stageWarning ??
                (snapshot.monitor.sourceKind === "none"
                  ? statusLabel
                  : OBSERVED_STATE_LABELS[snapshot.monitor.battleState])}
            </span>
            <span className="accuracy">
              {snapshot.monitor.sourceKind === "none"
                ? `±${snapshot.errorFrames} 帧`
                : `可信度 ${snapshot.monitor.confidence}%`}
            </span>
          </div>
        </div>
        <div className="next-action">
          <span>下一操作</span>
          <strong>{eventSummary(snapshot.nextEvent)}</strong>
          <b>{countdownText(snapshot.countdownFrames)}</b>
        </div>
        <div className="timer-actions">
          <button
            className={
              snapshot.recording ? "record-button active" : "record-button"
            }
            onClick={() =>
              run(() => commands.setRecording(!snapshot.recording))
            }
            type="button"
          >
            <span />
            {snapshot.recording ? "停止录轴" : "开始录轴"}
          </button>
          <button
            className={
              snapshot.proxy.enabled ? "proxy-button active" : "proxy-button"
            }
            onClick={() => run(() => commands.requestProxyExecution())}
            type="button"
          >
            {snapshot.proxy.status === "confirming"
              ? "再次确认代理执行"
              : snapshot.proxy.enabled
                ? "关闭代理执行"
                : "代理执行"}
          </button>
          {(snapshot.proxy.enabled ||
            snapshot.proxy.status === "confirming") && (
            <button
              className="danger-button"
              onClick={() => run(() => commands.emergencyStop())}
              type="button"
            >
              急停 <kbd>F12</kbd>
            </button>
          )}
        </div>
      </section>

      <section className="axis-panel">
        <div className="axis-toolbar">
          <strong>当前轴</strong>
          <span className="axis-name">{snapshot.axis.title}.axis.json</span>
          <span
            className={
              snapshot.recording ? "recording-state active" : "recording-state"
            }
          >
            {snapshot.recording ? "实时录轴" : "编辑"}
          </span>
          {(["deploy", "skill", "retreat"] as DraftKind[]).map(
            (kind, index) => (
              <button
                className="quick-action"
                key={kind}
                onClick={() => run(() => commands.recordEvent(kind))}
                type="button"
              >
                {KIND_LABELS[kind]}
                <kbd>F{index + 1}</kbd>
              </button>
            ),
          )}
          <button
            className={
              snapshot.clearPending ? "clear-axis pending" : "clear-axis"
            }
            onClick={() => run(() => commands.requestClearAxis())}
            type="button"
          >
            {snapshot.clearPending ? "再次清空" : "清空"}
            <kbd>F4</kbd>
          </button>
          <div className="axis-toolbar__end">
            <button onClick={importAxis} type="button">
              导入
            </button>
            <button onClick={exportAxis} type="button">
              导出
            </button>
            <button
              aria-label="缩小时间轴"
              onClick={() => setZoom(Math.max(0.5, zoom - 0.25))}
              type="button"
            >
              −
            </button>
            <button
              aria-label="放大时间轴"
              onClick={() => setZoom(Math.min(4, zoom + 0.25))}
              type="button"
            >
              ＋
            </button>
          </div>
        </div>

        <Timeline
          currentFrame={displayedFrame}
          events={snapshot.axis.events}
          traceDurationFrames={traceDurationFrames}
          tracePoints={visibleTracePoints}
          onCreate={(frame, kind) =>
            run(() => commands.addEvent({ frame, kind }))
          }
          onEdit={setEditing}
          onMove={(id, frame) => run(() => commands.moveEvent(id, frame))}
          onSelect={setSelectedId}
          onZoom={setZoom}
          selectedId={selectedId}
          zoom={zoom}
        />

        <div className="event-summary">
          {selected ? (
            <>
              <b>{KIND_LABELS[selected.kind]}</b>
              <span>{selected.label || selected.id}</span>
              <span>{selected.tile || "未填写格子"}</span>
              <span>
                {`${frameTime(
                  selected.frame,
                  snapshot.settings.framesPerCost,
                )} · F${selected.frame}`}
              </span>
              {!selected.complete && <em>待补全</em>}
            </>
          ) : (
            <span>{snapshot.lastMessage || "双击时间轴新增操作点"}</span>
          )}
          {traceDurationFrames !== null && (
            <label className="recording-trace-control">
              {snapshot.monitor.recordingSegments.length > 1 ? (
                <select
                  aria-label="关卡区段"
                  onChange={(event) => {
                    setRecordingSegmentIndex(
                      Number.parseInt(event.target.value, 10),
                    );
                    setTracePreviewFrame(0);
                  }}
                  value={recordingSegmentIndex}
                >
                  {snapshot.monitor.recordingSegments.map((segment) => (
                    <option key={segment.index} value={segment.index}>
                      {segment.stageRecognition.stage
                        ? `${segment.stageRecognition.stage.code} ${segment.stageRecognition.stage.name}`
                        : `关卡 ${segment.index + 1}（未确认）`}
                    </option>
                  ))}
                </select>
              ) : (
                "录屏轨迹"
              )}
              <input
                max={traceDurationFrames}
                min="0"
                onChange={(event) =>
                  setTracePreviewFrame(Number.parseInt(event.target.value, 10))
                }
                type="range"
                value={tracePreviewFrame ?? 0}
              />
            </label>
          )}
          {snapshot.monitor.stageRecognition.rawText && (
            <span
              className="stage-ocr"
              title={snapshot.monitor.stageRecognition.rawText}
            >
              OCR：
              {snapshot.monitor.stageRecognition.rawText.replaceAll(
                "\n",
                " / ",
              )}
            </span>
          )}
          {snapshot.monitor.stageRecognition.warning && (
            <em title={snapshot.monitor.stageRecognition.warning}>
              {snapshot.monitor.stageRecognition.warning}
            </em>
          )}
          {snapshot.proxy.message && (
            <em title={snapshot.proxy.message}>{snapshot.proxy.message}</em>
          )}
          {lastProxyRecord && (
            <span
              title={`${lastProxyRecord.eventId} · ${lastProxyRecord.message}`}
            >
              最近代理：F{lastProxyRecord.frame}{" "}
              {lastProxyRecord.success ? "完成" : "失败"}
            </span>
          )}
          <small>拖动改帧 · 右键编辑 · 双击空白新增</small>
        </div>
      </section>

      {editing && (
        <EventEditor
          event={editing}
          onCancel={() => setEditing(null)}
          onDelete={async () => {
            if (await run(() => commands.deleteEvent(editing.id))) {
              setEditing(null);
              setSelectedId(null);
            }
          }}
          onSave={async (input) => {
            if (await run(() => commands.updateEvent(input))) {
              setEditing(null);
            }
          }}
        />
      )}

      {metadataOpen && (
        <MetadataEditor
          snapshot={snapshot}
          onCancel={() => setMetadataOpen(false)}
          onSearchStages={searchStages}
          onSave={async (title, stageId) => {
            if (
              await run(() =>
                commands.setAxisMetadata({
                  title,
                  stageId: stageId || null,
                }),
              )
            ) {
              setMetadataOpen(false);
            }
          }}
        />
      )}

      {settingsOpen && (
        <SettingsEditor
          settings={snapshot.settings}
          onCancel={() => setSettingsOpen(false)}
          onSave={async (settings) => {
            if (await run(() => commands.updateSettings(settings))) {
              setSettingsOpen(false);
            }
          }}
        />
      )}

      {windowPickerOpen && (
        <WindowPicker
          candidates={gameWindows}
          onCancel={() => setWindowPickerOpen(false)}
          onRefresh={scanGameWindows}
          onSelect={async (id) => {
            if (await run(() => commands.selectGameWindow(id))) {
              setWindowPickerOpen(false);
            }
          }}
        />
      )}

      {stagePickerOpen && (
        <StagePicker
          candidates={stageCandidates}
          searching={stageSearching}
          onCancel={() => setStagePickerOpen(false)}
          onSearch={searchStages}
          onSelect={async (stageId) => {
            if (await run(() => commands.setManualStage(stageId))) {
              setStagePickerOpen(false);
            }
          }}
        />
      )}

      {aboutOpen && (
        <div className="modal-backdrop">
          <section className="compact-dialog about-dialog">
            <h2>Arknights Operation Runner</h2>
            <p>实机视觉时钟 · AxisLink 30 Hz 作战轴</p>
            <button onClick={() => setAboutOpen(false)} type="button">
              关闭
            </button>
          </section>
        </div>
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

type MenuButtonProps = {
  active: boolean;
  label: string;
  onClick: () => void;
  children: ReactNode;
};

function MenuButton({ active, label, onClick, children }: MenuButtonProps) {
  return (
    <div className="menu-root">
      <button
        className={active ? "menu-trigger active" : "menu-trigger"}
        onClick={onClick}
        type="button"
      >
        {label}
      </button>
      {active && <div className="menu-popover">{children}</div>}
    </div>
  );
}

type MenuItemProps = {
  checked?: boolean;
  label: string;
  onClick: () => void;
};

function MenuItem({ checked, label, onClick }: MenuItemProps) {
  return (
    <button className="menu-item" onClick={onClick} type="button">
      <span>{checked ? "✓" : ""}</span>
      {label}
    </button>
  );
}

type EventEditorProps = {
  event: DraftEvent;
  onSave: (input: UpdateEventInput) => void;
  onDelete: () => void;
  onCancel: () => void;
};

function EventEditor({ event, onSave, onDelete, onCancel }: EventEditorProps) {
  const [frame, setFrame] = useState(String(event.frame));
  const [kind, setKind] = useState<DraftKind>(event.kind);
  const [operator, setOperator] = useState(event.operator ?? "");
  const [tile, setTile] = useState(event.tile ?? "");
  const [direction, setDirection] = useState<DraftDirection>(
    event.direction ?? "right",
  );
  const [label, setLabel] = useState(event.label ?? "");

  return (
    <div className="modal-backdrop">
      <form
        className="compact-dialog event-editor"
        onSubmit={(submitEvent) => {
          submitEvent.preventDefault();
          onSave({
            id: event.id,
            frame: Math.max(0, Number.parseInt(frame, 10) || 0),
            kind,
            operator: kind === "deploy" ? operator || null : null,
            tile: tile.trim().toUpperCase() || null,
            direction: kind === "deploy" ? direction : null,
            label: label || null,
          });
        }}
      >
        <h2>编辑操作点</h2>
        <label>
          类型
          <select
            onChange={(event) => setKind(event.target.value as DraftKind)}
            value={kind}
          >
            {(["deploy", "skill", "retreat"] as DraftKind[]).map((value) => (
              <option key={value} value={value}>
                {KIND_LABELS[value]}
              </option>
            ))}
          </select>
        </label>
        <label>
          帧
          <input
            min="0"
            onChange={(event) => setFrame(event.target.value)}
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
              value={operator}
            />
          </label>
        )}
        <label>
          格子
          <input
            maxLength={3}
            onChange={(event) => setTile(event.target.value.toUpperCase())}
            pattern="[A-I](?:[1-9]|[12][0-9]|3[0-6])"
            placeholder="C5"
            value={tile}
          />
        </label>
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
        <label>
          说明
          <input
            maxLength={120}
            onChange={(event) => setLabel(event.target.value)}
            value={label}
          />
        </label>
        <footer>
          <button className="danger-button" onClick={onDelete} type="button">
            删除
          </button>
          <span />
          <button onClick={onCancel} type="button">
            取消
          </button>
          <button className="button--primary" type="submit">
            保存
          </button>
        </footer>
      </form>
    </div>
  );
}

type MetadataEditorProps = {
  snapshot: RunnerSnapshot;
  onSave: (title: string, stageId: string) => void;
  onSearchStages: (query: string) => Promise<StageCatalogEntry[]>;
  onCancel: () => void;
};

function MetadataEditor({
  snapshot,
  onSave,
  onSearchStages,
  onCancel,
}: MetadataEditorProps) {
  const [title, setTitle] = useState(snapshot.axis.title);
  const [stageId, setStageId] = useState(snapshot.axis.stageId ?? "");
  const [stageResults, setStageResults] = useState<StageCatalogEntry[]>([]);
  return (
    <div className="modal-backdrop">
      <form
        className="compact-dialog metadata-editor"
        onSubmit={(event) => {
          event.preventDefault();
          onSave(title, stageId);
        }}
      >
        <h2>轴属性</h2>
        <label>
          标题
          <input
            maxLength={128}
            onChange={(event) => setTitle(event.target.value)}
            value={title}
          />
        </label>
        <label>
          关卡
          <span className="stage-search-row">
            <input
              onChange={(event) => setStageId(event.target.value)}
              placeholder="代码、名称或 stageId"
              value={stageId}
            />
            <button
              onClick={async () =>
                setStageResults(await onSearchStages(stageId))
              }
              type="button"
            >
              搜索
            </button>
          </span>
        </label>
        {stageResults.length > 0 && (
          <select
            aria-label="关卡搜索结果"
            onChange={(event) => setStageId(event.target.value)}
            size={Math.min(5, stageResults.length)}
            value={
              stageResults.some((stage) => stage.id === stageId) ? stageId : ""
            }
          >
            <option disabled value="">
              选择匹配关卡
            </option>
            {stageResults.map((stage) => (
              <option key={stage.id} value={stage.id}>
                {stage.code} · {stage.name} · {stage.id}
              </option>
            ))}
          </select>
        )}
        <footer>
          <span />
          <button onClick={onCancel} type="button">
            取消
          </button>
          <button className="button--primary" type="submit">
            保存
          </button>
        </footer>
      </form>
    </div>
  );
}

type SettingsEditorProps = {
  settings: AppSettings;
  onSave: (settings: AppSettings) => void;
  onCancel: () => void;
};

function SettingsEditor({ settings, onSave, onCancel }: SettingsEditorProps) {
  const [theme, setTheme] = useState(settings.theme);
  const [framesPerCost, setFramesPerCost] = useState(
    String(settings.framesPerCost),
  );
  const [gameUiScale, setGameUiScale] = useState(String(settings.gameUiScale));
  return (
    <div className="modal-backdrop">
      <form
        className="compact-dialog settings-editor"
        onSubmit={(event) => {
          event.preventDefault();
          onSave({
            version: settings.version,
            theme,
            framesPerCost: Number.parseInt(framesPerCost, 10),
            gameUiScale: Number.parseInt(gameUiScale, 10),
          });
        }}
      >
        <h2>显示与监控设置</h2>
        <label>
          外观
          <select
            onChange={(event) => setTheme(event.target.value as AppTheme)}
            value={theme}
          >
            <option value="dark">深色</option>
            <option value="light">浅色</option>
          </select>
        </label>
        <label>
          费用帧分母
          <input
            list="frames-per-cost-presets"
            max="150"
            min="15"
            onChange={(event) => setFramesPerCost(event.target.value)}
            required
            type="number"
            value={framesPerCost}
          />
          <datalist id="frames-per-cost-presets">
            {[30, 45, 60, 90].map((value) => (
              <option key={value} value={value} />
            ))}
          </datalist>
        </label>
        <label>
          游戏 UI 比例
          <input
            max="100"
            min="0"
            onChange={(event) => setGameUiScale(event.target.value)}
            required
            type="number"
            value={gameUiScale}
          />
        </label>
        <p className="form-note">
          事件帧始终为 30 Hz；分母只用于费用周期与时间显示。
        </p>
        <footer>
          <span />
          <button onClick={onCancel} type="button">
            取消
          </button>
          <button className="button--primary" type="submit">
            保存
          </button>
        </footer>
      </form>
    </div>
  );
}

type WindowPickerProps = {
  candidates: GameWindowCandidate[];
  onSelect: (id: string) => void;
  onRefresh: () => void;
  onCancel: () => void;
};

function WindowPicker({
  candidates,
  onSelect,
  onRefresh,
  onCancel,
}: WindowPickerProps) {
  return (
    <div className="modal-backdrop">
      <section className="compact-dialog window-picker">
        <h2>选择明日方舟窗口</h2>
        <div className="window-candidates">
          {candidates.length === 0 ? (
            <p>未发现可捕获的 Arknights.exe 窗口。</p>
          ) : (
            candidates.map((candidate) => (
              <button
                key={candidate.id}
                onClick={() => onSelect(candidate.id)}
                type="button"
              >
                <strong>{candidate.title || "明日方舟"}</strong>
                <span>
                  {candidate.width}×{candidate.height}
                </span>
              </button>
            ))
          )}
        </div>
        <footer>
          <button onClick={onRefresh} type="button">
            重新扫描
          </button>
          <span />
          <button onClick={onCancel} type="button">
            取消
          </button>
        </footer>
      </section>
    </div>
  );
}

type StagePickerProps = {
  candidates: StageCatalogEntry[];
  searching: boolean;
  onSelect: (id: string) => void;
  onSearch: (query: string) => void;
  onCancel: () => void;
};

function StagePicker({
  candidates,
  searching,
  onSelect,
  onSearch,
  onCancel,
}: StagePickerProps) {
  const [query, setQuery] = useState("");
  return (
    <div className="modal-backdrop">
      <section className="compact-dialog stage-picker">
        <h2>手动确认当前关卡</h2>
        <form
          className="stage-search-row"
          onSubmit={(event) => {
            event.preventDefault();
            onSearch(query);
          }}
        >
          <input
            onChange={(event) => setQuery(event.target.value)}
            placeholder="输入关卡代码、名称或 stageId"
            value={query}
          />
          <button type="submit">{searching ? "搜索中…" : "搜索"}</button>
        </form>
        <div className="stage-candidates">
          {candidates.length === 0 ? (
            <p>没有匹配的关卡。</p>
          ) : (
            candidates.map((stage) => (
              <button
                key={stage.id}
                onClick={() => onSelect(stage.id)}
                type="button"
              >
                <strong>
                  {stage.code || "无代码"} · {stage.name}
                </strong>
                <span>{stage.id}</span>
              </button>
            ))
          )}
        </div>
        <footer>
          <span />
          <button onClick={onCancel} type="button">
            取消
          </button>
        </footer>
      </section>
    </div>
  );
}
