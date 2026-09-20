import { expect, it } from "vitest";
import { eventReviewStatus, operatorName } from "./console";

it("界面把内部单位键显示为中文名称", () => {
  expect(operatorName("char_2027_wang")).toBe("望");
  expect(operatorName("char_1050_chen3")).toBe("赤刃明霄陈");
  expect(operatorName("token_10064_wang_stone1")).toBe("棋子");
});

it("独立显示缺项、参数校验与时间状态", () => {
  const event = {
    kind: "deploy" as const,
    operator: "望",
    tile: "C4",
    direction: "up" as const,
    complete: false,
    timeConfirmation: "unconfirmed" as const,
  };
  expect(eventReviewStatus(event)).toBe("部署单位未识别 · 时间待确认");
  expect(eventReviewStatus({ ...event, timeConfirmation: "observed" })).toBe(
    "部署单位未识别",
  );
  expect(
    eventReviewStatus({ ...event, operator: "char_2027_wang", complete: true }),
  ).toBe("时间待确认");
  expect(eventReviewStatus({ ...event, tile: null })).toBe(
    "待补全参数 · 时间待确认",
  );
  expect(
    eventReviewStatus({
      ...event,
      complete: true,
      timeConfirmation: "observed",
    }),
  ).toBe("");
});
