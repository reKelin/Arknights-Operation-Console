mod axis;
mod bindings;
mod runner;

use std::{path::PathBuf, sync::Mutex, time::Instant};

use axis::{DraftAxis, DraftKind};
use bindings::{
    AddEventInput, AxisMetadataInput, CommandError, RunStrategy, RunnerSnapshot,
    RunnerSnapshotEvent, UpdateEventInput,
};
use runner::RunnerState;
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

fn locked<'a>(
    state: &'a tauri::State<'a, SharedRunner>,
) -> Result<std::sync::MutexGuard<'a, RunnerState>, CommandError> {
    state
        .0
        .lock()
        .map_err(|_| CommandError::new("state_poisoned", "Runner 状态不可用"))
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
fn continue_simulation(
    state: tauri::State<'_, SharedRunner>,
) -> Result<RunnerSnapshot, CommandError> {
    let mut runner = locked(&state)?;
    runner.continue_simulation(Instant::now())?;
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
            continue_simulation,
            set_always_on_top,
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
fn start_mock_clock(app: AppHandle) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(16));
            let snapshot = {
                let state = app.state::<SharedRunner>();
                let Ok(mut runner) = state.0.lock() else {
                    break;
                };
                runner.tick(Instant::now());
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
            .with_shortcuts(["F1", "F2", "F3"])?
            .with_handler(|app, shortcut, event| {
                if event.state != ShortcutState::Pressed {
                    return;
                }
                let kind = if shortcut.matches(Modifiers::empty(), Code::F1) {
                    Some(DraftKind::Deploy)
                } else if shortcut.matches(Modifiers::empty(), Code::F2) {
                    Some(DraftKind::Skill)
                } else if shortcut.matches(Modifiers::empty(), Code::F3) {
                    Some(DraftKind::Retreat)
                } else {
                    None
                };
                let Some(kind) = kind else {
                    return;
                };
                let state = app.state::<SharedRunner>();
                if let Ok(mut runner) = state.0.lock() {
                    let _ = runner.record_event(kind);
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
        .manage(SharedRunner(Mutex::new(RunnerState::new(Instant::now()))))
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            setup_tray(app)?;
            setup_shortcuts(app).map_err(|error| error.to_string())?;
            start_mock_clock(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("启动 Arknights Operation Runner 失败");
}
