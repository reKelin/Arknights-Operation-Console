import {
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type WheelEvent as ReactWheelEvent,
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
  frameToX,
  pointerToFrame,
  stackPositions,
  TIMELINE_PADDING,
  timelineWidth,
  zoomedScrollLeft,
} from "./timelineMath";

type TimelineProps = {
  events: DraftEvent[];
  currentFrame: number;
  traceDurationFrames: number | null;
  tracePoints: RecordingTracePoint[];
  zoom: number;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onCreate: (frame: number, kind: DraftKind) => void;
  onMove: (id: string, frame: number) => void;
  onEdit: (event: DraftEvent) => void;
  onZoom: (zoom: number) => void;
};

type DragState = {
  id: string;
  frame: number;
};

type CreateState = {
  frame: number;
};

const KIND_LABELS: Record<DraftKind, string> = {
  deploy: "部署",
  skill: "技能",
  retreat: "撤退",
};

export default function Timeline({
  events,
  currentFrame,
  traceDurationFrames,
  tracePoints,
  zoom,
  selectedId,
  onSelect,
  onCreate,
  onMove,
  onEdit,
  onZoom,
}: TimelineProps) {
  const viewportRef = useRef<HTMLFieldSetElement>(null);
  const [viewportWidth, setViewportWidth] = useState(760);
  const [drag, setDrag] = useState<DragState | null>(null);
  const [create, setCreate] = useState<CreateState | null>(null);

  const maxFrame = useMemo(
    () =>
      Math.max(
        3_600,
        currentFrame + 600,
        (traceDurationFrames ?? 0) + 300,
        ...events.map((event) => event.frame + 300),
      ),
    [currentFrame, events, traceDurationFrames],
  );
  const contentWidth = timelineWidth(maxFrame, zoom, viewportWidth);
  const marks = useMemo(() => {
    const step = zoom >= 2 ? 300 : zoom >= 1 ? 600 : 900;
    return Array.from(
      { length: Math.floor(maxFrame / step) + 1 },
      (_, index) => index * step,
    );
  }, [maxFrame, zoom]);
  const eventStacks = useMemo(() => stackPositions(events), [events]);
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
    if (!viewport) {
      return;
    }
    const observer = new ResizeObserver(([entry]) => {
      if (entry) {
        setViewportWidth(entry.contentRect.width);
      }
    });
    observer.observe(viewport);
    return () => observer.disconnect();
  }, []);

  function eventFrame(event: DraftEvent): number {
    return drag?.id === event.id ? drag.frame : event.frame;
  }

  function frameFromPointer(clientX: number): number {
    const viewport = viewportRef.current;
    if (!viewport) {
      return 0;
    }
    const rect = viewport.getBoundingClientRect();
    return pointerToFrame(
      clientX,
      rect.left,
      viewport.scrollLeft,
      zoom,
      maxFrame,
    );
  }

  function beginDrag(
    event: ReactPointerEvent<HTMLButtonElement>,
    point: DraftEvent,
  ) {
    if (event.button !== 0) {
      return;
    }
    event.currentTarget.setPointerCapture(event.pointerId);
    setDrag({ id: point.id, frame: point.frame });
    onSelect(point.id);
    setCreate(null);
  }

  function updateDrag(event: ReactPointerEvent<HTMLButtonElement>) {
    if (!drag) {
      return;
    }
    setDrag({ ...drag, frame: frameFromPointer(event.clientX) });
  }

  function finishDrag() {
    if (!drag) {
      return;
    }
    onMove(drag.id, drag.frame);
    setDrag(null);
  }

  function openCreate(event: ReactMouseEvent<HTMLFieldSetElement>) {
    if ((event.target as HTMLElement).closest("[data-axis-point]")) {
      return;
    }
    setCreate({ frame: frameFromPointer(event.clientX) });
  }

  function handleWheel(event: ReactWheelEvent<HTMLFieldSetElement>) {
    const viewport = viewportRef.current;
    if (!viewport) {
      return;
    }
    event.preventDefault();
    if (!event.altKey) {
      viewport.scrollLeft += event.deltaX || event.deltaY;
      return;
    }
    const nextZoom = Math.min(
      4,
      Math.max(0.5, zoom + (event.deltaY < 0 ? 0.25 : -0.25)),
    );
    if (nextZoom === zoom) {
      return;
    }
    const rect = viewport.getBoundingClientRect();
    const anchorFrame = frameFromPointer(event.clientX);
    const anchorOffset = event.clientX - rect.left;
    onZoom(nextZoom);
    requestAnimationFrame(() => {
      viewport.scrollLeft = zoomedScrollLeft(
        anchorFrame,
        anchorOffset,
        nextZoom,
      );
    });
  }

  return (
    <fieldset
      className="timeline-viewport"
      onDoubleClick={openCreate}
      onWheel={handleWheel}
      ref={viewportRef}
    >
      <legend className="visually-hidden">作战轴时间线</legend>
      <div className="timeline-content" style={{ width: contentWidth }}>
        {marks.map((frame) => (
          <div
            className="timeline-mark"
            key={frame}
            style={{ left: frameToX(frame, zoom) }}
          >
            <span>{frame}f</span>
          </div>
        ))}

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
            width: Math.max(0, frameToX(currentFrame, zoom) - TIMELINE_PADDING),
          }}
        />

        {stateChanges.map((point) => (
          <span
            className={`recording-trace-marker trace--${point.battleState}`}
            key={`${point.sourceFrame}-${point.battleState}`}
            style={{ left: frameToX(point.gameFrame, zoom) }}
            title={`${point.battleState} · F${point.gameFrame}`}
          />
        ))}

        {events.map((event) => {
          const frame = eventFrame(event);
          const passed = frame <= currentFrame;
          const stack = eventStacks.get(event.id) ?? { index: 0, count: 1 };
          const stackOffset = (stack.index - (stack.count - 1) / 2) * 11;
          return (
            <button
              aria-label={`${event.label || KIND_LABELS[event.kind]}，F${frame}`}
              className={[
                "axis-point",
                `axis-point--${event.kind}`,
                passed ? "axis-point--passed" : "",
                selectedId === event.id ? "axis-point--selected" : "",
                event.complete ? "" : "axis-point--incomplete",
              ]
                .filter(Boolean)
                .join(" ")}
              data-axis-point
              key={event.id}
              onClick={() => onSelect(event.id)}
              onContextMenu={(contextEvent) => {
                contextEvent.preventDefault();
                onEdit(event);
              }}
              onPointerDown={(pointerEvent) => beginDrag(pointerEvent, event)}
              onPointerMove={updateDrag}
              onPointerUp={finishDrag}
              style={{
                left: frameToX(frame, zoom),
                transform: `translate(-50%, ${stackOffset}px)`,
                zIndex: selectedId === event.id ? 10 : stack.index + 4,
              }}
              title="拖动改帧，右键编辑"
              type="button"
            >
              <span
                className={
                  stack.index === 0 || selectedId === event.id
                    ? "axis-point__label"
                    : "axis-point__label axis-point__label--hidden"
                }
              >
                {event.label || KIND_LABELS[event.kind]}
                {stack.count > 1 && stack.index === 0
                  ? ` +${stack.count - 1}`
                  : ""}
              </span>
              <span className="axis-point__dot" />
            </button>
          );
        })}

        <div
          className="timeline-playhead"
          style={{ left: frameToX(currentFrame, zoom) }}
        >
          <span />
        </div>

        {create && (
          <div
            className="create-menu"
            style={{ left: frameToX(create.frame, zoom) }}
          >
            <span>F{create.frame}</span>
            {(["deploy", "skill", "retreat"] as DraftKind[]).map((kind) => (
              <button
                key={kind}
                onClick={() => {
                  onCreate(create.frame, kind);
                  setCreate(null);
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
