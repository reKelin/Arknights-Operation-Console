export const TIMELINE_PADDING = 24;
export const MIN_VIEW_FRAMES = 40;
export const MAX_VIEW_FRAMES = 18_000;

const MAJOR_STEPS = [
  5, 10, 15, 30, 60, 90, 150, 300, 450, 600, 900, 1_800, 3_600, 6_000,
];

export function clampViewFrames(frames: number): number {
  return Math.min(
    MAX_VIEW_FRAMES,
    Math.max(MIN_VIEW_FRAMES, Math.round(frames)),
  );
}

export function majorTickFrames(
  viewFrames: number,
  viewportWidth = 900,
): number {
  const spacing = viewFrames <= 90 ? 50 : viewFrames <= 300 ? 70 : 100;
  const minimumStep = spacing / pixelsPerFrame(viewFrames, viewportWidth);
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

export function groupTimelineEvents<T extends { frame: number; kind: string }>(
  events: readonly T[],
  viewFrames: number,
  viewportWidth: number,
): T[][] {
  const groups: T[][] = [];
  const scale = pixelsPerFrame(viewFrames, viewportWidth);
  for (const event of events) {
    const previous = groups.at(-1);
    const first = previous?.[0];
    if (
      first &&
      first.kind === event.kind &&
      (event.frame - first.frame) * scale < 14
    )
      previous?.push(event);
    else groups.push([event]);
  }
  return groups;
}
