use std::{
    sync::{
        Arc, Mutex, RwLock,
        mpsc::{SyncSender, sync_channel},
    },
    thread,
    time::{Duration, Instant},
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

use crate::stage::{StageCatalog, StageMatchStatus, StageRecognition};

use super::{
    GameWindowCandidate, MonitorEvent, ObservedBattleState, VisionConfig, analyze_bgra,
    ocr::{OcrImage, StageOcrRecognizer, crop_title},
};

struct CaptureFlags {
    config: Arc<RwLock<VisionConfig>>,
    catalog: Arc<StageCatalog>,
    latest: Arc<Mutex<Option<MonitorEvent>>>,
}

struct LiveFrameHandler {
    config: Arc<RwLock<VisionConfig>>,
    latest: Arc<Mutex<Option<MonitorEvent>>>,
    started: Instant,
    ocr_sender: SyncSender<OcrImage>,
    ocr_result: Arc<Mutex<Option<StageRecognition>>>,
    last_ocr_at: Option<Instant>,
    outside_frames: u8,
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
            .spawn(move || match StageOcrRecognizer::new(catalog) {
                Ok(recognizer) => {
                    while let Ok(image) = ocr_receiver.recv() {
                        let recognition = recognizer.recognize(image);
                        if let Ok(mut result) = worker_result.lock() {
                            if result.as_ref().is_none_or(|current| {
                                current.status != StageMatchStatus::Matched
                                    || recognition.status == StageMatchStatus::Matched
                            }) {
                                *result = Some(recognition);
                            }
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
            })
            .map_err(|error| format!("启动关卡 OCR 线程失败：{error}"))?;
        Ok(Self {
            config: context.flags.config,
            latest: context.flags.latest,
            started: Instant::now(),
            ocr_sender,
            ocr_result,
            last_ocr_at: None,
            outside_frames: 0,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame<'_>,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let mut buffer = match frame.buffer_without_title_bar() {
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
                if observation.battle_state == ObservedBattleState::BattleBegin {
                    self.outside_frames = 0;
                    let now = Instant::now();
                    if self.last_ocr_at.is_none_or(|last| {
                        now.saturating_duration_since(last) >= Duration::from_millis(500)
                    }) {
                        self.last_ocr_at = Some(now);
                        if let Ok(image) =
                            crop_title(buffer.as_raw_buffer(), width, height, row_pitch)
                        {
                            let _ = self.ocr_sender.try_send(image);
                        }
                    }
                } else if observation.battle_state == ObservedBattleState::NotInBattle {
                    self.outside_frames = self.outside_frames.saturating_add(1);
                    if self.outside_frames >= 30 {
                        if let Ok(mut result) = self.ocr_result.lock() {
                            *result = Some(StageRecognition::default());
                        }
                        self.last_ocr_at = None;
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

fn find_window(id: &str) -> Result<(Window, GameWindowCandidate), String> {
    let windows = Window::enumerate().map_err(|error| format!("扫描游戏窗口失败：{error}"))?;
    windows
        .into_iter()
        .filter_map(|window| {
            let candidate = candidate_for_window(window).ok()?;
            (candidate.id == id).then_some((window, candidate))
        })
        .next()
        .ok_or_else(|| "所选 Arknights.exe 窗口已失效，请重新扫描".to_string())
}

fn candidate_for_window(window: Window) -> Result<GameWindowCandidate, String> {
    let process_name = window
        .process_name()
        .map_err(|error| format!("读取窗口进程失败：{error}"))?;
    if !process_name.eq_ignore_ascii_case("Arknights.exe") || !window.is_valid() {
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
        title: window
            .title()
            .map_err(|error| format!("读取窗口标题失败：{error}"))?,
        width: width as u32,
        height: height as u32,
    })
}
