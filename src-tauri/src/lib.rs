mod axis;
mod bindings;
mod monitor;
mod runner;
mod settings;

use std::{path::PathBuf, sync::Mutex, time::Instant};

use axis::{DraftAxis, DraftKind};
use bindings::{
    AddEventInput, AxisMetadataInput, CommandError, RunStrategy, RunnerSnapshot,
    RunnerSnapshotEvent, UpdateEventInput,
};
use monitor::{GameWindowCandidate, MonitorManager, VisionConfig};
use runner::RunnerState;
use settings::AppSettings;
use specta_typescript::Typescript;
use tauri::{AppHandle, Manager};
use tauri_specta::Builder;

#[cfg(feature = "desktop-app")]
use std::{thread, time::Duration};
#[cfg(feature = "desktop-app")]
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
#[cfg(feature = "desktop-app")]
use tauri_plugin_global_shortcut::{Code, Modifiers, ShortcutState};
#[cfg(feature = "desktop-app")]
use tauri_specta::Event;

pub struct SharedRunner(Mutex<RunnerState>);
pub struct SharedMonitor(Mutex<MonitorManager>);

fn locked<'a>(
    state: &'a tauri::State<'a, SharedRunner>,
) -> Result<std::sync::MutexGuard<'a, RunnerState>, CommandError> {
    state
        .0
        .lock()
        .map_err(|_| CommandError::new("state_poisoned", "Runner 状态不可用"))
}

fn locked_monitor<'a>(
    state: &'a tauri::State<'a, SharedMonitor>,
) -> Result<std::sync::MutexGuard<'a, MonitorManager>, CommandError> {
    state
        .0
        .lock()
        .map_err(|_| CommandError::new("monitor_poisoned", "监控状态不可用"))
}

#[tauri::command]
#[specta::specta]
fn get_snapshot(state: tauri::State<'_, SharedRunner>) -> Result<RunnerSnapshot, CommandError> {
    Ok(locked(&state)?.snapshot())
}

#[tauri::command]
#[specta::specta]
fn set_recording(
    enabled: bool,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.set_recording(enabled);
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn record_event(
    kind: DraftKind,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.record_event(kind)?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn add_event(
    input: AddEventInput,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.add_event(input.frame, input.kind)?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn update_event(
    input: UpdateEventInput,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.update_event(input)?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn move_event(
    id: String,
    frame: u32,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.move_event(&id, frame)?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn delete_event(
    id: String,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.delete_event(&id)?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn set_axis_metadata(
    input: AxisMetadataInput,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.set_axis_metadata(input)?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn import_axis(
    path: String,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let text = std::fs::read_to_string(PathBuf::from(path))?;
    let value = serde_json::from_str(&text)?;
    let axis = DraftAxis::from_axis_json(value)?;
    let mut runner = locked(&state)?;
    runner.replace_axis(axis);
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn export_axis(
    path: String,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let runner = locked(&state)?;
    let value = runner.axis().to_axis_json()?;
    let text = serde_json::to_string_pretty(&value)?;
    std::fs::write(PathBuf::from(path), format!("{text}\n"))?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn set_strategy(
    strategy: RunStrategy,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.set_strategy(strategy);
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn set_always_on_top(
    enabled: bool,
    app: AppHandle,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| CommandError::new("window_error", "主窗口不存在"))?;
    window
        .set_always_on_top(enabled)
        .map_err(|error| CommandError::new("window_error", error.to_string()))?;
    let mut runner = locked(&state)?;
    runner.set_always_on_top(enabled);
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn update_settings(
    input: AppSettings,
    app: AppHandle,
    runner_state: tauri::State<'_, SharedRunner>,
    monitor_state: tauri::State<'_, SharedMonitor>,
) -> Result<RunnerSnapshot, CommandError> {
    input
        .validate()
        .map_err(|message| CommandError::field("invalid_settings", message, "settings"))?;
    let path = app
        .path()
        .app_config_dir()
        .map_err(|error| CommandError::new("settings_path", error.to_string()))?
        .join("settings.json");
    input
        .save(&path)
        .map_err(|error| CommandError::new("settings_write", error.to_string()))?;
    locked_monitor(&monitor_state)?.update_config(VisionConfig::from(&input));
    let mut runner = locked(&runner_state)?;
    runner.update_settings(input)?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn request_clear_axis(
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.request_clear_axis(Instant::now());
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn list_game_windows() -> Result<Vec<GameWindowCandidate>, CommandError> {
    MonitorManager::list_game_windows()
        .map_err(|message| CommandError::new("window_discovery", message))
}

#[tauri::command]
#[specta::specta]
fn select_game_window(
    id: String,
    monitor_state: tauri::State<'_, SharedMonitor>,
    runner_state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut monitor = locked_monitor(&monitor_state)?;
    monitor
        .select_game_window(&id)
        .map_err(|message| CommandError::field("window_selection", message, "id"))?;
    let snapshot = monitor.snapshot();
    drop(monitor);
    let mut runner = locked(&runner_state)?;
    runner.set_monitor_snapshot(snapshot);
    runner.reset_monitor_clock("等待游戏进入关卡并开始运行");
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn analyze_recording(
    path: String,
    monitor_state: tauri::State<'_, SharedMonitor>,
    runner_state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut monitor = locked_monitor(&monitor_state)?;
    monitor
        .analyze_recording(&path)
        .map_err(|message| CommandError::field("recording_analysis", message, "path"))?;
    let snapshot = monitor.snapshot();
    drop(monitor);
    let mut runner = locked(&runner_state)?;
    runner.set_monitor_snapshot(snapshot);
    runner.reset_monitor_clock("正在离线分析录屏");
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn stop_monitor(
    monitor_state: tauri::State<'_, SharedMonitor>,
    runner_state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    locked_monitor(&monitor_state)?.stop();
    let mut runner = locked(&runner_state)?;
    runner.set_monitor_snapshot(Default::default());
    runner.reset_monitor_clock("监控已停止");
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn hide_to_tray(app: AppHandle) -> Result<(), CommandError> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| CommandError::new("window_error", "主窗口不存在"))?;
    window
        .hide()
        .map_err(|error| CommandError::new("window_error", error.to_string()))
}

#[tauri::command]
#[specta::specta]
fn close_app(app: AppHandle) {
    app.exit(0);
}

pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            get_snapshot,
            set_recording,
            record_event,
            add_event,
            update_event,
            move_event,
            delete_event,
            set_axis_metadata,
            import_axis,
            export_axis,
            set_strategy,
            set_always_on_top,
            update_settings,
            request_clear_axis,
            list_game_windows,
            select_game_window,
            analyze_recording,
            stop_monitor,
            hide_to_tray,
            close_app,
        ])
        .events(tauri_specta::collect_events![RunnerSnapshotEvent])
}

pub fn export_bindings(path: PathBuf) -> Result<(), String> {
    specta_builder()
        .export(Typescript::default(), path)
        .map_err(|error| error.to_string())
}

#[cfg(feature = "desktop-app")]
fn emit_snapshot(app: &AppHandle, snapshot: RunnerSnapshot) {
    let _ = RunnerSnapshotEvent(snapshot).emit(app);
}

#[cfg(feature = "desktop-app")]
fn start_runtime(app: AppHandle) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(16));
            let (monitor_event, monitor_snapshot) = {
                let state = app.state::<SharedMonitor>();
                let Ok(mut monitor) = state.0.lock() else {
                    break;
                };
                let event = monitor.poll();
                (event, monitor.snapshot())
            };
            let snapshot = {
                let state = app.state::<SharedRunner>();
                let Ok(mut runner) = state.0.lock() else {
                    break;
                };
                runner.set_monitor_snapshot(monitor_snapshot);
                let now = Instant::now();
                if let Some(event) = monitor_event {
                    runner.apply_monitor_event(event, now);
                }
                runner.refresh(now);
                runner.snapshot()
            };
            if RunnerSnapshotEvent(snapshot).emit(&app).is_err() {
                break;
            }
        }
    });
}

#[cfg(feature = "desktop-app")]
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(feature = "desktop-app")]
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut builder = TrayIconBuilder::new()
        .tooltip("Arknights Operation Runner")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg(feature = "desktop-app")]
fn setup_shortcuts(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    app.handle().plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_shortcuts(["F1", "F2", "F3", "F4"])?
            .with_handler(|app, shortcut, event| {
                if event.state != ShortcutState::Pressed {
                    return;
                }
                let clear = shortcut.matches(Modifiers::empty(), Code::F4);
                let kind = if shortcut.matches(Modifiers::empty(), Code::F1) {
                    Some(DraftKind::Deploy)
                } else if shortcut.matches(Modifiers::empty(), Code::F2) {
                    Some(DraftKind::Skill)
                } else if shortcut.matches(Modifiers::empty(), Code::F3) {
                    Some(DraftKind::Retreat)
                } else {
                    None
                };
                if !clear && kind.is_none() {
                    return;
                }
                let state = app.state::<SharedRunner>();
                if let Ok(mut runner) = state.0.lock() {
                    if clear {
                        runner.request_clear_axis(Instant::now());
                    } else if let Some(kind) = kind {
                        let _ = runner.record_event(kind);
                    }
                    emit_snapshot(app, runner.snapshot());
                }
            })
            .build(),
    )?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[cfg(feature = "desktop-app")]
pub fn run() {
    let builder = specta_builder();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let (settings, warning) = AppSettings::load(&settings_path);
            app.manage(SharedRunner(Mutex::new(RunnerState::with_settings(
                Instant::now(),
                settings.clone(),
                warning,
            ))));
            app.manage(SharedMonitor(Mutex::new(MonitorManager::new(
                VisionConfig::from(&settings),
            ))));
            builder.mount_events(app);
            setup_tray(app)?;
            if let Err(error) = setup_shortcuts(app) {
                let state = app.state::<SharedRunner>();
                if let Ok(mut runner) = state.0.lock() {
                    runner
                        .set_runtime_warning(format!("全局 F1–F4 注册失败，应用仍可使用：{error}"));
                }
            }
            start_runtime(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("启动 Arknights Operation Runner 失败");
}
