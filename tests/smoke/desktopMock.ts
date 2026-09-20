import type { Page } from "@playwright/test";
import type {
  AppSettings,
  ConfirmEventTimeInput,
  RunnerSnapshot,
  UpdateEventInput,
} from "../../src/generated/bindings";
import { createSnapshot, stages } from "./fixtures";

type Size = { width: number; height: number };
type InvokeCall = { command: string; args: Record<string, unknown> };

declare global {
  interface Window {
    __resizeSmokeWindow: (size: Size) => Promise<void>;
    __smoke: {
      snapshot: RunnerSnapshot;
      calls: InvokeCall[];
      sizes: Size[];
      errors: string[];
    };
  }
}

// 只模拟 IPC 边界；React、生成绑定、事件处理和 CSS 均使用真实源码。
export async function installDesktopMock(page: Page) {
  await page.exposeFunction("__resizeSmokeWindow", (size: Size) =>
    page.setViewportSize(size),
  );
  await page.addInitScript(
    ({ initial, catalog }) => {
      const state = structuredClone(initial);
      const calls: InvokeCall[] = [];
      const sizes: Size[] = [];
      const errors: string[] = [];
      window.__smoke = { snapshot: state, calls, sizes, errors };
      const callbacks = new Map<number, (payload: unknown) => void>();
      let callbackId = 0;
      let logEnabled = false;

      function snapshot() {
        const current = state.session.revisions.find(
          (item) => item.id === state.session.currentRevisionId,
        );
        if (current) current.axis = structuredClone(state.axis);
        return structuredClone(state);
      }

      async function invoke(
        command: string,
        args: Record<string, unknown> = {},
      ) {
        // 真实 IPC 会调用 Size 等参数的 toJSON，而不是直接传递类实例。
        const inputArgs = JSON.parse(JSON.stringify(args)) as Record<
          string,
          unknown
        >;
        calls.push({ command, args: structuredClone(inputArgs) });
        switch (command) {
          case "get_log_status":
            return { enabled: logEnabled, lines: 0, dropped: 0 };
          case "set_log_enabled":
            logEnabled = Boolean(inputArgs.enabled);
            return { enabled: logEnabled, lines: 1, dropped: 0 };
          case "read_logs":
            return args.errorsOnly
              ? "ERROR test: 测试错误"
              : "INFO test: 测试历史";
          case "export_logs":
            return null;
          case "get_snapshot":
            return snapshot();
          case "plugin:event|listen":
            return ++callbackId;
          case "plugin:event|unlisten":
            return;
          case "plugin:window|set_size": {
            const value = inputArgs.value as { Logical: Size };
            if (!value?.Logical) {
              throw new Error("冒烟仅接受逻辑窗口尺寸");
            }
            sizes.push(value.Logical);
            await window.__resizeSmokeWindow(value.Logical);
            return;
          }
          case "set_console_mode":
            state.consoleMode = inputArgs.mode as RunnerSnapshot["consoleMode"];
            return snapshot();
          case "set_always_on_top":
            state.alwaysOnTop = Boolean(inputArgs.enabled);
            return snapshot();
          case "set_recording":
            state.recording = Boolean(inputArgs.enabled);
            return snapshot();
          case "set_strategy":
            state.strategy = inputArgs.strategy as RunnerSnapshot["strategy"];
            return snapshot();
          case "update_settings": {
            const input = inputArgs.input as AppSettings;
            const changed =
              input.pauseKey !== state.settings.pauseKey ||
              input.skillKey !== state.settings.skillKey ||
              input.retreatKey !== state.settings.retreatKey;
            state.settings = {
              ...input,
              bindingsConfirmed: changed ? false : input.bindingsConfirmed,
            };
            return snapshot();
          }
          case "select_axis_revision": {
            const revision = state.session.revisions.find(
              (item) => item.id === inputArgs.revisionId,
            );
            if (!revision) throw new Error("未知冒烟版本");
            state.session.currentRevisionId = revision.id;
            state.axis = structuredClone(revision.axis);
            return snapshot();
          }
          case "set_axis_metadata":
            Object.assign(state.axis, inputArgs.input);
            return snapshot();
          case "list_stages":
            return structuredClone(catalog);
          case "update_event": {
            const input = inputArgs.input as UpdateEventInput;
            const event = state.axis.events.find(
              (item) => item.id === input.id,
            );
            if (!event) throw new Error("未知冒烟操作");
            if (event.frame !== input.frame)
              event.timeConfirmation = "unconfirmed";
            Object.assign(event, input);
            return snapshot();
          }
          case "confirm_event_times": {
            const inputs = inputArgs.inputs as ConfirmEventTimeInput[];
            for (const input of inputs) {
              const event = state.axis.events.find(
                (item) => item.id === input.id,
              );
              if (!event) throw new Error("未知冒烟操作");
              const inside =
                input.frame >= event.frameRange.start &&
                input.frame <= event.frameRange.end;
              if (!inside && !input.manualCorrectionConfirmed)
                throw new Error("缺少范围外人工确认");
            }
            for (const input of inputs) {
              const event = state.axis.events.find(
                (item) => item.id === input.id,
              );
              if (!event) throw new Error("未知冒烟操作");
              event.frame = input.frame;
              event.timeConfirmation =
                input.frame >= event.frameRange.start &&
                input.frame <= event.frameRange.end
                  ? "observed"
                  : "manuallyCorrected";
            }
            return snapshot();
          }
          case "add_event": {
            const input = inputArgs.input as {
              frame: number;
              kind: "bookmark";
            };
            const example = state.axis.events[0];
            if (!example) throw new Error("缺少冒烟操作样本");
            state.axis.events.push({
              ...structuredClone(example),
              ...input,
              id: `added-${state.axis.events.length}`,
              operator: null,
              direction: null,
              tile: null,
              label: null,
              complete: false,
              frameRange: { start: input.frame, end: input.frame },
            });
            return snapshot();
          }
          case "delete_event":
            state.axis.events = state.axis.events.filter(
              (item) => item.id !== inputArgs.id,
            );
            return snapshot();
          case "select_recording_segment": {
            const index = Number(inputArgs.segmentIndex);
            const segmentExists = state.monitor.recordingSegments.some(
              (segment) => segment.index === index,
            );
            if (!segmentExists) {
              throw new Error("未知冒烟区段");
            }
            const revision = state.session.revisions.find(
              (item) => item.id === state.session.currentRevisionId,
            );
            if (!revision?.recordingMerge) {
              throw new Error("缺少冒烟录屏来源");
            }
            revision.recordingMerge.segmentIndex = index;
            state.axis.title = `冒烟区段 ${index + 1}`;
            return snapshot();
          }
          case "preview_recording_merge":
            return {
              candidateCount: 1,
              skippedBeforeAnchor: 0,
              conflicts: [
                {
                  candidateId: "candidate-smoke",
                  existingEventId: "event-deploy",
                  alignedFrame: 30,
                  kind: "deploy",
                  tile: "C5",
                },
              ],
            };
          case "create_recording_merge_revision":
            state.axis.title = "冒烟合并结果";
            return snapshot();
          case "plugin:dialog|save":
            return "smoke-output.axis.json";
          case "export_axis":
            return snapshot();
          case "emergency_stop":
            state.proxy.enabled = false;
            state.proxy.status = "disabled";
            return snapshot();
          default: {
            const message = `未定义的冒烟 IPC：${command}`;
            errors.push(message);
            throw new Error(message);
          }
        }
      }
      Object.defineProperty(window, "__TAURI_INTERNALS__", {
        value: {
          invoke,
          metadata: {
            currentWindow: { label: "main" },
            currentWebview: { label: "main" },
          },
          transformCallback(callback: (payload: unknown) => void) {
            const id = ++callbackId;
            callbacks.set(id, callback);
            return id;
          },
          unregisterCallback(id: number) {
            callbacks.delete(id);
          },
        },
      });
      Object.defineProperty(window, "__TAURI_EVENT_PLUGIN_INTERNALS__", {
        value: {
          unregisterListener() {
            // 测试不派发原生事件；回调注册表由当前 mock 管理。
          },
        },
      });
    },
    { initial: createSnapshot(), catalog: stages },
  );
}
