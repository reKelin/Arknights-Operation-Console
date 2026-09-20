import { useEffect, useState } from "react";
import {
  CLOCK_QUALITY_LABELS,
  eventReviewStatus,
  frameTime,
  KIND_LABELS,
  type TypedResult,
} from "./console";
import Picker from "./Dialog";
import {
  commands,
  type DraftDirection,
  type DraftEvent,
  type DraftKind,
  type RunnerSnapshot,
  type StageCatalogEntry,
  type UpdateEventInput,
} from "./generated/bindings";
import Icon from "./Icon";

type AxisEditorProps = {
  snapshot: RunnerSnapshot;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onBack: () => void;
  onRun: (
    operation: () => Promise<TypedResult<RunnerSnapshot>>,
  ) => Promise<boolean>;
  onSearchStages: (query: string) => Promise<StageCatalogEntry[]>;
  onReviewRecording: () => void;
};

export default function AxisEditor({
  snapshot,
  selectedId,
  onSelect,
  onBack,
  onRun,
  onSearchStages,
  onReviewRecording,
}: AxisEditorProps) {
  const editable =
    snapshot.session.currentRevisionId ===
    snapshot.session.activeRecordingRevisionId;
  const selected =
    snapshot.axis.events.find((event) => event.id === selectedId) ??
    snapshot.axis.events[0] ??
    null;
  const [filter, setFilter] = useState<"all" | "incomplete" | DraftKind>("all");
  const [query, setQuery] = useState("");
  const [checked, setChecked] = useState<string[]>(
    selected ? [selected.id] : [],
  );
  const [dialog, setDialog] = useState<"metadata" | "delete" | "shift" | null>(
    null,
  );
  const [offset, setOffset] = useState("0");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const ids = checked.filter((id) =>
    snapshot.axis.events.some((event) => event.id === id),
  );
  const visible = snapshot.axis.events.filter(
    (event) =>
      (filter === "all" ||
        (filter === "incomplete"
          ? !event.complete || event.timeConfirmation === "unconfirmed"
          : event.kind === filter)) &&
      `${event.operator ?? ""} ${event.tile ?? ""} ${event.label ?? ""} ${frameTime(event.frame)} ${event.frame}f ${KIND_LABELS[event.kind]}`
        .toLowerCase()
        .includes(query.trim().toLowerCase()),
  );
  const selectedIndex = snapshot.axis.events.findIndex(
    (event) => event.id === selected?.id,
  );
  const canReorder = (direction: number) =>
    selected &&
    snapshot.axis.events[selectedIndex + direction]?.frame === selected.frame;

  return (
    <section className="editor-page" aria-label="轴编辑">
      <div className="editor-heading">
        <strong>编辑作战轴</strong>
        <input
          aria-label="轴名称"
          defaultValue={snapshot.axis.title}
          disabled={!editable}
          key={snapshot.session.currentRevisionId}
          maxLength={128}
          onBlur={(event) => {
            if (event.target.value !== snapshot.axis.title)
              onRun(() =>
                commands.setAxisMetadata({
                  title: event.target.value,
                  stageId: snapshot.axis.stageId,
                }),
              );
          }}
        />
        <button className="quiet" onClick={onBack} type="button">
          <Icon name="back" />
          返回
        </button>
      </div>
      <div className="editor-filters">
        <label>
          搜索
          <input
            aria-label="搜索操作"
            placeholder="单位、位置、时间或帧"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        <label>
          筛选
          <select
            aria-label="筛选操作类型"
            value={filter}
            onChange={(event) => setFilter(event.target.value as typeof filter)}
          >
            <option value="all">全部操作</option>
            <option value="incomplete">待校对</option>
            {Object.entries(KIND_LABELS).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <span className="editor-selection-count">已选 {ids.length} 项</span>
      </div>
      <div className="editor-list">
        <table className="editor-table" aria-label="操作序列">
          <thead>
            <tr>
              <th>
                <input
                  type="checkbox"
                  aria-label="选择全部显示的操作"
                  checked={
                    visible.length > 0 &&
                    visible.every((event) => ids.includes(event.id))
                  }
                  onChange={(event) =>
                    setChecked(
                      event.target.checked
                        ? [
                            ...new Set([
                              ...ids,
                              ...visible.map((item) => item.id),
                            ]),
                          ]
                        : ids.filter(
                            (id) => !visible.some((item) => item.id === id),
                          ),
                    )
                  }
                />
              </th>
              <th>时间</th>
              <th>帧</th>
              <th>操作</th>
              <th>参数</th>
            </tr>
          </thead>
          <tbody>
            {visible.map((event) => (
              <tr
                className={`${ids.includes(event.id) ? "is-selected" : ""} ${event.id === selected?.id ? "is-active" : ""}`}
                key={event.id}
              >
                <td>
                  <input
                    type="checkbox"
                    aria-label={`选择 ${KIND_LABELS[event.kind]} F${event.frame}`}
                    checked={ids.includes(event.id)}
                    onChange={(change) => {
                      setChecked(
                        change.target.checked
                          ? [...ids, event.id]
                          : ids.filter((id) => id !== event.id),
                      );
                      onSelect(event.id);
                    }}
                  />
                </td>
                <td>
                  <button
                    onClick={() => {
                      setChecked([event.id]);
                      onSelect(event.id);
                    }}
                    type="button"
                  >
                    {frameTime(event.frame)}
                  </button>
                </td>
                <td className="frame-cell">{event.frame}f</td>
                <td>{KIND_LABELS[event.kind]}</td>
                <td>
                  <button
                    onClick={() => {
                      setChecked([event.id]);
                      onSelect(event.id);
                    }}
                    type="button"
                    title={event.label ?? ""}
                  >
                    {[
                      event.operator,
                      event.tile,
                      event.direction
                        ? (
                            {
                              right: "朝右",
                              down: "朝下",
                              left: "朝左",
                              up: "朝上",
                            } as const
                          )[event.direction]
                        : "",
                      eventReviewStatus(event),
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  </button>
                </td>
              </tr>
            ))}
            {!visible.length && (
              <tr>
                <td colSpan={5} className="empty-row">
                  没有匹配的操作
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
      <div className="editor-actions">
        <button
          disabled={!editable || busy}
          onClick={async () => {
            if (
              await onRun(async () => {
                const result = await commands.addEvent({
                  frame: selected?.frame ?? snapshot.frame,
                  kind: "bookmark",
                });
                if (result.status === "ok") {
                  const added = result.data.axis.events.find(
                    (event) =>
                      !snapshot.axis.events.some(
                        (previous) => previous.id === event.id,
                      ),
                  );
                  if (added) {
                    onSelect(added.id);
                    setChecked([added.id]);
                  }
                }
                return result;
              })
            ) {
              setFilter("all");
              setQuery("");
            }
          }}
          type="button"
        >
          <Icon name="plus" />
          添加操作
        </button>
        <button
          disabled={!editable || !ids.length || busy}
          onClick={() => setDialog("delete")}
          type="button"
        >
          移除
        </button>
        <button
          disabled={!editable || !ids.length || busy}
          onClick={() => {
            setOffset("0");
            setDialog("shift");
          }}
          type="button"
        >
          批量改时
        </button>
        <span className="action-spacer" />
        <button
          disabled={!editable || !canReorder(-1) || busy}
          onClick={() =>
            selected && onRun(() => commands.reorderEvent(selected.id, -1))
          }
          title="调整同帧操作顺序"
          type="button"
        >
          上移
        </button>
        <button
          disabled={!editable || !canReorder(1) || busy}
          onClick={() =>
            selected && onRun(() => commands.reorderEvent(selected.id, 1))
          }
          title="调整同帧操作顺序"
          type="button"
        >
          下移
        </button>
        <button
          onClick={() => {
            const pending = snapshot.axis.events.filter(
              (event) =>
                !event.complete || event.timeConfirmation === "unconfirmed",
            );
            setNotice(
              pending.length
                ? `${pending.length} 个操作需要校对`
                : "操作参数与时间已确认",
            );
            if (pending[0]) {
              setFilter("incomplete");
              setQuery("");
              onSelect(pending[0].id);
            }
          }}
          type="button"
        >
          检查操作
        </button>
      </div>
      <fieldset className="editor-fieldset" disabled={!editable || busy}>
        {selected ? (
          <EventForm
            key={selected.id}
            event={selected}
            onSave={(input) => onRun(() => commands.updateEvent(input))}
            onConfirmTime={(manualCorrectionConfirmed) =>
              onRun(() =>
                commands.confirmEventTime({
                  id: selected.id,
                  frame: selected.frame,
                  manualCorrectionConfirmed,
                }),
              )
            }
          />
        ) : (
          <div className="editor-empty">
            选择一项操作以编辑，或添加新的操作。
          </div>
        )}
      </fieldset>
      <div className="editor-footer">
        <span>
          {visible.length} / {snapshot.axis.events.length} 个操作 ·{" "}
          {editable ? "修改即生效" : "旧版本只读"}
          {notice ? ` · ${notice}` : ""}
        </span>
        <div>
          <button
            className="quiet"
            onClick={() => setDialog("metadata")}
            type="button"
          >
            轴信息
          </button>
          {snapshot.monitor.sourceKind === "recording" && (
            <button className="quiet" onClick={onReviewRecording} type="button">
              录屏接续
            </button>
          )}
        </div>
      </div>
      {dialog === "metadata" && (
        <Picker title="轴信息" onClose={() => setDialog(null)}>
          <fieldset className="editor-fieldset" disabled={!editable}>
            <AxisMetadata
              snapshot={snapshot}
              onRun={onRun}
              onSearchStages={onSearchStages}
            />
          </fieldset>
        </Picker>
      )}
      {dialog === "delete" && (
        <Picker title="移除操作" onClose={() => setDialog(null)}>
          <p>移除选中的 {ids.length} 个操作？</p>
          <div className="dialog-actions">
            <button onClick={() => setDialog(null)} type="button">
              取消
            </button>
            <button
              className="stop"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                try {
                  for (const id of ids) {
                    if (!(await onRun(() => commands.deleteEvent(id)))) return;
                  }
                  setChecked([]);
                  setDialog(null);
                } finally {
                  setBusy(false);
                }
              }}
              type="button"
            >
              移除
            </button>
          </div>
        </Picker>
      )}
      {dialog === "shift" && (
        <Picker title="批量调整时间" onClose={() => setDialog(null)}>
          <form
            onSubmit={async (event) => {
              event.preventDefault();
              if (await onRun(() => commands.shiftEvents(ids, Number(offset))))
                setDialog(null);
            }}
          >
            <label className="batch-offset">
              帧偏移
              <input
                type="number"
                step="1"
                required
                value={offset}
                onChange={(event) => setOffset(event.target.value)}
              />
            </label>
            <p>影响 {ids.length} 个已选操作；修改后需要重新确认时间。</p>
            <div className="dialog-actions">
              <button onClick={() => setDialog(null)} type="button">
                取消
              </button>
              <button className="button--primary" type="submit">
                应用
              </button>
            </div>
          </form>
        </Picker>
      )}
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
  onConfirmTime,
}: {
  event: DraftEvent;
  onSave: (input: UpdateEventInput) => Promise<boolean>;
  onConfirmTime: (manualCorrectionConfirmed: boolean) => Promise<boolean>;
}) {
  const [frame, setFrame] = useState(String(event.frame));
  const [kind, setKind] = useState<DraftKind>(event.kind);
  const [operator, setOperator] = useState(event.operator ?? "");
  const [tile, setTile] = useState(event.tile ?? "");
  const [direction, setDirection] = useState<DraftDirection | "">(
    event.direction ?? "",
  );
  const [label, setLabel] = useState(event.label ?? "");
  const [manualCorrectionConfirmed, setManualCorrectionConfirmed] =
    useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    if (document.activeElement?.getAttribute("name") !== "edit-frame")
      setFrame(String(event.frame));
  }, [event.frame]);
  const outsideObservedRange =
    Number(frame) < event.frameRange.start ||
    Number(frame) > event.frameRange.end;

  async function saveFields() {
    if (
      !frame ||
      !Number.isInteger(Number(frame)) ||
      Number(frame) < 0 ||
      Number(frame) > 2_147_483_647
    ) {
      setError("时间必须是有效的非负整数帧");
      return;
    }
    if (
      kind !== "bookmark" &&
      tile &&
      !/^[A-I]([1-9]|[12][0-9]|3[0-6])$/.test(tile)
    ) {
      setError("位置应为 A1 到 I36");
      return;
    }
    const input: UpdateEventInput = {
      id: event.id,
      frame: Number(frame),
      kind,
      operator: kind === "deploy" ? operator.trim() || null : null,
      tile: kind === "bookmark" ? null : tile || null,
      direction: kind === "deploy" ? direction || null : null,
      label: label || null,
    };
    if (
      Object.entries(input).every(
        ([key, value]) => event[key as keyof DraftEvent] === value,
      )
    ) {
      setError("");
      return;
    }
    setSaving(true);
    try {
      if (await onSave(input)) setError("");
      else setError("修改未保存，请检查参数后重试");
    } finally {
      setSaving(false);
    }
  }

  return (
    <form
      className="editor-detail"
      onSubmit={(submit) => {
        submit.preventDefault();
        saveFields();
      }}
      onBlur={(blur) => {
        if ((blur.target as HTMLElement).matches("input, select")) saveFields();
      }}
    >
      <label>
        时间（帧）
        <input
          name="edit-frame"
          type="number"
          min="0"
          step="1"
          value={frame}
          onChange={(change) => setFrame(change.target.value)}
        />
      </label>
      <label>
        操作
        <select
          name="edit-type"
          value={kind}
          onChange={(change) => setKind(change.target.value as DraftKind)}
        >
          {Object.entries(KIND_LABELS).map(([value, title]) => (
            <option key={value} value={value}>
              {title}
            </option>
          ))}
        </select>
      </label>
      {kind === "deploy" && (
        <label className="unit-field">
          部署单位
          <input
            name="edit-operator"
            value={operator}
            placeholder="干员 ID"
            onChange={(change) => setOperator(change.target.value)}
          />
        </label>
      )}
      {kind !== "bookmark" && (
        <label>
          位置
          <input
            name="edit-tile"
            value={tile}
            maxLength={3}
            placeholder="例如 C5"
            onChange={(change) => setTile(change.target.value.toUpperCase())}
          />
        </label>
      )}
      {kind === "deploy" && (
        <label className="direction-field">
          朝向
          <select
            name="edit-direction"
            value={direction}
            onChange={(change) =>
              setDirection(change.target.value as DraftDirection | "")
            }
          >
            <option value="">待确认</option>
            <option value="up">朝上</option>
            <option value="right">朝右</option>
            <option value="down">朝下</option>
            <option value="left">朝左</option>
          </select>
        </label>
      )}
      <label className="note-field">
        备注
        <input
          name="edit-note"
          value={label}
          maxLength={120}
          placeholder="可选"
          onChange={(change) => setLabel(change.target.value)}
        />
      </label>
      {event.timeConfirmation === "unconfirmed" && (
        <div className="timing-evidence">
          <span>
            时间待确认 · 观测 F{event.frameRange.start}–F{event.frameRange.end}{" "}
            · 时钟{CLOCK_QUALITY_LABELS[event.clockQuality]}
          </span>
          {outsideObservedRange && (
            <label>
              <input
                type="checkbox"
                checked={manualCorrectionConfirmed}
                onChange={(change) =>
                  setManualCorrectionConfirmed(change.target.checked)
                }
              />
              确认人工校正到观测范围之外
            </label>
          )}
          <button
            disabled={
              saving ||
              Number(frame) !== event.frame ||
              (outsideObservedRange && !manualCorrectionConfirmed)
            }
            onClick={() => onConfirmTime(manualCorrectionConfirmed)}
            type="button"
          >
            确认时间
          </button>
        </div>
      )}
      {error && (
        <span className="editor-detail-error" role="alert">
          {error}
          <button type="submit">重试保存</button>
        </span>
      )}
    </form>
  );
}
