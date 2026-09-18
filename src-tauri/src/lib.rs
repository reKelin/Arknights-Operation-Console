mod axis;
mod bindings;
mod executor;
mod monitor;
mod runner;
mod session;
mod settings;
mod stage;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use axis::DraftAxis;
use bindings::{
    AddEventInput, AxisMetadataInput, CommandError, ConfirmEventTimeInput, ConsoleMode,
    ResolveExecutionReceiptInput, RunStrategy, RunnerSnapshot, RunnerSnapshotEvent,
    UpdateEventInput,
};
use executor::{CancelDrain, ExecutionBindings, ProxyBatchKind, ProxyBatchResult, SharedExecutor};
use monitor::{
    CandidateConfirmation, GameWindowCandidate, MonitorConnectionState, MonitorManager,
    MonitorSourceKind, VisionConfig,
};
use runner::RunnerState;
use settings::AppSettings;
use specta_typescript::Typescript;
use stage::{StageCatalogEntry, StageRepository};
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
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, ShortcutState};
#[cfg(feature = "desktop-app")]
use tauri_specta::Event;

pub struct SharedRunner(Mutex<RunnerState>);
pub struct SharedMonitor(Mutex<MonitorManager>);
pub struct SharedStages(Arc<StageRepository>);

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
fn set_console_mode(
    mode: ConsoleMode,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.set_console_mode(mode);
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn record_bookmark(state: tauri::State<'_, SharedRunner>) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.record_bookmark()?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn shift_events(
    ids: Vec<String>,
    delta: i32,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.shift_events(&ids, delta)?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn reorder_event(
    id: String,
    direction: i8,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.reorder_event(&id, direction)?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn clear_bookmarks(state: tauri::State<'_, SharedRunner>) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.clear_bookmarks()?;
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
fn confirm_event_time(
    input: ConfirmEventTimeInput,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.confirm_event_time(input)?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn confirm_recording_candidate(
    input: CandidateConfirmation,
    runner_state: tauri::State<'_, SharedRunner>,
    monitor_state: tauri::State<'_, SharedMonitor>,
) -> Result<RunnerSnapshot, CommandError> {
    let candidate = {
        let monitor = locked_monitor(&monitor_state)?;
        let snapshot = monitor.snapshot();
        if snapshot.source_kind != MonitorSourceKind::Recording
            || snapshot.connection_state != MonitorConnectionState::Ready
        {
            return Err(CommandError::new(
                "recording_analysis_not_ready",
                "录屏分析尚未完成，不能确认操作候选",
            ));
        }
        snapshot
            .recording_candidates
            .into_iter()
            .find(|candidate| candidate.id == input.candidate_id)
            .ok_or_else(|| CommandError::new("candidate_not_found", "未找到录屏操作候选"))?
    };
    let mut runner = locked(&runner_state)?;
    runner.confirm_recording_candidate(&candidate, input)?;
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
    runner_state: tauri::State<'_, SharedRunner>,
    stages: tauri::State<'_, SharedStages>,
) -> Result<RunnerSnapshot, CommandError> {
    let stage_id = input.stage_id.clone();
    {
        let mut runner = locked(&runner_state)?;
        runner.set_axis_metadata(input)?;
    }
    if let Some(stage_id) = stage_id.filter(|id| stages.0.catalog().find(id).is_some()) {
        let map = stages.0.load_map(&stage_id);
        let mut runner = locked(&runner_state)?;
        match map {
            Ok(bounds) => runner.set_stage_map_bounds(stage_id, bounds),
            Err(message) => {
                runner.set_runtime_warning(format!("轴关卡已更新，但地图暂不可用：{message}"))
            }
        }
        return Ok(runner.snapshot());
    }
    let runner = locked(&runner_state)?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn import_axis(
    path: String,
    runner_state: tauri::State<'_, SharedRunner>,
    stages: tauri::State<'_, SharedStages>,
) -> Result<RunnerSnapshot, CommandError> {
    let text = std::fs::read_to_string(PathBuf::from(path))?;
    let value = serde_json::from_str(&text)?;
    let axis = DraftAxis::from_axis_json(value)?;
    let stage_id = axis.stage_id.clone();
    {
        let mut runner = locked(&runner_state)?;
        runner.replace_axis(axis)?;
    }
    if let Some(stage_id) = stage_id.filter(|id| stages.0.catalog().find(id).is_some()) {
        let map = stages.0.load_map(&stage_id);
        let mut runner = locked(&runner_state)?;
        match map {
            Ok(bounds) => runner.set_stage_map_bounds(stage_id, bounds),
            Err(message) => {
                runner.set_runtime_warning(format!("轴已导入，但地图暂不可用：{message}"))
            }
        }
        return Ok(runner.snapshot());
    }
    let runner = locked(&runner_state)?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn export_axis(
    path: String,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let runner = locked(&state)?;
    let value = runner.axis_json_for_export()?;
    let text = serde_json::to_string_pretty(&value)?;
    std::fs::write(PathBuf::from(path), format!("{text}\n"))?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn export_axis_revision(
    path: String,
    revision_id: String,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let runner = locked(&state)?;
    let value = runner.axis_revision_json_for_export(&revision_id)?;
    let text = serde_json::to_string_pretty(&value)?;
    std::fs::write(PathBuf::from(path), format!("{text}\n"))?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn select_axis_revision(
    revision_id: String,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.select_axis_revision(&revision_id)?;
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
fn request_proxy_execution(
    runner_state: tauri::State<'_, SharedRunner>,
    executor: tauri::State<'_, SharedExecutor>,
) -> Result<RunnerSnapshot, CommandError> {
    let (enabled, axis, settings) = {
        let mut runner = locked(&runner_state)?;
        let enabled = runner.request_proxy_execution(Instant::now())?;
        (enabled, runner.axis().clone(), runner.snapshot().settings)
    };
    if enabled {
        executor
            .configure_bindings(ExecutionBindings::from(&settings))
            .map_err(|message| CommandError::new("invalid_bindings", message))?;
        if let Err(message) = executor.prepare(&axis) {
            executor.emergency_stop();
            let mut runner = locked(&runner_state)?;
            runner.finish_proxy_execution(Vec::new(), Some(message));
            return Ok(runner.snapshot());
        }
        locked(&runner_state)?.complete_proxy_enable()?;
        executor.set_capture_enabled(true);
    } else {
        executor.emergency_stop();
    }
    Ok(locked(&runner_state)?.snapshot())
}

#[tauri::command]
#[specta::specta]
fn emergency_stop(
    runner_state: tauri::State<'_, SharedRunner>,
    executor: tauri::State<'_, SharedExecutor>,
) -> Result<RunnerSnapshot, CommandError> {
    executor.emergency_stop();
    let mut runner = locked(&runner_state)?;
    runner.emergency_stop();
    Ok(runner.snapshot())
}

fn apply_executor_result(runner: &mut RunnerState, result: ProxyBatchResult) {
    match result.kind {
        ProxyBatchKind::Action => {
            runner.finish_proxy_execution(result.receipts, result.error);
        }
        ProxyBatchKind::PauseToggle => runner.finish_pause_toggle(result.error),
        ProxyBatchKind::TakeoverPause => {
            if let Some(message) = result.error {
                runner.fail_pending_takeover(format!("接管暂停无法确认：{message}"));
            } else if let Err(error) = runner.finalize_takeover_revision() {
                runner.fail_pending_takeover(format!("接管版本创建失败：{}", error.message));
            }
        }
    }
}

fn submit_takeover_pause(
    runner: &mut RunnerState,
    executor: &SharedExecutor,
    monitor: &monitor::MonitorSnapshot,
) {
    runner.mark_takeover_awaiting_pause();
    if let Err(message) =
        executor.submit_takeover_pause(monitor.window_id.as_deref().unwrap_or_default(), monitor)
    {
        runner.fail_pending_takeover(format!("接管暂停无法确认：{message}"));
    }
}

fn begin_takeover(runner: &mut RunnerState, executor: &SharedExecutor) -> Result<(), CommandError> {
    runner.request_takeover_revision()?;
    match executor.cancel_and_drain() {
        CancelDrain::Pending => {}
        CancelDrain::Complete(result) => {
            if let Some(result) = result {
                apply_executor_result(runner, result);
            }
            if runner.takeover_is_cancelling() {
                let monitor = runner.snapshot().monitor;
                submit_takeover_pause(runner, executor, &monitor);
            }
        }
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn takeover_now(
    runner_state: tauri::State<'_, SharedRunner>,
    executor: tauri::State<'_, SharedExecutor>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&runner_state)?;
    if !runner.takeover_available() {
        return Err(CommandError::new(
            "takeover_not_available",
            "只有正在执行的本局代理可以接管",
        ));
    }
    begin_takeover(&mut runner, &executor)?;
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn resolve_execution_receipt(
    input: ResolveExecutionReceiptInput,
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.resolve_execution_receipt(input.receipt_sequence, input.confirmed)?;
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
    executor: tauri::State<'_, SharedExecutor>,
) -> Result<RunnerSnapshot, CommandError> {
    input
        .validate()
        .map_err(|message| CommandError::field("invalid_settings", message, "settings"))?;
    let input = {
        let mut runner = locked(&runner_state)?;
        runner.update_settings(input)?;
        runner.snapshot().settings
    };
    executor
        .configure_bindings(ExecutionBindings::from(&input))
        .map_err(|message| CommandError::new("invalid_bindings", message))?;
    let path = app
        .path()
        .app_config_dir()
        .map_err(|error| CommandError::new("settings_path", error.to_string()))?
        .join("settings.json");
    input
        .save(&path)
        .map_err(|error| CommandError::new("settings_write", error.to_string()))?;
    locked_monitor(&monitor_state)?.update_config(VisionConfig::from(&input));
    Ok(locked(&runner_state)?.snapshot())
}

#[tauri::command]
#[specta::specta]
fn request_clear_axis(
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.request_clear_axis(Instant::now())?;
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
fn foreground_game_window() -> Result<GameWindowCandidate, CommandError> {
    MonitorManager::foreground_game_window()
        .map_err(|message| CommandError::new("window_discovery", message))
}

#[tauri::command]
#[specta::specta]
fn list_stages(
    query: String,
    state: tauri::State<'_, SharedStages>,
) -> Result<Vec<StageCatalogEntry>, CommandError> {
    Ok(state.0.catalog().search(&query, 100))
}

#[tauri::command]
#[specta::specta]
fn set_manual_stage(
    stage_id: String,
    stages: tauri::State<'_, SharedStages>,
    runner_state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let stage = stages.0.catalog().find(&stage_id).cloned().ok_or_else(|| {
        CommandError::field(
            "stage_not_found",
            format!("关卡目录中没有 {stage_id}"),
            "stageId",
        )
    })?;
    {
        let mut runner = locked(&runner_state)?;
        runner.set_manual_stage(stage)?;
    }
    let map = stages.0.load_map(&stage_id);
    let mut runner = locked(&runner_state)?;
    match map {
        Ok(bounds) => runner.set_stage_map_bounds(stage_id, bounds),
        Err(message) => {
            runner.set_runtime_warning(format!("已选择关卡，但地图暂不可用：{message}"))
        }
    }
    Ok(runner.snapshot())
}

#[tauri::command]
#[specta::specta]
fn select_game_window(
    id: String,
    monitor_state: tauri::State<'_, SharedMonitor>,
    runner_state: tauri::State<'_, SharedRunner>,
    executor: tauri::State<'_, SharedExecutor>,
) -> Result<RunnerSnapshot, CommandError> {
    executor.emergency_stop();
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
    executor: tauri::State<'_, SharedExecutor>,
) -> Result<RunnerSnapshot, CommandError> {
    executor.emergency_stop();
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
    executor: tauri::State<'_, SharedExecutor>,
) -> Result<RunnerSnapshot, CommandError> {
    executor.emergency_stop();
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
            set_console_mode,
            record_bookmark,
            shift_events,
            reorder_event,
            clear_bookmarks,
            add_event,
            update_event,
            move_event,
            confirm_event_time,
            confirm_recording_candidate,
            delete_event,
            set_axis_metadata,
            import_axis,
            export_axis,
            export_axis_revision,
            select_axis_revision,
            set_strategy,
            request_proxy_execution,
            emergency_stop,
            takeover_now,
            resolve_execution_receipt,
            set_always_on_top,
            update_settings,
            request_clear_axis,
            list_game_windows,
            foreground_game_window,
            list_stages,
            set_manual_stage,
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
        let mut record_shortcut_registered = false;
        let mut takeover_shortcut_registered = false;
        let mut executor_active = false;
        let mut shortcut_check = Instant::now() - Duration::from_secs(1);
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
            let (mut snapshot, pause_toggle, execution) = {
                let state = app.state::<SharedRunner>();
                let Ok(mut runner) = state.0.lock() else {
                    break;
                };
                runner.set_monitor_snapshot(monitor_snapshot.clone());
                let now = Instant::now();
                if let Some(event) = monitor_event {
                    runner.apply_monitor_event(event, now);
                }
                runner.refresh(now);
                if let Some(result) = app.state::<SharedExecutor>().take_result() {
                    apply_executor_result(&mut runner, result);
                    if runner.takeover_is_cancelling() {
                        submit_takeover_pause(
                            &mut runner,
                            &app.state::<SharedExecutor>(),
                            &monitor_snapshot,
                        );
                    }
                }
                let pause_toggle = runner.take_pending_pause_toggle();
                let requests = runner.take_pending_execution();
                let execution = (!requests.is_empty()).then(|| {
                    runner.begin_proxy_execution();
                    (
                        requests,
                        runner.snapshot().proxy.run_id.unwrap_or_default(),
                        runner.snapshot().frame,
                        runner.axis().stage_id.clone().unwrap_or_default(),
                        monitor_snapshot.window_id.clone().unwrap_or_default(),
                        monitor_snapshot.clone(),
                    )
                });
                (runner.snapshot(), pause_toggle, execution)
            };
            app.state::<SharedExecutor>().update_safety(
                &snapshot.monitor,
                snapshot.clock.quality,
                snapshot.clock.active,
                snapshot.stage_safety.status == stage::StageSafetyStatus::Matched,
                snapshot.proxy.enabled,
            );
            if pause_toggle {
                let result = app.state::<SharedExecutor>().submit_pause_toggle(
                    monitor_snapshot.window_id.as_deref().unwrap_or_default(),
                    &monitor_snapshot,
                );
                if let Err(message) = result {
                    let state = app.state::<SharedRunner>();
                    let Ok(mut runner) = state.0.lock() else {
                        break;
                    };
                    runner.finish_pause_toggle(Some(message));
                    snapshot = runner.snapshot();
                }
            } else if let Some((requests, run_id, frame, stage_id, window_id, monitor)) = execution
            {
                let result = app
                    .state::<SharedExecutor>()
                    .submit(&requests, &run_id, frame, &stage_id, &window_id, &monitor);
                if let Err(message) = result {
                    let state = app.state::<SharedRunner>();
                    let Ok(mut runner) = state.0.lock() else {
                        break;
                    };
                    runner.finish_proxy_execution(Vec::new(), Some(message));
                    snapshot = runner.snapshot();
                }
            }
            let takeover_settling = matches!(
                snapshot.session.takeover.status,
                session::TakeoverStatus::Cancelling | session::TakeoverStatus::AwaitingPauseProof
            );
            if !snapshot.proxy.enabled && !takeover_settling {
                if executor_active {
                    app.state::<SharedExecutor>().emergency_stop();
                    executor_active = false;
                }
            } else if snapshot.proxy.enabled {
                executor_active = true;
                // 捕获由代理生命周期控制，并为下一次事务持续保留新鲜画面。
                app.state::<SharedExecutor>().set_capture_enabled(true);
            } else {
                executor_active = true;
            }
            if shortcut_check.elapsed() >= Duration::from_millis(250) {
                shortcut_check = Instant::now();
                let game_is_foreground = snapshot
                    .monitor
                    .window_id
                    .as_deref()
                    .is_some_and(MonitorManager::is_game_foreground);
                let should_record = game_is_foreground
                    && snapshot.console_mode == ConsoleMode::ManualRecording
                    && !matches!(
                        snapshot.session.takeover.status,
                        session::TakeoverStatus::Cancelling
                            | session::TakeoverStatus::AwaitingPauseProof
                    );
                if should_record != record_shortcut_registered {
                    let result = if should_record {
                        app.global_shortcut().register("P")
                    } else {
                        app.global_shortcut().unregister("P")
                    };
                    record_shortcut_registered = should_record && result.is_ok();
                }
                let should_takeover = game_is_foreground
                    && snapshot.console_mode == ConsoleMode::Proxy
                    && snapshot.proxy.enabled
                    && snapshot.proxy.run_id.is_some();
                if should_takeover != takeover_shortcut_registered {
                    let result = if should_takeover {
                        app.global_shortcut().register("K")
                    } else {
                        app.global_shortcut().unregister("K")
                    };
                    takeover_shortcut_registered = should_takeover && result.is_ok();
                }
            }
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
        .tooltip("Arknights Operation Console")
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
            .with_handler(|app, shortcut, event| {
                if event.state != ShortcutState::Pressed {
                    return;
                }
                if shortcut.matches(Modifiers::empty(), Code::KeyP) {
                    let state = app.state::<SharedRunner>();
                    if let Ok(mut runner) = state.0.lock() {
                        let _ = runner.record_bookmark();
                        emit_snapshot(app, runner.snapshot());
                    }
                } else if shortcut.matches(Modifiers::empty(), Code::KeyK) {
                    let state = app.state::<SharedRunner>();
                    if let Ok(mut runner) = state.0.lock() {
                        if !runner.takeover_available() {
                            return;
                        }
                        let _ = begin_takeover(&mut runner, &app.state::<SharedExecutor>());
                        emit_snapshot(app, runner.snapshot());
                    }
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
            let stage_cache = app.path().app_cache_dir()?.join("stage-maps");
            let execution_cache = app.path().app_cache_dir()?.join("execution");
            let catalog = Arc::new(stage::StageCatalog::embedded().map_err(std::io::Error::other)?);
            let (settings, warning) = AppSettings::load(&settings_path);
            let executor = SharedExecutor::new(execution_cache, Arc::clone(&catalog))
                .map_err(std::io::Error::other)?;
            executor
                .configure_bindings(ExecutionBindings::from(&settings))
                .map_err(std::io::Error::other)?;
            let execution_vision = executor.vision();
            app.manage(SharedRunner(Mutex::new(RunnerState::with_settings(
                Instant::now(),
                settings.clone(),
                warning,
            ))));
            app.manage(SharedMonitor(Mutex::new(MonitorManager::new(
                VisionConfig::from(&settings),
                Arc::clone(&catalog),
                execution_vision,
            ))));
            app.manage(executor);
            app.manage(SharedStages(Arc::new(StageRepository::new(
                catalog,
                stage_cache,
            ))));
            builder.mount_events(app);
            setup_tray(app)?;
            if let Err(error) = setup_shortcuts(app) {
                let state = app.state::<SharedRunner>();
                if let Ok(mut runner) = state.0.lock() {
                    runner.set_runtime_warning(format!(
                        "全局 P 记录或 K 接管快捷键初始化失败，应用仍可使用：{error}"
                    ));
                }
            }
            start_runtime(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("启动 Arknights Operation Console 失败");
}
