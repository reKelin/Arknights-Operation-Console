export const TIMELINE_PADDING = 24;
export const MIN_VIEW_FRAMES = 40;
export const MAX_VIEW_FRAMES = 18_000;

const MAJOR_STEPS = [
  5, 10, 25, 50, 100, 150, 300, 600, 900, 1_500, 3_000, 6_000,
];

export function clampViewFrames(frames: number): number {
  return Math.min(
    MAX_VIEW_FRAMES,
    Math.max(MIN_VIEW_FRAMES, Math.round(frames)),
  );
}

export function majorTickFrames(viewFrames: number): number {
  const minimumStep = clampViewFrames(viewFrames) / 8;
  return MAJOR_STEPS.find((step) => step >= minimumStep) ?? 6_000;
}

export function pixelsPerFrame(
  viewFrames: number,
  viewportWidth: number,
): number {
  return (
    Math.max(1, viewportWidth - TIMELINE_PADDING * 2) /
    clampViewFrames(viewFrames)
  );
}

export function timelineWidth(
  maxFrame: number,
  viewFrames: number,
  viewportWidth: number,
): number {
  return Math.max(
    viewportWidth,
    Math.ceil(
      Math.max(0, maxFrame) * pixelsPerFrame(viewFrames, viewportWidth) +
        TIMELINE_PADDING * 2,
    ),
  );
}

export function frameToX(
  frame: number,
  viewFrames: number,
  viewportWidth: number,
): number {
  return (
    TIMELINE_PADDING +
    Math.max(0, frame) * pixelsPerFrame(viewFrames, viewportWidth)
  );
}

export function zoomedScrollLeft(
  anchorFrame: number,
  pointerOffset: number,
  viewFrames: number,
  viewportWidth: number,
): number {
  return Math.max(
    0,
    frameToX(anchorFrame, viewFrames, viewportWidth) - pointerOffset,
  );
}

export function pointerToFrame(
  clientX: number,
  viewportLeft: number,
  scrollLeft: number,
  viewFrames: number,
  viewportWidth: number,
  maxFrame: number,
): number {
  const contentX = clientX - viewportLeft + scrollLeft - TIMELINE_PADDING;
  return Math.min(
    maxFrame,
    Math.max(
      0,
      Math.round(contentX / pixelsPerFrame(viewFrames, viewportWidth)),
    ),
  );
}

export function stackPositions(
  events: ReadonlyArray<{ id: string; frame: number }>,
): Map<string, { index: number; count: number }> {
  const frames = new Map<number, Array<{ id: string }>>();
  for (const event of events) {
    const frameEvents = frames.get(event.frame) ?? [];
    frameEvents.push(event);
    frames.set(event.frame, frameEvents);
  }
  const positions = new Map<string, { index: number; count: number }>();
  for (const frameEvents of frames.values()) {
    frameEvents.forEach((event, index) => {
      positions.set(event.id, { index, count: frameEvents.length });
    });
  }
  return positions;
}
