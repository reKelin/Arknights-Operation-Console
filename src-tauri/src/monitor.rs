mod clock;
mod ocr;
mod recording;
mod vision;

#[cfg(windows)]
mod live;

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

use crate::executor::ExecutionVision;
use crate::stage::{StageCatalog, StageRecognition};
use serde::{Deserialize, Serialize};
use specta::Type;

pub use clock::{
    ClockAnchor, ClockMode, ClockQuality, ClockSnapshot, ClockTransition, ClockUpdate, HumanClock,
    ObservationClock, ProxyClock,
};
pub use recording::analysis::AnalysisCandidate;
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
    pub process_name: String,
    pub width: u32,
    pub height: u32,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSnapshot {
    pub source_kind: MonitorSourceKind,
    pub connection_state: MonitorConnectionState,
    pub source_name: Option<String>,
    pub window_id: Option<String>,
    pub battle_state: ObservedBattleState,
    pub confidence: u8,
    pub cost_phase: Option<u16>,
    pub cost_total: u16,
    pub cost_full: bool,
    pub trusted: bool,
    pub error: Option<String>,
    pub capture_warning: Option<String>,
    pub last_event_sequence: Option<f64>,
    pub last_source_timestamp_ns: Option<f64>,
    pub dropped_observations: u32,
    pub recording_progress: Option<u8>,
    pub trace_duration_frames: Option<u32>,
    pub trace_points: Vec<RecordingTracePoint>,
    pub recording_segments: Vec<RecordingSegment>,
    pub recording_candidates: Vec<AnalysisCandidate>,
    pub stage_recognition: StageRecognition,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordingTracePoint {
    pub source_frame: u32,
    pub source_timestamp_ns: f64,
    pub game_frame: u32,
    pub game_frame_min: u32,
    pub game_frame_max: u32,
    pub clock_quality: ClockQuality,
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

#[derive(Debug)]
pub enum MonitorEvent {
    Observation(VisualObservation),
    RecordingProgress {
        progress: u8,
        observation: VisualObservation,
    },
    RecordingReady {
        trace: Vec<RecordingTracePoint>,
        segments: Vec<RecordingSegment>,
        candidates: Vec<AnalysisCandidate>,
        duration_frames: u32,
    },
    Error(String),
}

impl MonitorEvent {
    fn source_timestamp_ns(&self) -> Option<u64> {
        match self {
            Self::Observation(observation) | Self::RecordingProgress { observation, .. } => {
                Some(observation.capture_timestamp_ns)
            }
            Self::RecordingReady { .. } | Self::Error(_) => None,
        }
    }
}

#[derive(Debug)]
pub struct MonitorEventEnvelope {
    pub sequence: u64,
    pub source_timestamp_ns: Option<u64>,
    pub dropped_before: u32,
    pub event: MonitorEvent,
}

const MONITOR_EVENT_QUEUE_CAPACITY: usize = 64;

pub(crate) struct MonitorEventQueue {
    events: VecDeque<MonitorEventEnvelope>,
    next_sequence: u64,
    dropped_since_poll: u32,
}

impl Default for MonitorEventQueue {
    fn default() -> Self {
        Self {
            events: VecDeque::with_capacity(MONITOR_EVENT_QUEUE_CAPACITY),
            next_sequence: 1,
            dropped_since_poll: 0,
        }
    }
}

impl MonitorEventQueue {
    pub(crate) fn publish(&mut self, event: MonitorEvent) {
        if self.events.len() == MONITOR_EVENT_QUEUE_CAPACITY {
            self.events.pop_front();
            self.dropped_since_poll = self.dropped_since_poll.saturating_add(1);
        }
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.events.push_back(MonitorEventEnvelope {
            sequence,
            source_timestamp_ns: event.source_timestamp_ns(),
            dropped_before: 0,
            event,
        });
    }

    fn pop(&mut self) -> Option<MonitorEventEnvelope> {
        let mut envelope = self.events.pop_front()?;
        envelope.dropped_before = std::mem::take(&mut self.dropped_since_poll);
        Some(envelope)
    }

    fn clear(&mut self) {
        self.events.clear();
        self.dropped_since_poll = 0;
    }
}

pub struct MonitorManager {
    config: Arc<RwLock<VisionConfig>>,
    catalog: Arc<StageCatalog>,
    execution_vision: Arc<ExecutionVision>,
    events: Arc<Mutex<MonitorEventQueue>>,
    snapshot: MonitorSnapshot,
    #[cfg(windows)]
    live: Option<live::LiveSession>,
    recording: Option<recording::RecordingSession>,
    connection_deadline: Option<Instant>,
}

impl MonitorManager {
    pub fn new(
        config: VisionConfig,
        catalog: Arc<StageCatalog>,
        execution_vision: Arc<ExecutionVision>,
    ) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            catalog,
            execution_vision,
            events: Arc::new(Mutex::new(MonitorEventQueue::default())),
            snapshot: MonitorSnapshot::default(),
            #[cfg(windows)]
            live: None,
            recording: None,
            connection_deadline: None,
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

    pub fn poll(&mut self) -> Option<MonitorEventEnvelope> {
        let envelope = self.events.lock().ok()?.pop();
        if envelope.is_none()
            && self
                .connection_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.connection_deadline = None;
            let message = "WGC 已启动，但两秒内没有收到首帧".to_string();
            self.snapshot.connection_state = MonitorConnectionState::Error;
            self.snapshot.error = Some(message.clone());
            let mut events = self.events.lock().ok()?;
            events.publish(MonitorEvent::Error(message));
            return events.pop();
        }
        let mut envelope = envelope?;
        self.snapshot.last_event_sequence = Some(envelope.sequence as f64);
        self.snapshot.last_source_timestamp_ns = envelope
            .source_timestamp_ns
            .map(|timestamp| timestamp as f64);
        if envelope.dropped_before > 0 {
            self.snapshot.dropped_observations = self
                .snapshot
                .dropped_observations
                .saturating_add(envelope.dropped_before);
            self.snapshot.trusted = false;
            self.snapshot.error = Some(format!(
                "监控处理落后，丢失 {} 条观测；等待时间锚点恢复",
                envelope.dropped_before
            ));
        }
        let forwarded = match envelope.event {
            MonitorEvent::Observation(observation) => {
                self.snapshot.connection_state = MonitorConnectionState::Watching;
                self.connection_deadline = None;
                self.snapshot.battle_state = observation.battle_state;
                self.snapshot.confidence = observation.confidence;
                self.snapshot.cost_phase = observation.cost_phase;
                self.snapshot.cost_total = observation.cost_total;
                self.snapshot.cost_full = observation.cost_full;
                self.snapshot.trusted =
                    envelope.dropped_before == 0 && observation.confidence >= 70;
                if envelope.dropped_before == 0 {
                    self.snapshot.error = None;
                }
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
                self.snapshot.cost_full = observation.cost_full;
                self.snapshot.trusted = observation.confidence >= 70;
                if let Some(recognition) = observation.stage_recognition {
                    self.snapshot.stage_recognition = recognition;
                }
                None
            }
            MonitorEvent::RecordingReady {
                trace,
                segments,
                candidates,
                duration_frames,
            } if self.snapshot.source_kind == MonitorSourceKind::Recording => {
                if segments.is_empty() {
                    let message = "录屏中未识别到可信关卡区段".to_string();
                    self.snapshot.connection_state = MonitorConnectionState::Error;
                    self.snapshot.recording_progress = Some(100);
                    self.snapshot.trace_points = trace;
                    self.snapshot.recording_candidates = candidates;
                    self.snapshot.trusted = false;
                    self.snapshot.error = Some(message.clone());
                    self.recording = None;
                    envelope.event = MonitorEvent::Error(message);
                    return Some(envelope);
                }
                self.snapshot.connection_state = MonitorConnectionState::Ready;
                self.snapshot.recording_progress = Some(100);
                self.snapshot.trace_duration_frames = Some(duration_frames);
                self.snapshot.trace_points = trace;
                self.snapshot.recording_segments = segments;
                self.snapshot.recording_candidates = candidates;
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
        };
        envelope.event = forwarded?;
        Some(envelope)
    }

    #[cfg(windows)]
    pub fn list_game_windows() -> Result<Vec<GameWindowCandidate>, String> {
        live::list_game_windows()
    }

    #[cfg(windows)]
    pub fn foreground_game_window() -> Result<GameWindowCandidate, String> {
        live::foreground_game_window()
    }

    #[cfg(windows)]
    pub fn is_game_foreground(id: &str) -> bool {
        live::is_foreground(id)
    }

    #[cfg(not(windows))]
    pub fn list_game_windows() -> Result<Vec<GameWindowCandidate>, String> {
        Err("游戏窗口监控仅支持 Windows 10/11".to_string())
    }

    #[cfg(not(windows))]
    pub fn foreground_game_window() -> Result<GameWindowCandidate, String> {
        Err("游戏窗口监控仅支持 Windows 10/11".to_string())
    }

    #[cfg(not(windows))]
    pub fn is_game_foreground(_id: &str) -> bool {
        false
    }

    #[cfg(windows)]
    pub fn select_game_window(&mut self, id: &str) -> Result<(), String> {
        self.stop();
        let (session, candidate, capture_warning) = live::LiveSession::start(
            id,
            Arc::clone(&self.config),
            Arc::clone(&self.catalog),
            Arc::clone(&self.execution_vision),
            Arc::clone(&self.events),
        )?;
        self.live = Some(session);
        self.connection_deadline = Some(Instant::now() + Duration::from_secs(2));
        self.snapshot = MonitorSnapshot {
            source_kind: MonitorSourceKind::Window,
            connection_state: MonitorConnectionState::Connecting,
            source_name: Some(candidate.title),
            window_id: Some(candidate.id),
            capture_warning,
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
        if let Ok(mut events) = self.events.lock() {
            events.clear();
        }
        self.snapshot = MonitorSnapshot::default();
        self.connection_deadline = None;
    }

    pub fn analyze_recording(&mut self, path: &str) -> Result<(), String> {
        self.stop();
        let (session, name, total) = recording::RecordingSession::start(
            path,
            *self.config.read().map_err(|_| "监控设置不可用")?,
            Arc::clone(&self.catalog),
            Arc::clone(&self.events),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_reports_exact_overflow_before_next_event() {
        let mut queue = MonitorEventQueue::default();
        for index in 0..MONITOR_EVENT_QUEUE_CAPACITY + 2 {
            queue.publish(MonitorEvent::Error(index.to_string()));
        }
        let envelope = queue.pop().unwrap();
        assert_eq!(envelope.sequence, 3);
        assert_eq!(envelope.dropped_before, 2);
        assert_eq!(queue.pop().unwrap().dropped_before, 0);
    }
}
