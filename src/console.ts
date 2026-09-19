import type {
  ClockQuality,
  CommandError,
  ConsoleMode,
  DraftKind,
  ObservedBattleState,
} from "./generated/bindings";

export type TypedResult<T> =
  | { status: "ok"; data: T }
  | { status: "error"; error: CommandError };
export type Mode = "live" | "video" | "proxy";

export const KIND_LABELS: Record<DraftKind, string> = {
  bookmark: "待分类",
  deploy: "部署",
  skill: "技能",
  retreat: "撤退",
};
export const STATE_LABELS: Record<ObservedBattleState, string> = {
  unknown: "状态未知",
  notInBattle: "关卡外",
  battleBegin: "正在进入关卡",
  oneXRunning: "1× 运行",
  twoXRunning: "2× 运行",
  pointTwoXRunning: "0.2× 运行",
  paused: "暂停",
  deployingOperator: "部署中",
  adjustingOperatorFacing: "调整方向",
};
export const MODE_TO_CONSOLE: Record<Mode, ConsoleMode> = {
  live: "manualRecording",
  video: "recordingAnalysis",
  proxy: "proxy",
};
export const CONSOLE_TO_MODE: Record<ConsoleMode, Mode> = {
  manualRecording: "live",
  recordingAnalysis: "video",
  proxy: "proxy",
};
export const CLOCK_QUALITY_LABELS: Record<ClockQuality, string> = {
  waiting: "等待锚点",
  trusted: "可信",
  uncertain: "不确定",
  lost: "已丢失",
};
export const REVISION_SOURCE_LABELS = {
  imported: "导入",
  manual: "人工",
  takeover: "接管续录",
  recordingMerge: "录屏分析",
} as const;

export function unwrap<T>(result: TypedResult<T>): T {
  if (result.status === "error") throw result.error;
  return result.data;
}

export function messageOf(error: unknown): string {
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    return error.message;
  }
  return String(error);
}

export function frameTime(frame: number, denominator = 30): string {
  const seconds = Math.floor(frame / denominator);
  return `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}:${String(frame % denominator).padStart(2, "0")}`;
}
