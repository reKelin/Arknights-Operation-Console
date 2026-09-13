import { open, save } from "@tauri-apps/plugin-dialog";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import appIcon from "../assets/app-icon-small.png";
import {
  type CommandError,
  commands,
  type DraftDirection,
  type DraftEvent,
  type DraftKind,
  events,
  type RunnerSnapshot,
  type RunStrategy,
  type UpdateEventInput,
} from "./generated/bindings";
import Timeline from "./Timeline";

type TypedResult<T> =
  | { status: "ok"; data: T }
  | { status: "error"; error: CommandError };

type MenuName = "file" | "axis" | "view" | "help";

const KIND_LABELS: Record<DraftKind, string> = {
  deploy: "部署",
  skill: "技能",
  retreat: "撤退",
};

const STRATEGY_LABELS: Record<RunStrategy, string> = {
  notify: "提前提示",
  pause: "到点暂停",
  dryRun: "执行预演",
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

function frameTime(frame: number): string {
  const totalSeconds = Math.floor(frame / 30);
  return `${String(Math.floor(totalSeconds / 60)).padStart(2, "0")}:${String(
    totalSeconds % 60,
  ).padStart(2, "0")}.${String(frame % 30).padStart(2, "0")}`;
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
    paused: "模拟已暂停",
    ended: "关卡结束",
  }[snapshot.status];

  return (
    <main className="app-shell">
      <header className="titlebar" data-tauri-drag-region>
        <img alt="" className="brand-mark" src={appIcon} />
        <strong data-tauri-drag-region>Operation Runner</strong>
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
          </MenuButton>
          <MenuButton
            active={activeMenu === "help"}
            label="帮助"
            onClick={() => setActiveMenu(activeMenu === "help" ? null : "help")}
          >
            <MenuItem label="关于" onClick={() => setAboutOpen(true)} />
          </MenuButton>
        </nav>
        <span className="connection-state" data-tauri-drag-region>
          模拟游戏时钟 · 已连接
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
          <strong>{snapshot.time}</strong>
          <div>
            <span>F{snapshot.frame.toString().padStart(5, "0")}</span>
            <span>30 FPS · {snapshot.speed}×</span>
            <span className={`status status--${snapshot.status}`}>
              {statusLabel}
            </span>
            <span className="accuracy">±{snapshot.errorFrames} 帧</span>
          </div>
        </div>
        <div className="next-action">
          <span>下一操作</span>
          <strong>{eventSummary(snapshot.nextEvent)}</strong>
          <b>{countdownText(snapshot.countdownFrames)}</b>
        </div>
        <div className="timer-actions">
          {snapshot.status === "paused" && (
            <button
              className="button button--primary"
              onClick={() => run(() => commands.continueSimulation())}
              type="button"
            >
              继续模拟
            </button>
          )}
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
          currentFrame={snapshot.frame}
          events={snapshot.axis.events}
          onCreate={(frame, kind) =>
            run(() => commands.addEvent({ frame, kind }))
          }
          onEdit={setEditing}
          onMove={(id, frame) => run(() => commands.moveEvent(id, frame))}
          onSelect={setSelectedId}
          selectedId={selectedId}
          zoom={zoom}
        />

        <div className="event-summary">
          {selected ? (
            <>
              <b>{KIND_LABELS[selected.kind]}</b>
              <span>{selected.label || selected.id}</span>
              <span>
                {frameTime(selected.frame)} · F{selected.frame}
              </span>
              {!selected.complete && <em>待补全</em>}
            </>
          ) : (
            <span>{snapshot.lastMessage || "双击时间轴新增操作点"}</span>
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

      {aboutOpen && (
        <div className="modal-backdrop">
          <section className="compact-dialog about-dialog">
            <h2>Arknights Operation Runner</h2>
            <p>交互 Demo · 模拟时钟 · 不接触游戏客户端</p>
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
  const [tileX, setTileX] = useState(String(event.tile?.x ?? 0));
  const [tileY, setTileY] = useState(String(event.tile?.y ?? 0));
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
            operator: operator || null,
            tile:
              kind === "deploy"
                ? {
                    x: Math.max(0, Number.parseInt(tileX, 10) || 0),
                    y: Math.max(0, Number.parseInt(tileY, 10) || 0),
                  }
                : null,
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
        <label>
          干员 ID
          <input
            onChange={(event) => setOperator(event.target.value)}
            placeholder="char_002_amiya"
            value={operator}
          />
        </label>
        {kind === "deploy" && (
          <>
            <label>
              格子
              <span className="coordinate-inputs">
                <input
                  min="0"
                  onChange={(event) => setTileX(event.target.value)}
                  type="number"
                  value={tileX}
                />
                <input
                  min="0"
                  onChange={(event) => setTileY(event.target.value)}
                  type="number"
                  value={tileY}
                />
              </span>
            </label>
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
          </>
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
  onCancel: () => void;
};

function MetadataEditor({ snapshot, onSave, onCancel }: MetadataEditorProps) {
  const [title, setTitle] = useState(snapshot.axis.title);
  const [stageId, setStageId] = useState(snapshot.axis.stageId ?? "");
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
          stageId
          <input
            onChange={(event) => setStageId(event.target.value)}
            placeholder="main_00-01"
            value={stageId}
          />
        </label>
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
