mod clock;
mod ocr;
mod recording;
mod vision;

#[cfg(windows)]
mod live;

use std::sync::{Arc, Mutex, RwLock};

use crate::stage::{StageCatalog, StageRecognition};
use serde::{Deserialize, Serialize};
use specta::Type;

pub use clock::{ClockTransition, ObservationClock};
pub use vision::{VisionConfig, VisualObservation, analyze_bgra};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum MonitorSourceKind {
    #[default]
    None,
    Window,
    Recording,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum MonitorConnectionState {
    #[default]
    Idle,
    Connecting,
    Watching,
    Analyzing,
    Ready,
    Error,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ObservedBattleState {
    #[default]
    Unknown,
    NotInBattle,
    BattleBegin,
    OneXRunning,
    TwoXRunning,
    PointTwoXRunning,
    Paused,
    DeployingOperator,
    AdjustingOperatorFacing,
}

impl ObservedBattleState {
    pub fn is_in_battle(self) -> bool {
        matches!(
            self,
            Self::OneXRunning
                | Self::TwoXRunning
                | Self::PointTwoXRunning
                | Self::Paused
                | Self::DeployingOperator
                | Self::AdjustingOperatorFacing
        )
    }

    pub fn is_running(self) -> bool {
        matches!(
            self,
            Self::OneXRunning
                | Self::TwoXRunning
                | Self::PointTwoXRunning
                | Self::DeployingOperator
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameWindowCandidate {
    pub id: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSnapshot {
    pub source_kind: MonitorSourceKind,
    pub connection_state: MonitorConnectionState,
    pub source_name: Option<String>,
    pub battle_state: ObservedBattleState,
    pub confidence: u8,
    pub cost_phase: Option<u16>,
    pub cost_total: u16,
    pub trusted: bool,
    pub error: Option<String>,
    pub recording_progress: Option<u8>,
    pub trace_duration_frames: Option<u32>,
    pub trace_points: Vec<RecordingTracePoint>,
    pub recording_segments: Vec<RecordingSegment>,
    pub stage_recognition: StageRecognition,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordingTracePoint {
    pub source_frame: u32,
    pub game_frame: u32,
    pub battle_state: ObservedBattleState,
    pub cost_phase: Option<u16>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSegment {
    pub index: u32,
    pub source_start_frame: u32,
    pub source_end_frame: u32,
    pub game_duration_frames: u32,
    pub stage_recognition: StageRecognition,
}

pub enum MonitorEvent {
    Observation(VisualObservation),
    RecordingProgress {
        progress: u8,
        observation: VisualObservation,
    },
    RecordingReady {
        trace: Vec<RecordingTracePoint>,
        segments: Vec<RecordingSegment>,
        duration_frames: u32,
    },
    Error(String),
}

pub struct MonitorManager {
    config: Arc<RwLock<VisionConfig>>,
    catalog: Arc<StageCatalog>,
    latest: Arc<Mutex<Option<MonitorEvent>>>,
    snapshot: MonitorSnapshot,
    #[cfg(windows)]
    live: Option<live::LiveSession>,
    recording: Option<recording::RecordingSession>,
}

impl MonitorManager {
    pub fn new(config: VisionConfig, catalog: Arc<StageCatalog>) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            catalog,
            latest: Arc::new(Mutex::new(None)),
            snapshot: MonitorSnapshot::default(),
            #[cfg(windows)]
            live: None,
            recording: None,
        }
    }

    pub fn snapshot(&self) -> MonitorSnapshot {
        self.snapshot.clone()
    }

    pub fn update_config(&self, config: VisionConfig) {
        if let Ok(mut current) = self.config.write() {
            *current = config;
        }
    }

    pub fn poll(&mut self) -> Option<MonitorEvent> {
        let event = self.latest.lock().ok()?.take()?;
        match event {
            MonitorEvent::Observation(observation) => {
                self.snapshot.connection_state = MonitorConnectionState::Watching;
                self.snapshot.battle_state = observation.battle_state;
                self.snapshot.confidence = observation.confidence;
                self.snapshot.cost_phase = observation.cost_phase;
                self.snapshot.cost_total = observation.cost_total;
                self.snapshot.trusted = observation.confidence >= 70;
                self.snapshot.error = None;
                if let Some(recognition) = observation.stage_recognition.clone() {
                    self.snapshot.stage_recognition = recognition;
                }
                Some(MonitorEvent::Observation(observation))
            }
            MonitorEvent::RecordingProgress {
                progress,
                observation,
            } if self.snapshot.source_kind == MonitorSourceKind::Recording => {
                self.snapshot.connection_state = MonitorConnectionState::Analyzing;
                self.snapshot.recording_progress = Some(progress);
                self.snapshot.battle_state = observation.battle_state;
                self.snapshot.confidence = observation.confidence;
                self.snapshot.cost_phase = observation.cost_phase;
                self.snapshot.cost_total = observation.cost_total;
                self.snapshot.trusted = observation.confidence >= 70;
                if let Some(recognition) = observation.stage_recognition {
                    self.snapshot.stage_recognition = recognition;
                }
                None
            }
            MonitorEvent::RecordingReady {
                trace,
                segments,
                duration_frames,
            } if self.snapshot.source_kind == MonitorSourceKind::Recording => {
                if segments.is_empty() {
                    let message = "录屏中未识别到可信关卡区段".to_string();
                    self.snapshot.connection_state = MonitorConnectionState::Error;
                    self.snapshot.recording_progress = Some(100);
                    self.snapshot.trace_points = trace;
                    self.snapshot.trusted = false;
                    self.snapshot.error = Some(message.clone());
                    self.recording = None;
                    return Some(MonitorEvent::Error(message));
                }
                self.snapshot.connection_state = MonitorConnectionState::Ready;
                self.snapshot.recording_progress = Some(100);
                self.snapshot.trace_duration_frames = Some(duration_frames);
                self.snapshot.trace_points = trace;
                self.snapshot.recording_segments = segments;
                self.snapshot.trusted = true;
                self.recording = None;
                None
            }
            MonitorEvent::RecordingProgress { .. } | MonitorEvent::RecordingReady { .. } => None,
            MonitorEvent::Error(message) => {
                self.snapshot.connection_state = MonitorConnectionState::Error;
                self.snapshot.trusted = false;
                self.snapshot.error = Some(message.clone());
                Some(MonitorEvent::Error(message))
            }
        }
    }

    #[cfg(windows)]
    pub fn list_game_windows() -> Result<Vec<GameWindowCandidate>, String> {
        live::list_game_windows()
    }

    #[cfg(not(windows))]
    pub fn list_game_windows() -> Result<Vec<GameWindowCandidate>, String> {
        Err("游戏窗口监控仅支持 Windows 10/11".to_string())
    }

    #[cfg(windows)]
    pub fn select_game_window(&mut self, id: &str) -> Result<(), String> {
        self.stop();
        let (session, candidate) = live::LiveSession::start(
            id,
            Arc::clone(&self.config),
            Arc::clone(&self.catalog),
            Arc::clone(&self.latest),
        )?;
        self.live = Some(session);
        self.snapshot = MonitorSnapshot {
            source_kind: MonitorSourceKind::Window,
            connection_state: MonitorConnectionState::Connecting,
            source_name: Some(candidate.title),
            cost_total: self
                .config
                .read()
                .map(|config| config.frames_per_cost)
                .unwrap_or(30),
            ..MonitorSnapshot::default()
        };
        Ok(())
    }

    #[cfg(not(windows))]
    pub fn select_game_window(&mut self, _id: &str) -> Result<(), String> {
        Err("游戏窗口监控仅支持 Windows 10/11".to_string())
    }

    pub fn stop(&mut self) {
        #[cfg(windows)]
        if let Some(session) = self.live.take() {
            session.stop();
        }
        if let Some(session) = self.recording.take() {
            session.stop();
        }
        if let Ok(mut latest) = self.latest.lock() {
            *latest = None;
        }
        self.snapshot = MonitorSnapshot::default();
    }

    pub fn analyze_recording(&mut self, path: &str) -> Result<(), String> {
        self.stop();
        let (session, name, total) = recording::RecordingSession::start(
            path,
            *self.config.read().map_err(|_| "监控设置不可用")?,
            Arc::clone(&self.catalog),
            Arc::clone(&self.latest),
        )?;
        self.recording = Some(session);
        self.snapshot = MonitorSnapshot {
            source_kind: MonitorSourceKind::Recording,
            connection_state: MonitorConnectionState::Analyzing,
            source_name: Some(name),
            cost_total: total,
            recording_progress: Some(0),
            ..MonitorSnapshot::default()
        };
        Ok(())
    }
}

impl Drop for MonitorManager {
    fn drop(&mut self) {
        self.stop();
    }
}
