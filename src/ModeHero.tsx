import {
  CLOCK_QUALITY_LABELS,
  frameTime,
  KIND_LABELS,
  type Mode,
  STATE_LABELS,
  type TypedResult,
} from "./console";
import {
  commands,
  type DraftEvent,
  type RunnerSnapshot,
} from "./generated/bindings";
import Icon from "./Icon";

type ModeHeroProps = {
  mode: Mode;
  axisEvents: DraftEvent[];
  snapshot: RunnerSnapshot;
  displayedFrame: number;
  displayedTime: string;
  selected: DraftEvent | null;
  recordingSegmentIndex: number;
  onSelectSegment: (index: number) => void;
  onSelectEvent: (id: string) => void;
  onToggleRecording: () => void;
  onChooseRecording: () => void;
  onEdit: () => void;
  onUseAxis: () => void;
  onRequestProxy: () => void;
  onSwitchVideo: () => void;
  onRun: (
    operation: () => Promise<TypedResult<RunnerSnapshot>>,
  ) => Promise<boolean>;
};

export default function ModeHero({
  mode,
  axisEvents,
  snapshot,
  displayedFrame,
  displayedTime,
  selected,
  recordingSegmentIndex,
  onSelectSegment,
  onSelectEvent,
  onToggleRecording,
  onChooseRecording,
  onEdit,
  onUseAxis,
  onRequestProxy,
  onSwitchVideo,
  onRun,
}: ModeHeroProps) {
  const videoReady =
    mode === "video" &&
    snapshot.monitor.sourceKind === "recording" &&
    snapshot.monitor.connectionState === "ready";
  const point =
    mode === "proxy"
      ? snapshot.nextEvent
      : (selected ??
        (mode === "video"
          ? axisEvents[0]
          : axisEvents
              .filter((event) => event.frame <= snapshot.frame)
              .at(-1)) ??
        null);
  const pointIndex = axisEvents.findIndex((event) => event.id === point?.id);
  const takeover = snapshot.session.takeover;
  const takingOver =
    takeover.status === "cancelling" ||
    takeover.status === "awaitingPauseProof";
  const waiting = snapshot.status === "waiting";
  const status =
    snapshot.monitor.connectionState === "error"
      ? "监控已中断"
      : snapshot.clock.quality === "lost"
        ? "时钟待恢复"
        : mode === "proxy"
          ? snapshot.proxy.enabled
            ? snapshot.proxy.runId
              ? "执行中"
              : "等待进关"
            : "就绪"
          : takingOver
            ? "接管中"
            : takeover.status === "unknown"
              ? "接管待确认"
              : !snapshot.recording
                ? "已停止录轴"
                : takeover.status === "recording"
                  ? "接管续录中"
                  : waiting
                    ? "等待进关"
                    : "录轴中";
  const time =
    videoReady && point
      ? frameTime(point.frame, snapshot.settings.framesPerCost)
      : displayedTime;
  const title = point
    ? point.kind === "bookmark"
      ? "待分类操作"
      : `${KIND_LABELS[point.kind]}${point.kind === "deploy" && point.operator ? ` · ${point.operator}` : ""}`
    : mode === "live" && waiting
      ? "等待记录"
      : "暂无操作";
  const direction = point?.direction
    ? ({ right: "朝右", down: "朝下", left: "朝左", up: "朝上" } as const)[
        point.direction
      ]
    : "";
  const detail =
    snapshot.monitor.error ??
    (mode === "proxy" && snapshot.proxy.message
      ? snapshot.proxy.message
      : null) ??
    (point
      ? [
          frameTime(point.frame),
          point.tile,
          direction,
          point.label,
          !point.complete
            ? "待补充类型与参数"
            : point.timeConfirmation === "unconfirmed"
              ? "时间待确认"
              : "",
        ]
          .filter(Boolean)
          .join(" · ")
      : "进入关卡后自动计时");

  if (mode === "video" && !videoReady) {
    const analyzing = snapshot.monitor.connectionState === "analyzing";
    const failed = snapshot.monitor.connectionState === "error";
    return (
      <section className="mode-hero video-hero">
        <div>
          <div className="eyebrow">
            {analyzing || failed ? "当前录屏" : "视频分析"}
          </div>
          <div className="file-title">
            {analyzing || failed
              ? snapshot.monitor.sourceName
              : "从录屏提取操作序列"}
          </div>
          <div className="subline">部署 · 技能 · 撤退</div>
        </div>
        <div className="operation">
          <div className="operation-title">
            {analyzing ? "正在分析" : failed ? "分析未完成" : "从录屏开始"}
          </div>
          <div className="operation-detail">
            {failed
              ? (snapshot.monitor.error ?? "无法读取录屏，请重试")
              : takeover.newRevisionId
                ? `打完后选择录屏 · 从 ${frameTime(snapshot.session.revisions.find((revision) => revision.id === takeover.newRevisionId)?.createdFrame ?? displayedFrame)} 接续`
                : "选择 MP4 或 MKV 录屏"}
          </div>
        </div>
        <div className="hero-actions">
          {analyzing ? (
            <>
              <div className="progress-number">
                {snapshot.monitor.recordingProgress ?? 0}
                <small>%</small>
              </div>
              <div className="eyebrow">已分析录屏</div>
            </>
          ) : (
            <button
              className="button--primary"
              onClick={onChooseRecording}
              type="button"
            >
              <Icon name="folder" />
              {failed ? "重试" : "选择录屏"}
            </button>
          )}
        </div>
      </section>
    );
  }

  return (
    <section
      className={`mode-hero mode-hero--${mode} ${videoReady ? "video-result" : ""}`}
    >
      <div className="clock-block">
        {videoReady && (
          <select
            className="segment-select"
            aria-label="关卡区段"
            value={recordingSegmentIndex}
            onChange={(event) => onSelectSegment(Number(event.target.value))}
          >
            {snapshot.monitor.recordingSegments.map((segment) => (
              <option key={segment.index} value={segment.index}>
                {segment.stageRecognition.stage?.code ?? "未确认关卡"} · 区段{" "}
                {segment.index + 1}
              </option>
            ))}
          </select>
        )}
        <div className="clock">
          {time.slice(0, -2)}
          <span className="frame-digits">{time.slice(-2)}</span>
        </div>
        <div
          className="subline"
          title={`${STATE_LABELS[snapshot.monitor.battleState]} · F${displayedFrame} · 时钟${CLOCK_QUALITY_LABELS[snapshot.clock.quality]}`}
        >
          {videoReady ? (
            <span>录屏操作</span>
          ) : (
            <span className="status-label">
              <span className="status-dot" />
              {status}
            </span>
          )}
          <span>
            {!videoReady && !waiting ? `${snapshot.speed}× · ` : ""}/
            {snapshot.settings.framesPerCost} 帧
          </span>
        </div>
      </div>
      <div className="operation">
        <div className="eyebrow">
          {mode === "proxy"
            ? "下一操作"
            : selected || videoReady
              ? "当前操作"
              : "最近操作"}
        </div>
        <div className="operation-title" title={title}>
          {title}
        </div>
        <div className="operation-detail" title={takeover.message ?? detail}>
          {takingOver || takeover.status === "unknown"
            ? takeover.message
            : detail}
        </div>
      </div>
      <div className="hero-actions">
        {mode === "live" && (
          <>
            {!snapshot.recording && axisEvents.length > 0 ? (
              <>
                <button
                  className="button--primary"
                  onClick={onUseAxis}
                  type="button"
                >
                  用于代理
                </button>
                <button
                  className="quiet"
                  onClick={onToggleRecording}
                  type="button"
                >
                  继续录轴
                </button>
              </>
            ) : (
              <>
                <button
                  className={
                    snapshot.recording && !waiting ? "stop" : "button--primary"
                  }
                  disabled={takingOver}
                  onClick={onToggleRecording}
                  type="button"
                >
                  {snapshot.recording ? "停止录轴" : "开始录轴"}
                </button>
                <button className="quiet" onClick={onEdit} type="button">
                  整理操作
                </button>
              </>
            )}
            {takeover.newRevisionId && (
              <button className="quiet" onClick={onSwitchVideo} type="button">
                改用录屏
              </button>
            )}
          </>
        )}
        {videoReady && (
          <>
            <button
              className="button--primary"
              onClick={onUseAxis}
              type="button"
            >
              用于代理
            </button>
            <div className="paired">
              <button
                disabled={pointIndex <= 0}
                onClick={() => {
                  const previous = axisEvents[pointIndex - 1];
                  if (previous) onSelectEvent(previous.id);
                }}
                type="button"
              >
                上一点
              </button>
              <button
                disabled={pointIndex < 0 || pointIndex >= axisEvents.length - 1}
                onClick={() => {
                  const next = axisEvents[pointIndex + 1];
                  if (next) onSelectEvent(next.id);
                }}
                type="button"
              >
                下一点
              </button>
            </div>
            <button className="quiet" onClick={onEdit} type="button">
              编辑操作
            </button>
          </>
        )}
        {mode === "proxy" && (
          <>
            <div className="countdown">
              {snapshot.countdownFrames === null
                ? "—"
                : (snapshot.countdownFrames / 30).toFixed(1)}
              <small>秒</small>
            </div>
            {snapshot.proxy.enabled ? (
              <>
                {snapshot.proxy.runId && (
                  <button
                    className="button--primary"
                    onClick={() => onRun(() => commands.takeoverNow())}
                    type="button"
                    title="K"
                  >
                    立即接管
                  </button>
                )}
                <button
                  className="quiet"
                  onClick={() => onRun(() => commands.emergencyStop())}
                  type="button"
                >
                  停止代理
                </button>
              </>
            ) : (
              <button
                className="button--primary"
                disabled={!snapshot.settings.bindingsConfirmed}
                title={
                  snapshot.settings.bindingsConfirmed
                    ? "用于下一局代理"
                    : "请先在设置中确认游戏键位"
                }
                onClick={onRequestProxy}
                type="button"
              >
                启用代理
              </button>
            )}
          </>
        )}
      </div>
    </section>
  );
}
