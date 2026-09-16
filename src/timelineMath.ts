export const TIMELINE_PADDING = 36;
export const BASE_PIXELS_PER_FRAME = 0.32;

export function pixelsPerFrame(zoom: number): number {
  return BASE_PIXELS_PER_FRAME * Math.min(32, Math.max(0.5, zoom));
}

export function timelineWidth(
  maxFrame: number,
  zoom: number,
  viewportWidth: number,
): number {
  return Math.max(
    viewportWidth,
    Math.ceil(maxFrame * pixelsPerFrame(zoom) + TIMELINE_PADDING * 2),
  );
}

export function frameToX(frame: number, zoom: number): number {
  return TIMELINE_PADDING + Math.max(0, frame) * pixelsPerFrame(zoom);
}

export function zoomedScrollLeft(
  anchorFrame: number,
  pointerOffset: number,
  zoom: number,
): number {
  return Math.max(0, frameToX(anchorFrame, zoom) - pointerOffset);
}

export function pointerToFrame(
  clientX: number,
  viewportLeft: number,
  scrollLeft: number,
  zoom: number,
  maxFrame: number,
): number {
  const contentX = clientX - viewportLeft + scrollLeft - TIMELINE_PADDING;
  return Math.min(
    maxFrame,
    Math.max(0, Math.round(contentX / pixelsPerFrame(zoom))),
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
