import {
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import type {
  DraftEvent,
  DraftKind,
  RecordingTracePoint,
} from "./generated/bindings";
import {
  clampViewFrames,
  frameToX,
  groupTimelineEvents,
  majorTickFrames,
  pointerToFrame,
  TIMELINE_PADDING,
  timelineWidth,
  zoomedScrollLeft,
} from "./timelineMath";

type TimelineProps = {
  events: DraftEvent[];
  currentFrame: number;
  traceDurationFrames: number | null;
  tracePoints: RecordingTracePoint[];
  viewFrames: number;
  selectedId: string | null;
  editable: boolean;
  onSelect: (id: string) => void;
  onCreate: (frame: number, kind: DraftKind) => void;
  onMove: (id: string, frame: number) => void;
  onEdit: (event: DraftEvent) => void;
  onViewFrames: (frames: number) => void;
};

const KIND_LABELS: Record<DraftKind, string> = {
  bookmark: "待分类",
  deploy: "部署",
  skill: "技能",
  retreat: "撤退",
};

export default function Timeline({
  events,
  currentFrame,
  traceDurationFrames,
  tracePoints,
  viewFrames,
  selectedId,
  editable,
  onSelect,
  onCreate,
  onMove,
  onEdit,
  onViewFrames,
}: TimelineProps) {
  const viewportRef = useRef<HTMLFieldSetElement>(null);
  const [viewportWidth, setViewportWidth] = useState(760);
  const [drag, setDrag] = useState<{ id: string; frame: number } | null>(null);
  const [createFrame, setCreateFrame] = useState<number | null>(null);
  const maxFrame = useMemo(
    () =>
      Math.max(
        viewFrames,
        currentFrame + Math.ceil(viewFrames / 3),
        traceDurationFrames ?? 0,
        ...events.map((event) => event.frame + Math.ceil(viewFrames / 6)),
      ),
    [currentFrame, events, traceDurationFrames, viewFrames],
  );
  const contentWidth = timelineWidth(maxFrame, viewFrames, viewportWidth);
  const majorStep = majorTickFrames(viewFrames, viewportWidth);
  const minorStep = majorStep / 5;
  const ticks = useMemo(
    () =>
      Array.from(
        { length: Math.floor(maxFrame / minorStep) + 1 },
        (_, index) => index * minorStep,
      ),
    [maxFrame, minorStep],
  );
  const eventGroups = useMemo(
    () => groupTimelineEvents(events, viewFrames, viewportWidth),
    [events, viewFrames, viewportWidth],
  );
  const stateChanges = useMemo(
    () =>
      tracePoints.filter(
        (point, index) =>
          index === 0 ||
          tracePoints[index - 1]?.battleState !== point.battleState,
      ),
    [tracePoints],
  );

  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setViewportWidth(entry.contentRect.width);
    });
    observer.observe(viewport);
    return () => observer.disconnect();
  }, []);

  const selectedFrame = events.find((event) => event.id === selectedId)?.frame;
  useEffect(() => {
    const viewport = viewportRef.current;
    if (viewport && selectedFrame !== undefined) {
      viewport.scrollTo({
        left: Math.max(
          0,
          frameToX(selectedFrame, viewFrames, viewportWidth) -
            viewport.clientWidth / 2,
        ),
        behavior: "smooth",
      });
    }
  }, [selectedFrame, viewFrames, viewportWidth]);

  function frameFromPointer(clientX: number): number {
    const viewport = viewportRef.current;
    if (!viewport) return 0;
    const rect = viewport.getBoundingClientRect();
    return pointerToFrame(
      clientX,
      rect.left,
      viewport.scrollLeft,
      viewFrames,
      viewportWidth,
      maxFrame,
    );
  }

  function beginDrag(
    event: ReactPointerEvent<HTMLButtonElement>,
    point: DraftEvent,
  ) {
    if (!editable) return;
    if (event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    setDrag({ id: point.id, frame: point.frame });
    onSelect(point.id);
    setCreateFrame(null);
  }

  function openCreate(event: ReactMouseEvent<HTMLFieldSetElement>) {
    if (!editable) return;
    if ((event.target as HTMLElement).closest("[data-axis-point]")) return;
    setCreateFrame(frameFromPointer(event.clientX));
  }

  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    function handleWheel(event: WheelEvent) {
      if (!viewport) return;
      event.preventDefault();
      if (!event.altKey) {
        viewport.scrollLeft += event.deltaX || event.deltaY;
        return;
      }
      const nextViewFrames = clampViewFrames(
        viewFrames * (event.deltaY < 0 ? 0.8 : 1.25),
      );
      const rect = viewport.getBoundingClientRect();
      const anchorFrame = pointerToFrame(
        event.clientX,
        rect.left,
        viewport.scrollLeft,
        viewFrames,
        viewportWidth,
        maxFrame,
      );
      const anchorOffset = event.clientX - rect.left;
      onViewFrames(nextViewFrames);
      requestAnimationFrame(() => {
        viewport.scrollLeft = zoomedScrollLeft(
          anchorFrame,
          anchorOffset,
          nextViewFrames,
          viewportWidth,
        );
      });
    }
    viewport.addEventListener("wheel", handleWheel, { passive: false });
    return () => viewport.removeEventListener("wheel", handleWheel);
  }, [maxFrame, onViewFrames, viewFrames, viewportWidth]);

  return (
    <fieldset
      className="timeline-viewport"
      onDoubleClick={openCreate}
      ref={viewportRef}
    >
      <legend className="visually-hidden">作战轴时间线</legend>
      <div className="timeline-content" style={{ width: contentWidth }}>
        <div
          className="timeline-rail timeline-rail--future"
          style={{
            left: TIMELINE_PADDING,
            width: contentWidth - TIMELINE_PADDING * 2,
          }}
        />
        <div
          className="timeline-rail timeline-rail--passed"
          style={{
            left: TIMELINE_PADDING,
            width: Math.max(
              0,
              frameToX(currentFrame, viewFrames, viewportWidth) -
                TIMELINE_PADDING,
            ),
          }}
        />

        {ticks.map((frame) => {
          const major = frame % majorStep === 0;
          return (
            <div
              className={`${major ? "timeline-tick major" : "timeline-tick minor"} ${frame === 0 ? "first" : frame + majorStep > maxFrame ? "last" : ""}`}
              key={frame}
              style={{ left: frameToX(frame, viewFrames, viewportWidth) }}
            >
              {major && <span>{formatTick(frame)}</span>}
            </div>
          );
        })}

        {stateChanges.map((point) => (
          <span
            className={`recording-trace-marker trace--${point.battleState}`}
            key={`${point.sourceFrame}-${point.battleState}`}
            style={{
              left: frameToX(point.gameFrame, viewFrames, viewportWidth),
            }}
            title={`${point.battleState} · F${point.gameFrame}`}
          />
        ))}

        {eventGroups.map((group) => {
          const event =
            group.find((point) => point.id === selectedId) ?? group[0];
          if (!event) return null;
          const frame = drag?.id === event.id ? drag.frame : event.frame;
          return (
            <button
              aria-label={`${event.label || KIND_LABELS[event.kind]}，F${frame}`}
              className={[
                "axis-point",
                group.length > 1 ? "axis-point--grouped" : "",
                `axis-point--${event.kind}`,
                frame <= currentFrame ? "axis-point--passed" : "",
                selectedId === event.id ? "axis-point--selected" : "",
                event.complete ? "" : "axis-point--incomplete",
              ]
                .filter(Boolean)
                .join(" ")}
              data-axis-point
              key={event.id}
              onClick={() => {
                const index = group.findIndex(
                  (point) => point.id === selectedId,
                );
                onSelect(group[(index + 1) % group.length]?.id ?? event.id);
              }}
              onContextMenu={(contextEvent) => {
                contextEvent.preventDefault();
                onEdit(event);
              }}
              onPointerDown={(pointerEvent) => {
                if (group.length === 1) beginDrag(pointerEvent, event);
              }}
              onPointerMove={(pointerEvent) => {
                if (drag)
                  setDrag({
                    ...drag,
                    frame: frameFromPointer(pointerEvent.clientX),
                  });
              }}
              onPointerUp={() => {
                if (drag && drag.frame !== event.frame)
                  onMove(drag.id, drag.frame);
                setDrag(null);
              }}
              onPointerCancel={() => setDrag(null)}
              style={{
                left: frameToX(frame, viewFrames, viewportWidth),
                transform: "translateX(-50%)",
                zIndex: selectedId === event.id ? 10 : 4,
              }}
              title={`${event.label || KIND_LABELS[event.kind]} · F${frame}${!event.complete || event.timeConfirmation === "unconfirmed" ? " · 待校对" : ""}${group.length > 1 ? ` · ${group.length} 个操作，点击切换` : ""} · 右键编辑`}
              type="button"
            >
              <svg
                className="operation-mark"
                viewBox="0 0 20 20"
                aria-hidden="true"
              >
                {(["mark-halo", "mark-shape"] as const).map((className) =>
                  event.kind === "deploy" ? (
                    <circle
                      key={className}
                      className={className}
                      cx="10"
                      cy="10"
                      r="4.5"
                    />
                  ) : event.kind === "skill" ? (
                    <path
                      key={className}
                      className={className}
                      d="M10 4 16 10 10 16 4 10Z"
                    />
                  ) : event.kind === "retreat" ? (
                    <path
                      key={className}
                      className={className}
                      d="M4 2 10 8 16 2 18 4 12 10 18 16 16 18 10 12 4 18 2 16 8 10 2 4Z"
                    />
                  ) : (
                    <rect
                      key={className}
                      className={className}
                      x="6"
                      y="6"
                      width="8"
                      height="8"
                    />
                  ),
                )}
              </svg>
            </button>
          );
        })}

        <div
          className="timeline-playhead"
          style={{ left: frameToX(currentFrame, viewFrames, viewportWidth) }}
        />

        {createFrame !== null && (
          <div
            className="create-menu"
            style={{ left: frameToX(createFrame, viewFrames, viewportWidth) }}
          >
            <span>F{createFrame}</span>
            {(["deploy", "skill", "retreat"] as DraftKind[]).map((kind) => (
              <button
                key={kind}
                onClick={() => {
                  onCreate(createFrame, kind);
                  setCreateFrame(null);
                }}
                type="button"
              >
                {KIND_LABELS[kind]}
              </button>
            ))}
          </div>
        )}
      </div>
    </fieldset>
  );
}

function formatTick(frame: number): string {
  if (frame % 30 !== 0) return `${frame % 30}f`;
  const seconds = frame / 30;
  return `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
}
