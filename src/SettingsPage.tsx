import { save } from "@tauri-apps/plugin-dialog";
import { type ReactNode, useState } from "react";
import type { TypedResult } from "./console";
import { messageOf, unwrap } from "./console";
import {
  type AppSettings,
  commands,
  type RunnerSnapshot,
} from "./generated/bindings";
import Icon from "./Icon";

type SettingsPageProps = {
  snapshot: RunnerSnapshot;
  onBack: () => void;
  onRun: (
    operation: () => Promise<TypedResult<RunnerSnapshot>>,
  ) => Promise<boolean>;
  onScanWindows: () => void;
  onSelectForeground: () => void;
  onSelectStage: () => void;
};

export default function SettingsPage({
  snapshot,
  onBack,
  onRun,
  onScanWindows,
  onSelectForeground,
  onSelectStage,
}: SettingsPageProps) {
  const [tab, setTab] = useState<
    "monitor" | "appearance" | "shortcuts" | "execution" | "diagnostics"
  >("monitor");
  const [logEnabled, setLogEnabled] = useState(false);
  const [logMessage, setLogMessage] = useState("");
  async function openDiagnostics() {
    try {
      setLogEnabled((await commands.getLogStatus()).enabled);
    } catch (error) {
      setLogMessage(messageOf(error));
    }
  }
  async function exportLogs() {
    try {
      const path = await save({
        defaultPath: "console-diagnostics.log",
        filters: [{ name: "诊断日志", extensions: ["log"] }],
      });
      if (!path) return;
      unwrap(await commands.exportLogs(path));
      setLogMessage("日志已导出");
    } catch (error) {
      setLogMessage(messageOf(error));
    }
  }
  const [framesPerCost, setFramesPerCost] = useState(
    String(snapshot.settings.framesPerCost),
  );
  const [gameUiScale, setGameUiScale] = useState(
    String(snapshot.settings.gameUiScale),
  );
  const [pauseKey, setPauseKey] = useState(
    snapshot.settings.pauseKey ?? "Escape",
  );
  const [skillKey, setSkillKey] = useState(snapshot.settings.skillKey ?? "D");
  const [retreatKey, setRetreatKey] = useState(
    snapshot.settings.retreatKey ?? "A",
  );

  function updateSettings(input: Partial<AppSettings>) {
    return onRun(() =>
      commands.updateSettings({
        ...snapshot.settings,
        pauseKey,
        skillKey,
        retreatKey,
        ...input,
      }),
    );
  }

  return (
    <section className="settings-page">
      <div className="page-heading">
        <div>
          <strong>设置</strong>
          <span>显示、监控与快捷键</span>
        </div>
        <button onClick={onBack} type="button">
          <Icon name="back" />
          返回
        </button>
      </div>
      <div className="settings-tabs" role="tablist">
        {(
          [
            ["monitor", "监控"],
            ["appearance", "外观"],
            ["shortcuts", "快捷键"],
            ["execution", "执行"],
            ["diagnostics", "诊断"],
          ] as const
        ).map(([value, label]) => (
          <button
            aria-selected={tab === value}
            key={value}
            onClick={() => {
              setTab(value);
              if (value === "diagnostics") void openDiagnostics();
            }}
            role="tab"
            type="button"
          >
            {label}
          </button>
        ))}
      </div>
      <div className="settings-content">
        {tab === "diagnostics" && (
          <>
            <SettingRow
              label="开启日志模式"
              note="记录当前会话的 INFO / DEBUG 信息；最多保留 4000 条，关闭后保留已有日志；不记录视频路径或画面"
            >
              <input
                aria-label="开启日志模式"
                type="checkbox"
                checked={logEnabled}
                onChange={async (event) => {
                  try {
                    setLogEnabled(
                      (await commands.setLogEnabled(event.target.checked))
                        .enabled,
                    );
                  } catch (error) {
                    setLogMessage(messageOf(error));
                  }
                }}
              />
            </SettingRow>
            <SettingRow label="导出日志" note="复现问题前开启日志，复现后导出">
              <button type="button" onClick={() => void exportLogs()}>
                导出日志
              </button>
            </SettingRow>
            {logMessage && <p role="status">{logMessage}</p>}
          </>
        )}

        {tab === "monitor" && (
          <>
            <SettingRow
              label="当前监控源"
              note="进入关卡后自动计时，暂停与倍速跟随游戏"
            >
              <span>{snapshot.monitor.sourceName ?? "未选择"}</span>
            </SettingRow>
            <SettingRow label="游戏窗口">
              <div className="inline-actions">
                <button onClick={onScanWindows} type="button">
                  扫描窗口
                </button>
                <button onClick={onSelectForeground} type="button">
                  使用前台窗口
                </button>
                <button
                  onClick={() => onRun(() => commands.stopMonitor())}
                  type="button"
                >
                  停止监控
                </button>
              </div>
            </SettingRow>
            <SettingRow label="关卡确认">
              <button onClick={onSelectStage} type="button">
                手动选择关卡
              </button>
            </SettingRow>
          </>
        )}
        {tab === "appearance" && (
          <>
            <SettingRow label="主题">
              <select
                aria-label="主题"
                onChange={(event) =>
                  updateSettings({
                    theme: event.target.value as AppSettings["theme"],
                  })
                }
                value={snapshot.settings.theme}
              >
                <option value="dark">深色</option>
                <option value="light">浅色</option>
              </select>
            </SettingRow>
            <SettingRow label="窗口置顶">
              <input
                checked={snapshot.alwaysOnTop}
                onChange={(event) =>
                  onRun(() => commands.setAlwaysOnTop(event.target.checked))
                }
                type="checkbox"
              />
            </SettingRow>
            <SettingRow label="费用帧分母" note="事件时间仍固定为 30 Hz">
              <input
                max="150"
                min="15"
                onBlur={() =>
                  updateSettings({ framesPerCost: Number(framesPerCost) })
                }
                onChange={(event) => setFramesPerCost(event.target.value)}
                type="number"
                value={framesPerCost}
              />
            </SettingRow>
            <SettingRow label="游戏 UI 比例">
              <input
                max="100"
                min="0"
                onBlur={() =>
                  updateSettings({ gameUiScale: Number(gameUiScale) })
                }
                onChange={(event) => setGameUiScale(event.target.value)}
                type="number"
                value={gameUiScale}
              />
            </SettingRow>
          </>
        )}
        {tab === "shortcuts" && (
          <>
            <SettingRow label="记录待分类操作" note="游戏窗口位于前台时">
              <kbd>P</kbd>
            </SettingRow>
            <SettingRow label="整理操作" note="Console 位于前台时">
              <kbd>H</kbd>
            </SettingRow>
            <SettingRow label="导出作战轴" note="Console 位于前台时">
              <kbd>Ctrl S</kbd>
            </SettingRow>
            <SettingRow label="即时接管" note="代理运行且游戏窗口位于前台时">
              <kbd>K</kbd>
            </SettingRow>
          </>
        )}
        {tab === "execution" && (
          <>
            <SettingRow label="操作提醒">
              <button
                onClick={() => onRun(() => commands.setStrategy("notify"))}
                type="button"
              >
                {snapshot.strategy === "notify" ? "已启用" : "启用"}
              </button>
            </SettingRow>
            <SettingRow
              label="暂停键"
              note="部署、技能和撤退均在暂停事务中执行"
            >
              <input
                aria-label="暂停键"
                onBlur={() => updateSettings({ pauseKey })}
                onChange={(event) => setPauseKey(event.target.value)}
                value={pauseKey}
              />
            </SettingRow>
            <SettingRow label="技能键">
              <input
                aria-label="技能键"
                onBlur={() => updateSettings({ skillKey })}
                onChange={(event) => setSkillKey(event.target.value)}
                value={skillKey}
              />
            </SettingRow>
            <SettingRow label="撤退键">
              <input
                aria-label="撤退键"
                onBlur={() => updateSettings({ retreatKey })}
                onChange={(event) => setRetreatKey(event.target.value)}
                value={retreatKey}
              />
            </SettingRow>
            <SettingRow
              label="确认游戏键位"
              note="更改任一键位后必须重新确认；确认仅解锁下一次代理武装"
            >
              <input
                aria-label="确认游戏键位"
                checked={snapshot.settings.bindingsConfirmed ?? false}
                onChange={(event) =>
                  updateSettings({ bindingsConfirmed: event.target.checked })
                }
                type="checkbox"
              />
            </SettingRow>
          </>
        )}
      </div>
    </section>
  );
}

function SettingRow({
  label,
  note,
  children,
}: {
  label: string;
  note?: string;
  children: ReactNode;
}) {
  return (
    <div className="setting-row">
      <span>
        <strong>{label}</strong>
        {note && <small>{note}</small>}
      </span>
      {children}
    </div>
  );
}
