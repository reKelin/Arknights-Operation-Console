use std::{
    sync::{
        Arc, Mutex, RwLock,
        mpsc::{SyncSender, sync_channel},
    },
    thread,
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::GetForegroundWindow,
    },
    core::PWSTR,
};
use windows_capture::{
    capture::{CaptureControl, Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
    window::Window,
};

use crate::executor::ExecutionVision;
use crate::stage::{StageCatalog, StageMatchStatus, StageRecognition};

use super::{
    GameWindowCandidate, MonitorEvent, ObservedBattleState, VisionConfig, analyze_bgra,
    ocr::{OcrImage, StageOcrAccumulator, StageOcrRecognizer, crop_title},
};

enum OcrCommand {
    Analyze(OcrImage),
    Reset,
}

struct CaptureFlags {
    config: Arc<RwLock<VisionConfig>>,
    catalog: Arc<StageCatalog>,
    execution_vision: Arc<ExecutionVision>,
    latest: Arc<Mutex<Option<MonitorEvent>>>,
}

struct LiveFrameHandler {
    config: Arc<RwLock<VisionConfig>>,
    latest: Arc<Mutex<Option<MonitorEvent>>>,
    started: Instant,
    ocr_sender: SyncSender<OcrCommand>,
    ocr_result: Arc<Mutex<Option<StageRecognition>>>,
    last_ocr_at: Option<Instant>,
    outside_frames: u8,
    execution_vision: Arc<ExecutionVision>,
    last_execution_capture: Instant,
}

impl GraphicsCaptureApiHandler for LiveFrameHandler {
    type Flags = CaptureFlags;
    type Error = String;

    fn new(context: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let (ocr_sender, ocr_receiver) = sync_channel(1);
        let ocr_result: Arc<Mutex<Option<StageRecognition>>> = Arc::new(Mutex::new(None));
        let worker_result = Arc::clone(&ocr_result);
        let catalog = context.flags.catalog;
        thread::Builder::new()
            .name("stage-ocr".to_string())
            .spawn(
                move || match StageOcrRecognizer::new(Arc::clone(&catalog)) {
                    Ok(recognizer) => {
                        let mut accumulator = StageOcrAccumulator::new(catalog);
                        while let Ok(command) = ocr_receiver.recv() {
                            let recognition = match command {
                                OcrCommand::Analyze(image) => {
                                    match recognizer.recognize_text(image) {
                                        Ok(text) => accumulator.push(&text),
                                        Err(warning) => StageRecognition {
                                            warning: Some(warning),
                                            ..StageRecognition::default()
                                        },
                                    }
                                }
                                OcrCommand::Reset => {
                                    accumulator.reset();
                                    StageRecognition::default()
                                }
                            };
                            if let Ok(mut result) = worker_result.lock()
                                && result.as_ref().is_none_or(|current| {
                                    current.status != StageMatchStatus::Matched
                                        || recognition.status == StageMatchStatus::Matched
                                })
                            {
                                *result = Some(recognition);
                            }
                        }
                    }
                    Err(warning) => {
                        if let Ok(mut result) = worker_result.lock() {
                            *result = Some(StageRecognition {
                                warning: Some(warning),
                                ..StageRecognition::default()
                            });
                        }
                    }
                },
            )
            .map_err(|error| format!("启动关卡 OCR 线程失败：{error}"))?;
        Ok(Self {
            config: context.flags.config,
            latest: context.flags.latest,
            started: Instant::now(),
            ocr_sender,
            ocr_result,
            last_ocr_at: None,
            outside_frames: 0,
            execution_vision: context.flags.execution_vision,
            last_execution_capture: Instant::now() - Duration::from_secs(1),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame<'_>,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let mut buffer = match frame.buffer() {
            Ok(buffer) => buffer,
            Err(error) => {
                let message = format!("读取 WGC 帧失败：{error}");
                if let Ok(mut latest) = self.latest.lock() {
                    *latest = Some(MonitorEvent::Error(message.clone()));
                }
                return Err(message);
            }
        };
        let width = buffer.width();
        let height = buffer.height();
        let row_pitch = buffer.row_pitch();
        let config = self.config.read().map(|config| *config).unwrap_or_default();
        if self.last_execution_capture.elapsed() >= Duration::from_millis(100) {
            self.execution_vision
                .publish(buffer.as_raw_buffer(), width, height, row_pitch);
            self.last_execution_capture = Instant::now();
        }
        let timestamp = self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        let event = match analyze_bgra(
            buffer.as_raw_buffer(),
            width,
            height,
            row_pitch,
            timestamp,
            config,
        ) {
            Ok(mut observation) => {
                if observation.title_candidate {
                    self.outside_frames = 0;
                    let now = Instant::now();
                    if self.last_ocr_at.is_none_or(|last| {
                        now.saturating_duration_since(last) >= Duration::from_millis(200)
                    }) {
                        self.last_ocr_at = Some(now);
                        if let Ok(image) =
                            crop_title(buffer.as_raw_buffer(), width, height, row_pitch)
                        {
                            let _ = self.ocr_sender.try_send(OcrCommand::Analyze(image));
                        }
                    }
                } else if observation.battle_state == ObservedBattleState::NotInBattle {
                    self.outside_frames = self.outside_frames.saturating_add(1);
                    if self.outside_frames >= 30 {
                        if let Ok(mut result) = self.ocr_result.lock() {
                            *result = Some(StageRecognition::default());
                        }
                        self.last_ocr_at = None;
                        let _ = self.ocr_sender.try_send(OcrCommand::Reset);
                    }
                } else {
                    self.outside_frames = 0;
                }
                observation.stage_recognition = self
                    .ocr_result
                    .lock()
                    .ok()
                    .and_then(|result| result.clone());
                MonitorEvent::Observation(observation)
            }
            Err(message) => MonitorEvent::Error(message),
        };
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(event);
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(MonitorEvent::Error("游戏窗口已关闭".to_string()));
        }
        Ok(())
    }
}

pub struct LiveSession {
    control: Option<CaptureControl<LiveFrameHandler, String>>,
}

impl LiveSession {
    pub fn start(
        id: &str,
        config: Arc<RwLock<VisionConfig>>,
        catalog: Arc<StageCatalog>,
        execution_vision: Arc<ExecutionVision>,
        latest: Arc<Mutex<Option<MonitorEvent>>>,
    ) -> Result<(Self, GameWindowCandidate), String> {
        let (window, candidate) = find_window(id)?;
        let settings = Settings::new(
            window,
            CursorCaptureSettings::WithoutCursor,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Exclude,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            CaptureFlags {
                config,
                catalog,
                execution_vision,
                latest,
            },
        );
        let control = LiveFrameHandler::start_free_threaded(settings)
            .map_err(|error| format!("启动 WGC 捕获失败：{error}"))?;
        Ok((
            Self {
                control: Some(control),
            },
            candidate,
        ))
    }

    pub fn stop(mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.stop();
        }
    }
}

pub fn list_game_windows() -> Result<Vec<GameWindowCandidate>, String> {
    let windows = Window::enumerate().map_err(|error| format!("扫描游戏窗口失败：{error}"))?;
    let mut candidates = windows
        .into_iter()
        .filter_map(|window| candidate_for_window(window).ok())
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.title.cmp(&right.title));
    Ok(candidates)
}

pub fn foreground_game_window() -> Result<GameWindowCandidate, String> {
    let window = Window::foreground().map_err(|error| format!("读取前台窗口失败：{error}"))?;
    candidate_for_window(window)
}

pub fn is_foreground(id: &str) -> bool {
    usize::from_str_radix(id, 16)
        .is_ok_and(|raw| unsafe { GetForegroundWindow().0 as usize == raw })
}

fn find_window(id: &str) -> Result<(Window, GameWindowCandidate), String> {
    let raw = usize::from_str_radix(id, 16).map_err(|_| "游戏窗口 ID 无效".to_string())?;
    let window = Window::from_raw_hwnd(raw as *mut core::ffi::c_void);
    let candidate = candidate_for_window(window)?;
    Ok((window, candidate))
}

fn candidate_for_window(window: Window) -> Result<GameWindowCandidate, String> {
    if !window.is_valid() {
        return Err("不是可捕获的顶层窗口".to_string());
    }
    let title = window
        .title()
        .map_err(|error| format!("读取窗口标题失败：{error}"))?;
    let process_name = limited_process_name(&window);
    let title_matches =
        title.trim() == "明日方舟" || title.trim().eq_ignore_ascii_case("Arknights");
    if !process_name
        .as_deref()
        .is_some_and(|name| name.eq_ignore_ascii_case("Arknights.exe"))
        && !title_matches
    {
        return Err("不是可捕获的 Arknights.exe 窗口".to_string());
    }
    let width = window
        .width()
        .map_err(|error| format!("读取窗口宽度失败：{error}"))?;
    let height = window
        .height()
        .map_err(|error| format!("读取窗口高度失败：{error}"))?;
    if width < 640 || height < 360 {
        return Err("游戏窗口尺寸过小".to_string());
    }
    Ok(GameWindowCandidate {
        id: format!("{:x}", window.as_raw_hwnd() as usize),
        title,
        process_name: process_name
            .clone()
            .unwrap_or_else(|| "Arknights.exe".to_string()),
        width: width as u32,
        height: height as u32,
        warning: process_name
            .is_none()
            .then(|| "进程路径受限，已通过窗口标题确认".to_string()),
    })
}

fn limited_process_name(window: &Window) -> Option<String> {
    let process_id = window.process_id().ok()?;
    let process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let mut buffer = vec![0_u16; 32_768];
    let mut length = buffer.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    result.ok()?;
    std::path::Path::new(&String::from_utf16_lossy(&buffer[..length as usize]))
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToString::to_string)
}
