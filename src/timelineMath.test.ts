import { describe, expect, it } from "vitest";

import {
  clampViewFrames,
  frameToX,
  MAX_VIEW_FRAMES,
  MIN_VIEW_FRAMES,
  majorTickFrames,
  pointerToFrame,
  stackPositions,
  TIMELINE_PADDING,
  timelineWidth,
  zoomedScrollLeft,
} from "./timelineMath";

describe("timelineMath", () => {
  it("round-trips a frame through its timeline coordinate", () => {
    const x = frameToX(1_110, 1_800, 900);
    expect(pointerToFrame(x, 0, 0, 1_800, 900, 3_600)).toBe(1_110);
  });

  it("clamps the visible range from 40 frames to ten minutes", () => {
    expect(clampViewFrames(1)).toBe(MIN_VIEW_FRAMES);
    expect(clampViewFrames(30_000)).toBe(MAX_VIEW_FRAMES);
  });

  it("uses five equal minor subdivisions for every major interval", () => {
    for (const range of [40, 300, 1_800, 18_000]) {
      expect(majorTickFrames(range) % 5).toBe(0);
    }
  });

  it("accounts for horizontal scrolling and clamps the result", () => {
    expect(pointerToFrame(100, 20, 200, 300, 900, 300)).toBe(90);
    expect(pointerToFrame(0, 100, 0, 300, 900, 300)).toBe(0);
  });

  it("never makes the content narrower than its viewport", () => {
    expect(timelineWidth(10, 300, 900)).toBe(900);
    expect(timelineWidth(3_600, 300, 100)).toBeGreaterThan(
      TIMELINE_PADDING * 2,
    );
  });

  it("assigns a separate stack position to same-frame operations", () => {
    const positions = stackPositions([
      { id: "a", frame: 30 },
      { id: "b", frame: 30 },
      { id: "c", frame: 60 },
    ]);
    expect(positions.get("a")).toEqual({ index: 0, count: 2 });
    expect(positions.get("b")).toEqual({ index: 1, count: 2 });
    expect(positions.get("c")).toEqual({ index: 0, count: 1 });
  });

  it("keeps the pointed frame under the cursor while zooming", () => {
    const scrollLeft = zoomedScrollLeft(900, 240, 1_800, 1_000);
    expect(pointerToFrame(240, 0, scrollLeft, 1_800, 1_000, 3_600)).toBe(900);
  });
});
