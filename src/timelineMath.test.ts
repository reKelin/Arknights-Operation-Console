import { describe, expect, it } from "vitest";

import {
  frameToX,
  pointerToFrame,
  stackPositions,
  TIMELINE_PADDING,
  timelineWidth,
} from "./timelineMath";

describe("timelineMath", () => {
  it("round-trips a frame through its timeline coordinate", () => {
    const frame = 1_110;
    const x = frameToX(frame, 1);

    expect(pointerToFrame(x, 0, 0, 1, 3_600)).toBe(frame);
  });

  it("accounts for horizontal scrolling and clamps the result", () => {
    expect(pointerToFrame(100, 20, 200, 1, 300)).toBe(300);
    expect(pointerToFrame(0, 100, 0, 1, 300)).toBe(0);
  });

  it("never makes the content narrower than its viewport", () => {
    expect(timelineWidth(10, 0.5, 900)).toBe(900);
    expect(timelineWidth(3_600, 1, 100)).toBeGreaterThan(TIMELINE_PADDING * 2);
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
});
