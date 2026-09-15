use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use crate::{
    axis::{DraftAxis, DraftEvent, DraftKind, valid_tile_code},
    bindings::{
        AxisMetadataInput, BattleStatus, CommandError, NoticeKind, RunNotice, RunStrategy,
        RunnerSnapshot, UpdateEventInput,
    },
    monitor::{
        ClockTransition, MonitorEvent, MonitorSnapshot, MonitorSourceKind, ObservationClock,
        ObservedBattleState,
    },
    settings::AppSettings,
    stage::{
        StageCatalogEntry, StageIdentitySource, StageMapBounds, StageMatchStatus,
        StageSafetySnapshot, StageSafetyStatus, validate_tile_in_map,
    },
};

const NOTIFY_LEAD_FRAMES: u32 = 90;
const CLEAR_CONFIRM_DURATION: Duration = Duration::from_secs(3);
const OBSERVATION_TIMEOUT: Duration = Duration::from_millis(250);

pub struct RunnerState {
    axis: DraftAxis,
    settings: AppSettings,
    monitor: MonitorSnapshot,
    clock: ObservationClock,
    frame: u32,
    status: BattleStatus,
    speed: u8,
    recording: bool,
    strategy: RunStrategy,
    triggered: HashSet<String>,
    next_id: u64,
    next_order: u32,
    next_notice: u32,
    notices: Vec<RunNotice>,
    last_message: Option<String>,
    always_on_top: bool,
    clear_pending_deadline: Option<Instant>,
    last_observation_at: Option<Instant>,
    error_frames: u16,
    manual_stage: Option<StageCatalogEntry>,
    stage_bounds: Option<(String, StageMapBounds)>,
}

impl RunnerState {
    #[cfg(test)]
    pub fn new(now: Instant) -> Self {
        Self::with_settings(now, AppSettings::default(), None)
    }

    pub fn with_settings(
        _now: Instant,
        settings: AppSettings,
        settings_warning: Option<String>,
    ) -> Self {
        Self {
            axis: DraftAxis::empty(),
            settings,
            monitor: MonitorSnapshot::default(),
            clock: ObservationClock::default(),
            frame: 0,
            status: BattleStatus::Waiting,
            speed: 0,
            recording: true,
            strategy: RunStrategy::DryRun,
            triggered: HashSet::new(),
            next_id: 1,
            next_order: 0,
            next_notice: 1,
            notices: Vec::new(),
            last_message: settings_warning.or_else(|| Some("等待选择监控源".to_string())),
            always_on_top: true,
            clear_pending_deadline: None,
            last_observation_at: None,
            error_frames: 0,
            manual_stage: None,
            stage_bounds: None,
        }
    }

    pub fn refresh(&mut self, now: Instant) {
        if self
            .clear_pending_deadline
            .is_some_and(|deadline| now > deadline)
        {
            self.clear_pending_deadline = None;
        }
        if self.monitor.source_kind != MonitorSourceKind::None
            && self
                .last_observation_at
                .is_some_and(|last| now.saturating_duration_since(last) > OBSERVATION_TIMEOUT)
        {
            let update = self.clock.freeze();
            if update.active {
                self.status = BattleStatus::Paused;
            }
            self.speed = 0;
            self.error_frames = self.error_frames.max(1);
            self.monitor.trusted = false;
            self.monitor.battle_state = ObservedBattleState::Unknown;
            self.last_observation_at = None;
            self.last_message = Some("监控观测已过期，计时已冻结".to_string());
        }
    }

    pub fn snapshot(&self) -> RunnerSnapshot {
        let next_event = self
            .axis
            .events
            .iter()
            .find(|event| event.frame >= self.frame)
            .cloned();
        let countdown_frames = next_event
            .as_ref()
            .map(|event| event.frame.saturating_sub(self.frame) as i32);
        RunnerSnapshot {
            axis: self.axis.clone(),
            settings: self.settings.clone(),
            monitor: self.monitor.clone(),
            stage_safety: self.stage_safety(),
            frame: self.frame,
            time: format_frame(self.frame, self.settings.frames_per_cost),
            speed: self.speed,
            status: self.status,
            recording: self.recording,
            strategy: self.strategy,
            next_event,
            countdown_frames,
            error_frames: self.error_frames,
            last_message: self.last_message.clone(),
            notices: self.notices.clone(),
            always_on_top: self.always_on_top,
            clear_pending: self.clear_pending_deadline.is_some(),
        }
    }

    fn stage_safety(&self) -> StageSafetySnapshot {
        let ocr_stage = (self.monitor.stage_recognition.status == StageMatchStatus::Matched)
            .then(|| self.monitor.stage_recognition.stage.clone())
            .flatten();
        let (observed_stage, source) = if let Some(stage) = ocr_stage {
            (Some(stage), StageIdentitySource::Ocr)
        } else if let Some(stage) = self.manual_stage.clone() {
            (Some(stage), StageIdentitySource::Manual)
        } else {
            (None, StageIdentitySource::None)
        };
        let status = match (self.axis.stage_id.as_deref(), observed_stage.as_ref()) {
            (Some(expected), Some(observed)) if expected == observed.id => {
                StageSafetyStatus::Matched
            }
            (Some(_), Some(_)) => StageSafetyStatus::Mismatched,
            _ => StageSafetyStatus::Unverified,
        };
        StageSafetySnapshot {
            status,
            expected_stage_id: self.axis.stage_id.clone(),
            observed_stage,
            source,
        }
    }

    pub fn set_recording(&mut self, enabled: bool) {
        self.recording = enabled;
        self.last_message = Some(if enabled {
            "实时录轴已开启".to_string()
        } else {
            "实时录轴已停止".to_string()
        });
    }

    pub fn record_event(&mut self, kind: DraftKind) -> Result<(), CommandError> {
        if !self.recording {
            return Err(CommandError::new("recording_disabled", "实时录轴尚未开启"));
        }
        if !matches!(self.status, BattleStatus::Running | BattleStatus::Paused) {
            return Err(CommandError::new(
                "battle_not_running",
                "当前不在运行中的关卡内",
            ));
        }
        if self.stage_safety().status != StageSafetyStatus::Matched {
            return Err(CommandError::new(
                "stage_unverified",
                "当前关卡未确认或与轴不一致，不能录制操作点",
            ));
        }
        self.insert_draft(self.frame, kind);
        Ok(())
    }

    pub fn add_event(&mut self, frame: u32, kind: DraftKind) -> Result<(), CommandError> {
        validate_frame(frame)?;
        self.insert_draft(frame, kind);
        Ok(())
    }

    pub fn update_event(&mut self, input: UpdateEventInput) -> Result<(), CommandError> {
        validate_frame(input.frame)?;
        self.clear_pending_deadline = None;
        if input
            .label
            .as_ref()
            .is_some_and(|label| label.chars().count() > 120)
        {
            return Err(CommandError::field(
                "invalid_label",
                "说明不能超过 120 个字符",
                "label",
            ));
        }
        if input
            .tile
            .as_deref()
            .is_some_and(|tile| !valid_tile_code(tile))
        {
            return Err(CommandError::field(
                "invalid_tile",
                "格子必须是 A1 到 I36 的短代码",
                "tile",
            ));
        }
        if let (Some(tile), Some(stage_id), Some((bounds_stage, bounds))) = (
            input.tile.as_deref(),
            self.axis.stage_id.as_deref(),
            self.stage_bounds.as_ref(),
        ) {
            if stage_id == bounds_stage {
                validate_tile_in_map(tile, *bounds)
                    .map_err(|message| CommandError::field("invalid_tile", message, "tile"))?;
            }
        }
        let event_id = input.id.clone();
        {
            let event = self
                .axis
                .events
                .iter_mut()
                .find(|event| event.id == input.id)
                .ok_or_else(|| CommandError::new("event_not_found", "未找到操作点"))?;
            event.frame = input.frame;
            event.kind = input.kind;
            event.operator = if matches!(event.kind, DraftKind::Deploy) {
                normalized_optional(input.operator)
            } else {
                None
            };
            event.label = normalized_optional(input.label);
            event.tile = normalized_optional(input.tile);
            if matches!(event.kind, DraftKind::Deploy) {
                event.direction = input.direction;
            } else {
                event.direction = None;
            }
            event.refresh_complete();
        }
        self.axis.sort_events();
        self.rebuild_triggered();
        self.last_message = Some(format!("已更新操作点 {event_id}"));
        Ok(())
    }

    pub fn move_event(&mut self, id: &str, frame: u32) -> Result<(), CommandError> {
        validate_frame(frame)?;
        self.clear_pending_deadline = None;
        let event = self
            .axis
            .events
            .iter_mut()
            .find(|event| event.id == id)
            .ok_or_else(|| CommandError::new("event_not_found", "未找到操作点"))?;
        event.frame = frame;
        self.axis.sort_events();
        self.rebuild_triggered();
        self.last_message = Some(format!("已移动操作点 {id}"));
        Ok(())
    }

    pub fn delete_event(&mut self, id: &str) -> Result<(), CommandError> {
        self.clear_pending_deadline = None;
        let previous_len = self.axis.events.len();
        self.axis.events.retain(|event| event.id != id);
        if self.axis.events.len() == previous_len {
            return Err(CommandError::new("event_not_found", "未找到操作点"));
        }
        self.rebuild_triggered();
        self.last_message = Some(format!("已删除操作点 {id}"));
        Ok(())
    }

    pub fn set_axis_metadata(&mut self, input: AxisMetadataInput) -> Result<(), CommandError> {
        let title = input.title.trim();
        if title.is_empty() || title.chars().count() > 128 {
            return Err(CommandError::field(
                "invalid_title",
                "轴标题必须包含 1–128 个字符",
                "title",
            ));
        }
        self.axis.title = title.to_string();
        let stage_id = normalized_optional(input.stage_id);
        if self.axis.stage_id != stage_id {
            self.stage_bounds = None;
        }
        self.axis.stage_id = stage_id;
        self.last_message = Some("轴属性已更新".to_string());
        Ok(())
    }

    pub fn replace_axis(&mut self, axis: DraftAxis) {
        self.clear_pending_deadline = None;
        self.next_order = axis
            .events
            .iter()
            .map(|event| event.order)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        self.next_id = 1;
        self.stage_bounds = None;
        self.axis = axis;
        self.rebuild_triggered();
        self.last_message = Some("AxisLink 已导入".to_string());
    }

    pub fn axis(&self) -> &DraftAxis {
        &self.axis
    }

    pub fn set_strategy(&mut self, strategy: RunStrategy) {
        self.strategy = strategy;
        self.rebuild_triggered();
        self.last_message = Some("运行策略已更新".to_string());
    }

    pub fn set_always_on_top(&mut self, enabled: bool) {
        self.always_on_top = enabled;
    }

    pub fn set_runtime_warning(&mut self, message: String) {
        self.last_message = Some(message);
    }

    pub fn set_monitor_snapshot(&mut self, monitor: MonitorSnapshot) {
        self.monitor = monitor;
    }

    pub fn set_manual_stage(&mut self, stage: StageCatalogEntry) {
        if self.axis.stage_id.is_none() {
            self.axis.stage_id = Some(stage.id.clone());
        }
        self.manual_stage = Some(stage);
        self.last_message = Some("已手动确认当前关卡".to_string());
    }

    pub fn set_stage_map_bounds(&mut self, stage_id: String, bounds: StageMapBounds) {
        if self.axis.stage_id.as_deref() == Some(stage_id.as_str()) {
            self.stage_bounds = Some((stage_id, bounds));
        }
    }

    pub fn reset_monitor_clock(&mut self, message: &str) {
        self.clock = ObservationClock::default();
        self.frame = 0;
        self.status = BattleStatus::Waiting;
        self.speed = 0;
        self.error_frames = 0;
        self.last_observation_at = None;
        self.manual_stage = None;
        self.triggered.clear();
        self.notices.clear();
        self.last_message = Some(message.to_string());
    }

    pub fn apply_monitor_event(&mut self, event: MonitorEvent, received_at: Instant) {
        match event {
            MonitorEvent::Observation(observation) => {
                self.last_observation_at = Some(received_at);
                if self.axis.stage_id.is_none()
                    && observation
                        .stage_recognition
                        .as_ref()
                        .is_some_and(|recognition| {
                            recognition.status == StageMatchStatus::Matched
                                && recognition.stage.is_some()
                        })
                {
                    self.axis.stage_id = observation
                        .stage_recognition
                        .as_ref()
                        .and_then(|recognition| recognition.stage.as_ref())
                        .map(|stage| stage.id.clone());
                }
                let previous_frame = self.frame;
                let update = self.clock.observe(&observation);
                self.frame = update.frame;
                self.speed = update.speed;
                self.error_frames = update.error_frames.min(u32::from(u16::MAX)) as u16;
                self.status = if !update.active {
                    BattleStatus::Waiting
                } else if matches!(
                    update.transition,
                    ClockTransition::Paused | ClockTransition::Frozen
                ) {
                    BattleStatus::Paused
                } else {
                    BattleStatus::Running
                };
                match update.transition {
                    ClockTransition::Started => {
                        self.triggered.clear();
                        self.notices.clear();
                        self.last_message = Some("已识别到关卡运行，计时开始".to_string());
                        self.dispatch_events(0);
                    }
                    ClockTransition::Advanced if self.frame > previous_frame => {
                        self.dispatch_events(self.frame);
                    }
                    ClockTransition::Exited => {
                        self.triggered.clear();
                        self.notices.clear();
                        self.manual_stage = None;
                        self.last_message = Some("已离开关卡，计时已归零".to_string());
                    }
                    ClockTransition::Frozen => {
                        self.last_message = Some("游戏状态不可信，计时已冻结".to_string());
                    }
                    ClockTransition::Paused => {
                        self.last_message = Some("游戏已暂停".to_string());
                    }
                    ClockTransition::None | ClockTransition::Advanced => {}
                }
                if self.stage_safety().status == StageSafetyStatus::Mismatched {
                    self.last_message =
                        Some("观测关卡与当前轴不一致，调度和录轴已停止".to_string());
                }
            }
            MonitorEvent::Error(message) => {
                let update = self.clock.freeze();
                if update.active {
                    self.status = BattleStatus::Paused;
                }
                self.speed = 0;
                self.error_frames = self.error_frames.max(1);
                self.last_observation_at = None;
                self.monitor.trusted = false;
                self.last_message = Some(message);
            }
            MonitorEvent::RecordingProgress { .. } | MonitorEvent::RecordingReady { .. } => {}
        }
    }

    pub fn update_settings(&mut self, settings: AppSettings) -> Result<(), CommandError> {
        settings
            .validate()
            .map_err(|message| CommandError::field("invalid_settings", message, "settings"))?;
        self.settings = settings;
        self.last_message = Some("设置已保存".to_string());
        Ok(())
    }

    pub fn request_clear_axis(&mut self, now: Instant) {
        if self.axis.events.is_empty() {
            self.clear_pending_deadline = None;
            self.last_message = Some("当前轴已经为空".to_string());
            return;
        }
        if self
            .clear_pending_deadline
            .is_some_and(|deadline| now <= deadline)
        {
            self.axis.events.clear();
            self.triggered.clear();
            self.next_id = 1;
            self.next_order = 0;
            self.clear_pending_deadline = None;
            self.last_message = Some("当前轴已清空".to_string());
        } else {
            self.clear_pending_deadline = Some(now + CLEAR_CONFIRM_DURATION);
            self.last_message = Some("再次按 F4 或点击清空以确认".to_string());
        }
    }

    fn dispatch_events(&mut self, current_frame: u32) {
        if self.stage_safety().status != StageSafetyStatus::Matched {
            for event in &self.axis.events {
                if trigger_frame(event, self.strategy) <= current_frame {
                    self.triggered.insert(event.id.clone());
                }
            }
            return;
        }
        let due: Vec<DraftEvent> = self
            .axis
            .events
            .iter()
            .filter(|event| {
                let trigger = trigger_frame(event, self.strategy);
                trigger <= current_frame && !self.triggered.contains(&event.id)
            })
            .cloned()
            .collect();

        if self.strategy == RunStrategy::Pause {
            let Some(first) = due.first() else {
                return;
            };
            let pause_frame = first.frame;
            for event in due.iter().take_while(|event| event.frame == pause_frame) {
                self.triggered.insert(event.id.clone());
                self.push_notice(
                    NoticeKind::Paused,
                    Some(event.id.clone()),
                    format!("已到暂停点 {}", event_name(event)),
                );
            }
            self.last_message = Some("真实暂停输入尚未接入，仅记录到点暂停请求".to_string());
            return;
        }

        let mut summaries = Vec::with_capacity(due.len());
        for event in due {
            self.triggered.insert(event.id.clone());
            summaries.push(event_name(&event));
            match self.strategy {
                RunStrategy::Notify => self.push_notice(
                    NoticeKind::Notify,
                    Some(event.id.clone()),
                    format!("提示：即将执行 {}", event_name(&event)),
                ),
                RunStrategy::DryRun => self.push_notice(
                    NoticeKind::DryRun,
                    Some(event.id.clone()),
                    format!("预演：执行 {}", event_name(&event)),
                ),
                RunStrategy::Pause => unreachable!("pause handled above"),
            }
        }
        if summaries.len() > 1 {
            let prefix = if self.strategy == RunStrategy::Notify {
                "提示：即将执行"
            } else {
                "预演：执行"
            };
            self.last_message = Some(format!("{prefix} {}", summaries.join("、")));
        }
    }

    fn push_notice(&mut self, kind: NoticeKind, event_id: Option<String>, message: String) {
        self.last_message = Some(message.clone());
        self.notices.push(RunNotice {
            sequence: self.next_notice,
            kind,
            message,
            event_id,
        });
        self.next_notice = self.next_notice.saturating_add(1);
    }

    fn insert_draft(&mut self, frame: u32, kind: DraftKind) {
        self.clear_pending_deadline = None;
        let id = loop {
            let candidate = format!("draft-{:06}", self.next_id);
            self.next_id += 1;
            if !self.axis.events.iter().any(|event| event.id == candidate) {
                break candidate;
            }
        };
        let order = self.next_order;
        self.next_order = self.next_order.saturating_add(1);
        self.axis
            .events
            .push(DraftEvent::new(id.clone(), frame, order, kind));
        self.axis.sort_events();
        self.rebuild_triggered();
        self.last_message = Some(format!("已记录操作点 {id}"));
    }

    fn rebuild_triggered(&mut self) {
        let current_frame = self.frame;
        self.triggered = self
            .axis
            .events
            .iter()
            .filter(|event| event.frame <= current_frame)
            .map(|event| event.id.clone())
            .collect();
    }
}

fn trigger_frame(event: &DraftEvent, strategy: RunStrategy) -> u32 {
    match strategy {
        RunStrategy::Notify => event.frame.saturating_sub(NOTIFY_LEAD_FRAMES),
        RunStrategy::Pause | RunStrategy::DryRun => event.frame,
    }
}

fn event_name(event: &DraftEvent) -> String {
    event
        .label
        .clone()
        .unwrap_or_else(|| format!("{} @ F{}", kind_name(event.kind), event.frame))
}

fn kind_name(kind: DraftKind) -> &'static str {
    match kind {
        DraftKind::Deploy => "部署",
        DraftKind::Skill => "技能",
        DraftKind::Retreat => "撤退",
    }
}

fn format_frame(frame: u32, frames_per_cost: u16) -> String {
    let denominator = u32::from(frames_per_cost);
    let total_seconds = frame / denominator;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    let subframe = frame % denominator;
    format!("{minutes:02}:{seconds:02}:{subframe:02}/{denominator}")
}

fn normalized_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn validate_frame(frame: u32) -> Result<(), CommandError> {
    if frame > i32::MAX as u32 {
        return Err(CommandError::field(
            "invalid_frame",
            "frame 不能超过 2147483647",
            "frame",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::VisualObservation;

    fn observation(timestamp: u64, state: ObservedBattleState) -> MonitorEvent {
        MonitorEvent::Observation(VisualObservation {
            capture_timestamp_ns: timestamp,
            battle_state: state,
            confidence: 90,
            cost_phase: None,
            cost_total: 30,
            cost_full: false,
            stage_recognition: None,
        })
    }

    fn stage(id: &str) -> StageCatalogEntry {
        StageCatalogEntry {
            id: id.to_string(),
            code: "TEST-1".to_string(),
            name: "测试关卡".to_string(),
            level_path: "obt/test.json".to_string(),
        }
    }

    fn observation_with_stage(timestamp: u64, id: &str) -> MonitorEvent {
        MonitorEvent::Observation(VisualObservation {
            capture_timestamp_ns: timestamp,
            battle_state: ObservedBattleState::OneXRunning,
            confidence: 90,
            cost_phase: None,
            cost_total: 30,
            cost_full: false,
            stage_recognition: Some(crate::stage::StageRecognition {
                status: StageMatchStatus::Matched,
                raw_text: "TEST-1\n测试关卡".to_string(),
                stage: Some(stage(id)),
                candidates: vec![stage(id)],
                warning: None,
            }),
        })
    }

    fn confirm_axis_stage(runner: &mut RunnerState) {
        runner.axis.stage_id = Some("test_stage".to_string());
        runner.manual_stage = Some(stage("test_stage"));
    }

    #[test]
    fn default_axis_is_empty() {
        let runner = RunnerState::new(Instant::now());

        assert!(runner.axis.events.is_empty());
    }

    #[test]
    fn clear_axis_requires_a_second_request() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        runner.add_event(30, DraftKind::Skill).unwrap();

        runner.request_clear_axis(start);
        assert_eq!(runner.axis.events.len(), 1);

        runner.request_clear_axis(start + Duration::from_secs(1));
        assert!(runner.axis.events.is_empty());
    }

    #[test]
    fn logical_time_uses_configured_denominator() {
        assert_eq!(format_frame(60, 60), "00:01:00/60");
        assert_eq!(format_frame(75, 60), "00:01:15/60");
    }

    #[test]
    fn frame_zero_event_triggers_on_observed_battle() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        runner.add_event(0, DraftKind::Skill).unwrap();
        let id = runner
            .axis
            .events
            .iter()
            .find(|event| event.frame == 0)
            .unwrap()
            .id
            .clone();

        runner.apply_monitor_event(observation(0, ObservedBattleState::OneXRunning), start);

        assert!(runner.triggered.contains(&id));
    }

    #[test]
    fn leaving_observed_battle_resets_clock_and_keeps_axis() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        runner.add_event(60, DraftKind::Skill).unwrap();
        runner.apply_monitor_event(observation(0, ObservedBattleState::OneXRunning), start);
        runner.apply_monitor_event(
            observation(1_000_000_000, ObservedBattleState::OneXRunning),
            start,
        );
        for index in 0..30 {
            runner.apply_monitor_event(
                observation(
                    1_100_000_000 + index * 33_333_333,
                    ObservedBattleState::NotInBattle,
                ),
                start,
            );
        }

        assert_eq!(runner.status, BattleStatus::Waiting);
        assert_eq!(runner.frame, 0);
        assert_eq!(runner.axis.events.len(), 1);
    }

    #[test]
    fn moving_processed_event_to_future_rearms_it() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        runner.frame = 100;
        runner.add_event(50, DraftKind::Skill).unwrap();
        let id = runner
            .axis
            .events
            .iter()
            .find(|event| event.frame == 50)
            .unwrap()
            .id
            .clone();
        assert!(runner.triggered.contains(&id));

        runner.move_event(&id, 200).unwrap();

        assert!(!runner.triggered.contains(&id));
    }

    #[test]
    fn notify_rebuild_keeps_future_event_pending() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        runner.frame = 100;
        confirm_axis_stage(&mut runner);
        runner.set_strategy(RunStrategy::Notify);
        runner.add_event(150, DraftKind::Skill).unwrap();
        let id = runner
            .axis
            .events
            .iter()
            .find(|event| event.frame == 150)
            .unwrap()
            .id
            .clone();

        assert!(!runner.triggered.contains(&id));

        runner.dispatch_events(101);

        assert!(runner.triggered.contains(&id));
        assert_eq!(
            runner.notices.last().unwrap().event_id.as_deref(),
            Some(id.as_str())
        );
    }

    #[test]
    fn one_observation_preserves_every_due_notice() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        confirm_axis_stage(&mut runner);
        for _ in 0..40 {
            runner.add_event(0, DraftKind::Skill).unwrap();
        }

        runner.apply_monitor_event(observation(0, ObservedBattleState::OneXRunning), start);

        assert_eq!(runner.notices.len(), 40);
        assert_ne!(runner.notices[0].sequence, runner.notices[1].sequence);
    }

    #[test]
    fn imported_draft_style_id_is_not_reused() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        let mut axis = DraftAxis::demo();
        axis.events[0].id = "draft-000001".to_string();
        runner.replace_axis(axis);

        runner.add_event(10, DraftKind::Skill).unwrap();

        let mut ids = HashSet::new();
        assert!(runner.axis.events.iter().all(|event| ids.insert(&event.id)));
    }

    #[test]
    fn mismatched_stage_consumes_due_events_without_catch_up() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        runner.axis = DraftAxis::demo();
        let due_id = runner.axis.events[0].id.clone();

        runner.apply_monitor_event(observation_with_stage(0, "other_stage"), start);
        runner.apply_monitor_event(observation_with_stage(20_000_000_000, "other_stage"), start);
        assert!(runner.triggered.contains(&due_id));
        assert!(runner.notices.is_empty());

        runner.apply_monitor_event(observation_with_stage(21_000_000_000, "main_00-01"), start);
        assert!(runner.notices.is_empty());
    }
}
