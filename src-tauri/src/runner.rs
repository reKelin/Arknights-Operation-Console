use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use crate::{
    axis::{
        DraftAxis, DraftDirection, DraftEvent, DraftKind, EventFrameRange, TimeConfirmation,
        valid_operator_id, valid_tile_code,
    },
    bindings::{
        AxisMetadataInput, BattleStatus, CommandError, ConfirmEventTimeInput, ConsoleMode,
        NoticeKind, RecordingAttempt, RecordingAttemptStatus, RunNotice, RunStrategy,
        RunnerSnapshot, UpdateEventInput,
    },
    executor::{
        ExecutionReceipt, ExecutionReceiptStatus, PauseProofStatus, ProxySnapshot, ProxyStatus,
    },
    monitor::{
        AnalysisCandidate, CandidateActionKind, CandidateConfirmation, ClockMode, ClockQuality,
        ClockSnapshot, ClockTransition, FacingDirection, HumanClock, MonitorConnectionState,
        MonitorEvent, MonitorEventEnvelope, MonitorSnapshot, MonitorSourceKind,
        ObservedBattleState, ProxyClock, UnconfirmedField, VisualObservation, confirm_candidate,
    },
    recording_continuation::{
        RecordingAlignment, RecordingMergeError, RecordingMergeInput, RecordingMergeMode,
        RecordingMergePreview, plan_recording_merge, resolve_recording_merge,
    },
    session::{AxisRevisionSource, OperationSession, RecordingMergeProvenance, TakeoverStatus},
    settings::AppSettings,
    stage::{
        StageCatalogEntry, StageIdentitySource, StageMapBounds, StageMatchStatus,
        StageSafetySnapshot, StageSafetyStatus, validate_tile_in_map,
    },
};

const NOTIFY_LEAD_FRAMES: u32 = 90;
const CLEAR_CONFIRM_DURATION: Duration = Duration::from_secs(3);
const PROXY_CONFIRM_DURATION: Duration = Duration::from_secs(5);
const OBSERVATION_TIMEOUT: Duration = Duration::from_millis(250);
const PROXY_PAUSE_LEAD_FRAMES: u32 = 3;

#[derive(Clone, Copy)]
struct PausedTransactionProof {
    frame: u32,
    cost_phase: u16,
    cost_total: u16,
}

#[derive(Clone)]
struct PendingTakeover {
    run_id: String,
    parent_revision_id: String,
    attempt_id: Option<String>,
    created_frame: u32,
}

pub struct RunnerState {
    axis: DraftAxis,
    session: OperationSession,
    settings: AppSettings,
    monitor: MonitorSnapshot,
    human_clock: HumanClock,
    proxy_clock: ProxyClock,
    clock_mode: ClockMode,
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
    proxy: ProxySnapshot,
    proxy_confirm_deadline: Option<Instant>,
    pending_execution: Vec<DraftEvent>,
    pending_pause_toggle: bool,
    pause_toggle_inflight: bool,
    pause_toggle_expect_paused: Option<bool>,
    paused_transaction: Option<PausedTransactionProof>,
    console_mode: ConsoleMode,
    recording_attempts: Vec<RecordingAttempt>,
    staged_recording_events: Vec<DraftEvent>,
    active_attempt_id: Option<String>,
    next_attempt_sequence: u32,
    next_proxy_run_sequence: u32,
    pending_takeover: Option<PendingTakeover>,
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
        let axis = DraftAxis::empty();
        Self {
            session: OperationSession::new(
                "session-000001".to_string(),
                axis.clone(),
                AxisRevisionSource::Manual,
            ),
            axis,
            settings,
            monitor: MonitorSnapshot::default(),
            human_clock: HumanClock::default(),
            proxy_clock: ProxyClock::default(),
            clock_mode: ClockMode::Human,
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
            proxy: ProxySnapshot::default(),
            proxy_confirm_deadline: None,
            pending_execution: Vec::new(),
            pending_pause_toggle: false,
            pause_toggle_inflight: false,
            pause_toggle_expect_paused: None,
            paused_transaction: None,
            console_mode: ConsoleMode::ManualRecording,
            recording_attempts: Vec::new(),
            staged_recording_events: Vec::new(),
            active_attempt_id: None,
            next_attempt_sequence: 1,
            next_proxy_run_sequence: 1,
            pending_takeover: None,
        }
    }

    pub fn refresh(&mut self, now: Instant) {
        if self
            .clear_pending_deadline
            .is_some_and(|deadline| now > deadline)
        {
            self.clear_pending_deadline = None;
        }
        if self
            .proxy_confirm_deadline
            .is_some_and(|deadline| now > deadline)
        {
            self.proxy_confirm_deadline = None;
            if !self.proxy.enabled {
                self.proxy.status = ProxyStatus::Disabled;
                self.proxy.message = Some("代理执行确认已超时".to_string());
            }
        }
        if self.monitor.source_kind != MonitorSourceKind::None
            && self
                .last_observation_at
                .is_some_and(|last| now.saturating_duration_since(last) > OBSERVATION_TIMEOUT)
        {
            let update = self.freeze_clock();
            if update.active {
                self.status = BattleStatus::Paused;
            }
            self.speed = 0;
            self.error_frames = self.error_frames.max(1);
            self.monitor.trusted = false;
            self.monitor.battle_state = ObservedBattleState::Unknown;
            self.last_observation_at = None;
            self.last_message = Some("监控观测已过期，计时已冻结".to_string());
            self.disable_proxy("监控观测已过期，代理执行已关闭");
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
            session: self.session.clone(),
            console_mode: self.console_mode,
            recording_attempts: self.recording_attempts.clone(),
            staged_recording_events: self.staged_recording_events.clone(),
            settings: self.settings.clone(),
            monitor: self.monitor.clone(),
            clock: self.clock_snapshot(),
            stage_safety: self.stage_safety(),
            frame: self.frame,
            time: format_frame(self.frame, self.settings.frames_per_cost),
            speed: self.speed,
            status: self.status,
            recording: self.recording,
            strategy: self.strategy,
            proxy: self.proxy.clone(),
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
        self.stage_safety_for_axis(&self.axis)
    }

    fn stage_safety_for_axis(&self, axis: &DraftAxis) -> StageSafetySnapshot {
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
        let status = match (axis.stage_id.as_deref(), observed_stage.as_ref()) {
            (Some(expected), Some(observed)) if expected == observed.id => {
                StageSafetyStatus::Matched
            }
            (Some(_), Some(_)) => StageSafetyStatus::Mismatched,
            _ => StageSafetyStatus::Unverified,
        };
        StageSafetySnapshot {
            status,
            expected_stage_id: axis.stage_id.clone(),
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

    pub fn set_console_mode(&mut self, mode: ConsoleMode) {
        if self.console_mode == mode {
            return;
        }
        self.disable_proxy("工作模式已切换，代理执行已关闭");
        self.console_mode = mode;
        self.last_message = Some("工作模式已切换".to_string());
    }

    pub fn record_bookmark(&mut self) -> Result<(), CommandError> {
        self.ensure_revision_editable()?;
        if self.session.takeover.status == TakeoverStatus::Unknown {
            return Err(CommandError::new(
                "takeover_time_unknown",
                "接管时间锚点尚未确认，不能继续录轴",
            ));
        }
        if !self.recording {
            return Err(CommandError::new("recording_disabled", "实时录轴尚未开启"));
        }
        if !matches!(self.status, BattleStatus::Running | BattleStatus::Paused) {
            return Err(CommandError::new(
                "battle_not_running",
                "当前不在运行中的关卡内",
            ));
        }
        let attempt_id = self.active_attempt_id.clone().ok_or_else(|| {
            CommandError::new("recording_attempt_missing", "当前没有活动录制场次")
        })?;
        let clock = self.clock_snapshot();
        let source_timestamp_ns = clock
            .source_timestamp_ns
            .filter(|timestamp| timestamp.is_finite())
            .ok_or_else(|| {
                CommandError::new("clock_timestamp_missing", "当前没有可追溯的观测来源时间")
            })?;
        let id = self.insert_draft(self.frame, DraftKind::Bookmark);
        if let Some(event) = self.axis.events.iter_mut().find(|event| event.id == id) {
            event.label = Some(format!("待分类 {}", event.order + 1));
            event.attempt_id = Some(attempt_id.clone());
            event.source_timestamp_ns = Some(source_timestamp_ns);
            event.frame_range = EventFrameRange {
                start: clock.frame.saturating_sub(clock.uncertainty_frames),
                end: clock.frame.saturating_add(clock.uncertainty_frames),
            };
            event.clock_quality = clock.quality;
            event.time_confirmation =
                if clock.quality == ClockQuality::Trusted && clock.uncertainty_frames == 0 {
                    TimeConfirmation::Observed
                } else {
                    TimeConfirmation::Unconfirmed
                };
        }
        if let Some(attempt) = self
            .recording_attempts
            .iter_mut()
            .find(|attempt| attempt.id == attempt_id)
        {
            attempt.event_ids.push(id);
        }
        self.last_message = Some(format!("已在 F{} 记录待分类操作", self.frame));
        self.sync_active_revision()?;
        Ok(())
    }

    pub fn add_event(&mut self, frame: u32, kind: DraftKind) -> Result<(), CommandError> {
        self.ensure_revision_editable()?;
        validate_frame(frame)?;
        self.insert_draft(frame, kind);
        self.sync_active_revision()?;
        Ok(())
    }

    pub fn confirm_recording_candidate(
        &mut self,
        candidate: &AnalysisCandidate,
        recording_analysis_id: &str,
        confirmation: CandidateConfirmation,
    ) -> Result<(), CommandError> {
        if self.console_mode != ConsoleMode::RecordingAnalysis {
            return Err(CommandError::new(
                "recording_mode_required",
                "请先切换到录屏分析模式",
            ));
        }
        if self
            .axis
            .events
            .iter()
            .chain(self.staged_recording_events.iter())
            .any(|event| {
                event.source_recording_id.as_deref() == Some(recording_analysis_id)
                    && event.source_segment_index == Some(candidate.segment_index)
                    && event.source_candidate_id.as_deref() == Some(candidate.id.as_str())
            })
        {
            return Err(CommandError::new(
                "candidate_already_confirmed",
                "该录屏候选已加入当前轴",
            ));
        }
        let manually_confirmed = confirmation.manual_time_confirmation;
        let operation = confirm_candidate(candidate, confirmation).map_err(|error| {
            CommandError::field("invalid_candidate_confirmation", error.message, error.field)
        })?;
        validate_frame(operation.game_frame)?;
        let exact_trusted_time = candidate.clock_quality == ClockQuality::Trusted
            && candidate.game_frame_range.start == candidate.game_frame_range.end
            && operation.game_frame == candidate.game_frame_range.start;
        if !exact_trusted_time && !manually_confirmed {
            return Err(CommandError::field(
                "manual_time_confirmation_required",
                "该候选的操作时间仍不确定，请确认校正后的帧",
                "gameFrame",
            ));
        }
        let source_timestamp_ns =
            candidate.source_end.nanoseconds().map_err(|_| {
                CommandError::new("invalid_source_timestamp", "候选来源时间无法换算")
            })? as f64;
        if !source_timestamp_ns.is_finite() {
            return Err(CommandError::new(
                "invalid_source_timestamp",
                "候选来源时间超出可序列化范围",
            ));
        }
        let kind = match operation.kind {
            CandidateActionKind::Deploy => DraftKind::Deploy,
            CandidateActionKind::Skill => DraftKind::Skill,
            CandidateActionKind::Retreat => DraftKind::Retreat,
        };
        let mut event = self.allocate_draft(operation.game_frame, kind);
        let id = event.id.clone();
        event.operator = operation.operator;
        event.tile = Some(operation.tile);
        event.direction = operation.direction.map(|direction| match direction {
            FacingDirection::Up => DraftDirection::Up,
            FacingDirection::Right => DraftDirection::Right,
            FacingDirection::Down => DraftDirection::Down,
            FacingDirection::Left => DraftDirection::Left,
        });
        event.label = Some("录屏校对操作".to_string());
        event.source_recording_id = Some(recording_analysis_id.to_string());
        event.source_candidate_id = Some(operation.candidate_id);
        event.source_segment_index = Some(candidate.segment_index);
        event.source_timestamp_ns = Some(source_timestamp_ns);
        event.frame_range = EventFrameRange {
            start: candidate.game_frame_range.start,
            end: candidate.game_frame_range.end,
        };
        event.clock_quality = candidate.clock_quality;
        event.time_confirmation = if exact_trusted_time {
            TimeConfirmation::Observed
        } else {
            TimeConfirmation::ManuallyCorrected
        };
        event.refresh_complete();
        self.staged_recording_events.push(event);
        self.last_message = Some(format!("已校对录屏候选，等待创建接续版本：{id}"));
        Ok(())
    }

    pub fn preview_recording_merge(
        &self,
        input: &RecordingMergeInput,
    ) -> Result<RecordingMergePreview, CommandError> {
        Ok(self.recording_merge_plan(input)?.preview())
    }

    pub fn create_recording_merge_revision(
        &mut self,
        input: RecordingMergeInput,
    ) -> Result<(), CommandError> {
        let mode = input.mode;
        let (_, attempt_id, created_frame) = self.recording_merge_context(&input)?;
        let plan = self.recording_merge_plan(&input)?;
        let result = resolve_recording_merge(plan, &input.conflict_decisions)
            .map_err(recording_merge_error)?;
        let recording_analysis_id = result.recording_analysis_id.clone();
        let segment_index = result.segment_index;
        let candidate_ids = result.candidate_ids.clone();
        let revision_id = self.session.create_recording_merge_revision(
            &input.parent_revision_id,
            attempt_id,
            created_frame,
            result.axis,
            RecordingMergeProvenance {
                recording_analysis_id: result.recording_analysis_id,
                segment_index,
                frame_offset: result.offset_frames,
                candidate_ids,
            },
        )?;
        self.axis = self.session.current_axis().clone();
        self.staged_recording_events.retain(|event| {
            event.source_recording_id.as_deref() != Some(recording_analysis_id.as_str())
                || event.source_segment_index != Some(segment_index)
        });
        self.stage_bounds = None;
        self.rebuild_triggered();
        let label = match mode {
            RecordingMergeMode::NewAxis => "录屏轴版本",
            RecordingMergeMode::Continuation => "录屏接续版本",
        };
        self.last_message = Some(format!("已创建{label} {revision_id}"));
        Ok(())
    }

    fn recording_merge_plan(
        &self,
        input: &RecordingMergeInput,
    ) -> Result<crate::recording_continuation::RecordingMergePlan, CommandError> {
        let (parent_axis, _, _) = self.recording_merge_context(input)?;
        if self.monitor.recording_analysis_id.as_deref()
            != Some(input.recording_analysis_id.as_str())
        {
            return Err(CommandError::new(
                "recording_analysis_changed",
                "录屏分析结果已变化，请重新检查接续",
            ));
        }
        let segment = self
            .monitor
            .recording_segments
            .iter()
            .find(|segment| segment.index == input.segment_index)
            .ok_or_else(|| CommandError::new("recording_segment_not_found", "未找到录屏区段"))?;
        if input.source_anchor_frame > segment.game_duration_frames {
            return Err(CommandError::field(
                "recording_anchor_out_of_range",
                "录屏接管锚点超出所选区段",
                "sourceAnchorFrame",
            ));
        }
        let candidates = self
            .staged_recording_events
            .iter()
            .chain(self.axis.events.iter().filter(|event| {
                event.complete && event.time_confirmation != TimeConfirmation::Unconfirmed
            }))
            .filter(|event| {
                event.source_recording_id.as_deref() == Some(input.recording_analysis_id.as_str())
                    && event.source_segment_index == Some(input.segment_index)
            })
            .cloned()
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Err(CommandError::new(
                "recording_merge_empty",
                "所选录屏区段没有已校对候选",
            ));
        }
        plan_recording_merge(
            &parent_axis,
            &input.recording_analysis_id,
            input.segment_index,
            RecordingAlignment {
                source_anchor_frame: input.source_anchor_frame,
                target_anchor_frame: input.target_anchor_frame,
                offset_frames: input.offset_frames,
                quality: ClockQuality::Uncertain,
                manual_confirmation: input.manual_alignment_confirmed,
            },
            &candidates,
        )
        .map_err(recording_merge_error)
    }

    fn recording_merge_context(
        &self,
        input: &RecordingMergeInput,
    ) -> Result<(DraftAxis, Option<String>, u32), CommandError> {
        if self.pending_takeover.is_some() {
            return Err(CommandError::new(
                "takeover_cancelling",
                "接管仍在归并最终执行回执，请稍候",
            ));
        }
        if self.proxy.enabled {
            return Err(CommandError::new(
                "proxy_active",
                "代理已武装，停止代理后才能创建接续版本",
            ));
        }
        let parent = self
            .session
            .revision(&input.parent_revision_id)
            .ok_or_else(|| CommandError::new("revision_not_found", "未找到接续父版本"))?;
        match input.mode {
            RecordingMergeMode::NewAxis => Ok((
                DraftAxis {
                    title: parent.axis.title.clone(),
                    stage_id: self
                        .monitor
                        .recording_segments
                        .iter()
                        .find(|segment| segment.index == input.segment_index)
                        .and_then(|segment| segment.stage_recognition.stage.as_ref())
                        .map(|stage| stage.id.clone()),
                    events: Vec::new(),
                },
                None,
                input.target_anchor_frame,
            )),
            RecordingMergeMode::Continuation => {
                if !matches!(
                    parent.source,
                    AxisRevisionSource::Takeover | AxisRevisionSource::RecordingMerge
                ) || parent.attempt_id.is_none()
                {
                    return Err(CommandError::new(
                        "recording_merge_parent_ineligible",
                        "只能从本局接管或录屏接续版本继续合并",
                    ));
                }
                if input.target_anchor_frame != parent.created_frame {
                    return Err(CommandError::field(
                        "recording_target_anchor_mismatch",
                        "目标锚点必须使用父版本的接管帧",
                        "targetAnchorFrame",
                    ));
                }
                parent.axis_json_for_use()?;
                Ok((
                    parent.axis.clone(),
                    parent.attempt_id.clone(),
                    parent.created_frame,
                ))
            }
        }
    }

    pub fn update_event(&mut self, input: UpdateEventInput) -> Result<(), CommandError> {
        self.ensure_revision_editable()?;
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
        ) && stage_id == bounds_stage
        {
            validate_tile_in_map(tile, *bounds)
                .map_err(|message| CommandError::field("invalid_tile", message, "tile"))?;
        }
        let event_id = input.id.clone();
        {
            let event = self
                .axis
                .events
                .iter_mut()
                .find(|event| event.id == input.id)
                .ok_or_else(|| CommandError::new("event_not_found", "未找到操作点"))?;
            if event.frame != input.frame {
                event.time_confirmation = TimeConfirmation::Unconfirmed;
            }
            event.frame = input.frame;
            event.kind = input.kind;
            event.operator = if matches!(event.kind, DraftKind::Deploy) {
                normalized_optional(input.operator)
            } else {
                None
            };
            event.label = normalized_optional(input.label);
            event.tile = if event.kind == DraftKind::Bookmark {
                None
            } else {
                normalized_optional(input.tile)
            };
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
        self.sync_active_revision()?;
        Ok(())
    }

    pub fn move_event(&mut self, id: &str, frame: u32) -> Result<(), CommandError> {
        self.ensure_revision_editable()?;
        validate_frame(frame)?;
        self.clear_pending_deadline = None;
        let event = self
            .axis
            .events
            .iter_mut()
            .find(|event| event.id == id)
            .ok_or_else(|| CommandError::new("event_not_found", "未找到操作点"))?;
        if event.frame != frame {
            event.frame = frame;
            event.time_confirmation = TimeConfirmation::Unconfirmed;
        }
        self.axis.sort_events();
        self.rebuild_triggered();
        self.last_message = Some(format!("已移动操作点 {id}"));
        self.sync_active_revision()?;
        Ok(())
    }

    pub fn confirm_event_time(&mut self, input: ConfirmEventTimeInput) -> Result<(), CommandError> {
        self.ensure_revision_editable()?;
        validate_frame(input.frame)?;
        let event = self
            .axis
            .events
            .iter_mut()
            .find(|event| event.id == input.id)
            .ok_or_else(|| CommandError::new("event_not_found", "未找到操作点"))?;
        let inside_observed_range =
            (event.frame_range.start..=event.frame_range.end).contains(&input.frame);
        if !inside_observed_range && !input.manual_correction_confirmed {
            return Err(CommandError::field(
                "manual_time_confirmation_required",
                "目标帧超出观测范围，必须明确确认人工校正",
                "manualCorrectionConfirmed",
            ));
        }
        event.frame = input.frame;
        event.time_confirmation = if inside_observed_range {
            TimeConfirmation::Observed
        } else {
            TimeConfirmation::ManuallyCorrected
        };
        self.axis.sort_events();
        self.rebuild_triggered();
        self.last_message = Some(format!("已确认操作点 {} 的时间", input.id));
        self.sync_active_revision()?;
        Ok(())
    }

    pub fn delete_event(&mut self, id: &str) -> Result<(), CommandError> {
        self.ensure_revision_editable()?;
        self.clear_pending_deadline = None;
        let previous_len = self.axis.events.len();
        self.axis.events.retain(|event| event.id != id);
        if self.axis.events.len() == previous_len {
            return Err(CommandError::new("event_not_found", "未找到操作点"));
        }
        for attempt in &mut self.recording_attempts {
            attempt.event_ids.retain(|event_id| event_id != id);
        }
        self.rebuild_triggered();
        self.last_message = Some(format!("已删除操作点 {id}"));
        self.sync_active_revision()?;
        Ok(())
    }

    pub fn shift_events(&mut self, ids: &[String], delta: i32) -> Result<(), CommandError> {
        self.ensure_revision_editable()?;
        if ids.is_empty() {
            return Err(CommandError::new("no_events", "没有选择要平移的标记"));
        }
        for event in self
            .axis
            .events
            .iter_mut()
            .filter(|event| ids.contains(&event.id))
        {
            event.frame = if delta.is_negative() {
                event.frame.saturating_sub(delta.unsigned_abs())
            } else {
                event
                    .frame
                    .saturating_add(delta as u32)
                    .min(i32::MAX as u32)
            };
            event.time_confirmation = TimeConfirmation::Unconfirmed;
        }
        self.axis.sort_events();
        self.rebuild_triggered();
        self.last_message = Some(format!("已平移 {} 个标记", ids.len()));
        self.sync_active_revision()?;
        Ok(())
    }

    pub fn reorder_event(&mut self, id: &str, direction: i8) -> Result<(), CommandError> {
        self.ensure_revision_editable()?;
        let index = self
            .axis
            .events
            .iter()
            .position(|event| event.id == id)
            .ok_or_else(|| CommandError::new("event_not_found", "未找到标记"))?;
        let target = if direction < 0 {
            index.checked_sub(1)
        } else {
            (index + 1 < self.axis.events.len()).then_some(index + 1)
        }
        .ok_or_else(|| CommandError::new("event_order_boundary", "标记已经位于边界"))?;
        let order = self.axis.events[index].order;
        self.axis.events[index].order = self.axis.events[target].order;
        self.axis.events[target].order = order;
        self.axis.sort_events();
        self.last_message = Some("标记顺序已更新".to_string());
        self.sync_active_revision()?;
        Ok(())
    }

    pub fn clear_bookmarks(&mut self) -> Result<(), CommandError> {
        self.ensure_revision_editable()?;
        self.axis
            .events
            .retain(|event| event.kind != DraftKind::Bookmark);
        let remaining: HashSet<&str> = self
            .axis
            .events
            .iter()
            .map(|event| event.id.as_str())
            .collect();
        for attempt in &mut self.recording_attempts {
            attempt
                .event_ids
                .retain(|event_id| remaining.contains(event_id.as_str()));
        }
        self.rebuild_triggered();
        self.last_message = Some("未分类书签已清空".to_string());
        self.sync_active_revision()
    }

    pub fn set_axis_metadata(&mut self, input: AxisMetadataInput) -> Result<(), CommandError> {
        self.ensure_revision_editable()?;
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
        self.sync_active_revision()?;
        Ok(())
    }

    pub fn replace_axis(&mut self, axis: DraftAxis) -> Result<(), CommandError> {
        if self.pending_takeover.is_some() {
            return Err(CommandError::new(
                "takeover_cancelling",
                "接管仍在归并最终执行回执，不能导入新轴",
            ));
        }
        let parent_revision_id = self.session.current_revision_id.clone();
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
        self.axis = axis.clone();
        self.session
            .create_imported_revision(&parent_revision_id, self.frame, axis)?;
        self.rebuild_triggered();
        self.last_message = Some("AxisLink 已导入".to_string());
        Ok(())
    }

    pub fn axis(&self) -> &DraftAxis {
        &self.axis
    }

    pub fn axis_json_for_export(&self) -> Result<serde_json::Value, CommandError> {
        if self.stage_safety().status == StageSafetyStatus::Mismatched {
            return Err(CommandError::new(
                "stage_mismatched",
                "当前观测关卡与轴关卡不匹配，不能导出",
            ));
        }
        self.session.current_revision().axis_json_for_use()
    }

    pub fn axis_revision_json_for_export(
        &self,
        revision_id: &str,
    ) -> Result<serde_json::Value, CommandError> {
        let revision = self
            .session
            .revision(revision_id)
            .ok_or_else(|| CommandError::new("revision_not_found", "未找到轴版本"))?;
        if self.stage_safety_for_axis(&revision.axis).status == StageSafetyStatus::Mismatched {
            return Err(CommandError::new(
                "stage_mismatched",
                "当前观测关卡与轴关卡不匹配，不能导出",
            ));
        }
        revision.axis_json_for_use()
    }

    pub fn select_axis_revision(&mut self, revision_id: &str) -> Result<(), CommandError> {
        if self.pending_takeover.is_some() {
            return Err(CommandError::new(
                "takeover_cancelling",
                "接管仍在归并最终执行回执，不能切换轴版本",
            ));
        }
        if self.proxy.enabled {
            return Err(CommandError::new(
                "proxy_active",
                "代理已武装，停止代理后才能切换轴版本",
            ));
        }
        self.session.select_revision(revision_id)?;
        self.axis = self.session.current_axis().clone();
        self.stage_bounds = None;
        self.rebuild_triggered();
        self.last_message = Some(format!("已切换到轴版本 {revision_id}"));
        Ok(())
    }

    pub fn set_strategy(&mut self, strategy: RunStrategy) {
        self.strategy = strategy;
        self.rebuild_triggered();
        self.last_message = Some("运行策略已更新".to_string());
    }

    pub fn request_proxy_execution(&mut self, now: Instant) -> Result<bool, CommandError> {
        if self.pending_takeover.is_some() {
            return Err(CommandError::new(
                "takeover_cancelling",
                "接管仍在归并最终执行回执，请稍候",
            ));
        }
        if self.console_mode != ConsoleMode::Proxy {
            return Err(CommandError::new(
                "proxy_mode_required",
                "请先切换到代理指挥模式",
            ));
        }
        if self.proxy.enabled {
            self.disable_proxy("代理执行已关闭");
            return Ok(false);
        }
        if self
            .proxy_confirm_deadline
            .is_some_and(|deadline| now <= deadline)
        {
            if self.monitor.source_kind != MonitorSourceKind::Window
                || self.monitor.window_id.is_none()
            {
                return Err(CommandError::new(
                    "proxy_not_ready",
                    "武装代理需要已选择的前台游戏窗口",
                ));
            }
            self.session
                .current_revision()
                .axis_json_for_use()
                .map_err(|error| {
                    CommandError::new(
                        "proxy_axis_not_ready",
                        format!("当前作战轴不能用于代理：{}", error.message),
                    )
                })?;
            self.proxy.status = ProxyStatus::Confirming;
            self.proxy.message = Some("正在准备代理执行资源".to_string());
            self.proxy_confirm_deadline = None;
            self.strategy = RunStrategy::Proxy;
            return Ok(true);
        }
        self.proxy.status = ProxyStatus::Confirming;
        self.proxy.message = Some(
            "再次确认将发送 Windows 合成触摸与键盘输入；自动化可能存在账号或反作弊风险".to_string(),
        );
        self.proxy_confirm_deadline = Some(now + PROXY_CONFIRM_DURATION);
        Ok(false)
    }

    pub fn complete_proxy_enable(&mut self) -> Result<(), CommandError> {
        let revision_id = self.session.current_revision_id.clone();
        self.session.arm_revision(&revision_id)?;
        self.proxy.enabled = true;
        self.proxy.status = ProxyStatus::Armed;
        self.proxy.message = Some("代理已武装，将在下一局可信 F0 接管".to_string());
        self.proxy.stop_reason = None;
        self.proxy.pause_proof = PauseProofStatus::None;
        self.proxy.pause_proof_message = None;
        Ok(())
    }

    pub fn emergency_stop(&mut self) {
        self.disable_proxy("界面停止：代理执行已关闭");
    }

    pub fn takeover_available(&self) -> bool {
        self.console_mode == ConsoleMode::Proxy
            && self.proxy.enabled
            && self.proxy.run_id.is_some()
            && self.session.armed_revision_id.is_some()
    }

    pub fn request_takeover_revision(&mut self) -> Result<(), CommandError> {
        if !self.takeover_available() {
            return Err(CommandError::new(
                "takeover_not_available",
                "只有正在执行的本局代理可以接管",
            ));
        }
        let run_id = self.proxy.run_id.clone().expect("checked above");
        let parent_revision_id = self
            .session
            .armed_revision_id
            .clone()
            .expect("checked above");
        let requested_source_timestamp_ns = self
            .clock_snapshot()
            .source_timestamp_ns
            .filter(|timestamp| timestamp.is_finite());
        self.session.takeover.status = TakeoverStatus::Cancelling;
        self.session.takeover.base_revision_id = Some(parent_revision_id.clone());
        self.session.takeover.new_revision_id = None;
        self.session.takeover.requested_source_timestamp_ns = requested_source_timestamp_ns;
        self.session.takeover.inherited_anchor = None;
        self.session.takeover.uncertain_receipt_sequences.clear();
        self.session.takeover.message = Some("接管中：正在取消在途输入并归并最终回执".to_string());
        self.pending_takeover = Some(PendingTakeover {
            run_id,
            parent_revision_id,
            attempt_id: self.active_attempt_id.clone(),
            created_frame: self.frame,
        });
        self.takeover("K 接管：代理执行已中断，等待人工续录");
        self.console_mode = ConsoleMode::ManualRecording;
        self.last_message = self.session.takeover.message.clone();
        Ok(())
    }

    pub fn mark_takeover_awaiting_pause(&mut self) {
        if self.pending_takeover.is_some() {
            self.session.takeover.status = TakeoverStatus::AwaitingPauseProof;
            self.session.takeover.message =
                Some("旧代理事务已收尾，正在确认游戏保持暂停".to_string());
            self.last_message = self.session.takeover.message.clone();
        }
    }

    pub fn finalize_takeover_revision(&mut self) -> Result<(), CommandError> {
        let pending = self
            .pending_takeover
            .clone()
            .ok_or_else(|| CommandError::new("takeover_not_pending", "当前没有等待收尾的接管"))?;
        let mut receipts: Vec<_> = self
            .proxy
            .receipts
            .iter()
            .filter(|receipt| receipt.run_id == pending.run_id)
            .cloned()
            .collect();
        receipts.sort_by_key(|receipt| receipt.receipt_sequence);
        let new_revision_id = self.session.create_takeover_revision(
            &pending.parent_revision_id,
            &pending.run_id,
            pending.attempt_id,
            pending.created_frame,
            &receipts,
        )?;
        self.axis = self.session.current_axis().clone();
        self.rebuild_triggered();

        let clock = self.clock_snapshot();
        let mut trusted_handoff = clock.quality == ClockQuality::Trusted
            && self.monitor.battle_state == ObservedBattleState::Paused;
        if trusted_handoff
            && self
                .session
                .confirm_takeover_time(&new_revision_id)
                .is_err()
        {
            trusted_handoff = false;
        }
        let uncertain_receipt_sequences = self
            .session
            .current_revision()
            .takeover
            .as_ref()
            .and_then(|provenance| provenance.uncertain_receipt_sequence)
            .into_iter()
            .collect();
        self.session.takeover.generation = self.session.takeover.generation.saturating_add(1);
        self.session.takeover.new_revision_id = Some(new_revision_id);
        self.session.takeover.inherited_anchor = clock.anchor;
        self.session.takeover.uncertain_receipt_sequences = uncertain_receipt_sequences;
        self.session.takeover.status = if trusted_handoff {
            TakeoverStatus::Recording
        } else {
            TakeoverStatus::Unknown
        };
        self.session.takeover.message = Some(if trusted_handoff {
            "接管完成；新轴继承本局可信时间，可继续按 P 录制".to_string()
        } else {
            "接管已停止输入，但无法证明暂停或时间锚点；请先确认状态".to_string()
        });
        self.pending_takeover = None;
        self.last_message = self.session.takeover.message.clone();
        Ok(())
    }

    pub fn takeover_is_cancelling(&self) -> bool {
        self.pending_takeover.is_some()
            && self.session.takeover.status == TakeoverStatus::Cancelling
    }

    pub fn fail_pending_takeover(&mut self, message: String) {
        let Some(pending) = self.pending_takeover.take() else {
            return;
        };
        if let Ok(new_revision_id) = self.session.create_takeover_revision(
            &pending.parent_revision_id,
            &pending.run_id,
            pending.attempt_id,
            pending.created_frame,
            &[],
        ) {
            self.axis = self.session.current_axis().clone();
            self.session.takeover.new_revision_id = Some(new_revision_id);
        }
        self.session.takeover.generation = self.session.takeover.generation.saturating_add(1);
        self.session.takeover.status = TakeoverStatus::Unknown;
        self.session.takeover.inherited_anchor = self.clock_snapshot().anchor;
        self.session.takeover.message = Some(message.clone());
        self.console_mode = ConsoleMode::ManualRecording;
        self.last_message = Some(message);
        self.rebuild_triggered();
    }

    pub fn takeover(&mut self, message: &str) {
        if self.clock_mode == ClockMode::Proxy {
            let source_timestamp_ns = self
                .monitor
                .last_source_timestamp_ns
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map(|value| value as u64);
            if let Some(timestamp) = source_timestamp_ns
                && let Some(handoff) = self.proxy_clock.takeover_handoff(timestamp)
            {
                self.human_clock.accept_handoff(handoff);
                self.clock_mode = ClockMode::Human;
            } else {
                self.proxy.pause_proof = PauseProofStatus::Uncertain;
                self.proxy.pause_proof_message = Some("接管时无法建立可信时间锚点".to_string());
            }
        }
        self.disable_proxy(message);
    }

    pub fn take_pending_execution(&mut self) -> Vec<DraftEvent> {
        std::mem::take(&mut self.pending_execution)
    }

    pub fn take_pending_pause_toggle(&mut self) -> bool {
        if !self.pending_pause_toggle {
            return false;
        }
        self.pending_pause_toggle = false;
        self.pause_toggle_inflight = true;
        true
    }

    pub fn begin_proxy_execution(&mut self) {
        if self.status != BattleStatus::Paused || self.monitor.cost_full {
            return;
        }
        let Some(cost_phase) = self.monitor.cost_phase else {
            return;
        };
        self.paused_transaction = Some(PausedTransactionProof {
            frame: self.frame,
            cost_phase,
            cost_total: self.monitor.cost_total,
        });
        self.proxy_clock.begin_paused_transaction();
        self.proxy.pause_proof = PauseProofStatus::Uncertain;
        self.proxy.pause_proof_message = Some("暂停事务进行中，时间暂未确认".to_string());
    }

    pub fn finish_proxy_execution(
        &mut self,
        receipts: Vec<ExecutionReceipt>,
        error: Option<String>,
    ) {
        let proxy_was_enabled = self.proxy.enabled;
        self.proxy.receipts.extend(receipts.clone());
        if self.proxy.receipts.len() > 50 {
            self.proxy.receipts.drain(..self.proxy.receipts.len() - 50);
        }
        for receipt in &receipts {
            if receipt.status == ExecutionReceiptStatus::Confirmed {
                self.triggered.insert(receipt.event_id.clone());
            }
        }
        if !proxy_was_enabled {
            self.proxy.status = ProxyStatus::Disabled;
            return;
        }
        let time_confirmed = self.confirm_paused_transaction();
        if receipts
            .iter()
            .any(|receipt| receipt.status == ExecutionReceiptStatus::Uncertain)
        {
            self.proxy.status = ProxyStatus::WaitingConfirmation;
            self.proxy.message = Some("操作结果待人工确认；游戏保持暂停".to_string());
            self.proxy.stop_reason = error;
        } else if let Some(message) = error {
            self.disable_proxy(&message);
            self.proxy.status = ProxyStatus::Error;
            self.proxy.message = Some(message.clone());
            self.proxy.stop_reason = Some(message.clone());
            self.last_message = Some(message);
        } else if !time_confirmed {
            self.proxy.status = ProxyStatus::WaitingConfirmation;
            self.proxy.message = Some("动作已完成，但无法证明事务内时间未推进".to_string());
        } else {
            self.proxy.status = ProxyStatus::Ready;
            self.proxy.message = Some("代理执行批次完成".to_string());
            self.schedule_proxy(self.frame);
        }
    }

    pub fn finish_pause_toggle(&mut self, error: Option<String>) {
        self.pause_toggle_inflight = false;
        if let Some(message) = error {
            self.disable_proxy(&message);
            self.proxy.status = ProxyStatus::Error;
            self.proxy.stop_reason = Some(message.clone());
            self.last_message = Some(message);
        }
    }

    pub fn resolve_execution_receipt(
        &mut self,
        receipt_sequence: u32,
        confirmed: bool,
    ) -> Result<(), CommandError> {
        let receipt = self
            .proxy
            .receipts
            .iter_mut()
            .find(|receipt| receipt.receipt_sequence == receipt_sequence)
            .ok_or_else(|| CommandError::new("receipt_not_found", "未找到执行回执"))?;
        if receipt.status != ExecutionReceiptStatus::Uncertain {
            return Err(CommandError::new(
                "receipt_not_uncertain",
                "只有待确认回执可以人工处理",
            ));
        }
        receipt.status = if confirmed {
            self.triggered.insert(receipt.event_id.clone());
            ExecutionReceiptStatus::Confirmed
        } else {
            ExecutionReceiptStatus::Failed
        };
        receipt.reason = if confirmed {
            "用户已确认游戏操作完成".to_string()
        } else {
            "用户确认游戏操作未完成".to_string()
        };
        if self
            .session
            .takeover
            .uncertain_receipt_sequences
            .contains(&receipt_sequence)
        {
            self.session
                .resolve_uncertain_receipt(receipt_sequence, confirmed)?;
            self.axis = self.session.active_axis().clone();
            self.session
                .takeover
                .uncertain_receipt_sequences
                .retain(|sequence| *sequence != receipt_sequence);
        }
        if confirmed && self.proxy.enabled && self.proxy.pause_proof == PauseProofStatus::Trusted {
            self.proxy.status = ProxyStatus::Ready;
            self.proxy.message = Some("人工确认完成；可继续代理".to_string());
            self.schedule_proxy(self.frame);
        } else if !confirmed {
            self.disable_proxy("用户确认操作失败，代理已停止");
        }
        Ok(())
    }

    fn clock_snapshot(&self) -> ClockSnapshot {
        match self.clock_mode {
            ClockMode::Human => self.human_clock.snapshot(),
            ClockMode::Proxy => self.proxy_clock.snapshot(),
        }
    }

    fn observe_clock(&mut self, observation: &VisualObservation) -> crate::monitor::ClockUpdate {
        match self.clock_mode {
            ClockMode::Human => self.human_clock.observe(observation),
            ClockMode::Proxy => self.proxy_clock.observe(observation),
        }
    }

    fn mark_clock_gap(&mut self, dropped: u32) -> crate::monitor::ClockUpdate {
        match self.clock_mode {
            ClockMode::Human => self.human_clock.mark_observation_gap(dropped),
            ClockMode::Proxy => self.proxy_clock.mark_observation_gap(dropped),
        }
    }

    fn freeze_clock(&mut self) -> crate::monitor::ClockUpdate {
        match self.clock_mode {
            ClockMode::Human => self.human_clock.freeze(),
            ClockMode::Proxy => self.proxy_clock.freeze(),
        }
    }

    fn try_return_to_human_clock(&mut self) {
        if self.clock_mode == ClockMode::Proxy
            && let Some(handoff) = self.proxy_clock.trusted_handoff()
        {
            self.human_clock.accept_handoff(handoff);
            self.clock_mode = ClockMode::Human;
        }
    }

    fn disable_proxy(&mut self, message: &str) {
        self.proxy.enabled = false;
        self.proxy.status = ProxyStatus::Disabled;
        self.proxy.message = Some(message.to_string());
        self.proxy.stop_reason = Some(message.to_string());
        self.proxy_confirm_deadline = None;
        self.pending_execution.clear();
        self.pending_pause_toggle = false;
        self.pause_toggle_inflight = false;
        self.pause_toggle_expect_paused = None;
        self.paused_transaction = None;
        self.session.disarm();
        self.try_return_to_human_clock();
    }

    fn activate_proxy_at_f0(&mut self) -> Result<(), &'static str> {
        if self.frame != 0
            || self.clock_snapshot().quality != ClockQuality::Trusted
            || self.stage_safety().status != StageSafetyStatus::Matched
        {
            return Err("下一局 F0 的时钟或关卡未通过可信检查");
        }
        let Some(handoff) = self.human_clock.trusted_handoff() else {
            return Err("F0 时钟交接失败");
        };
        self.proxy_clock.accept_handoff(handoff);
        self.clock_mode = ClockMode::Proxy;
        let run_sequence = self.next_proxy_run_sequence;
        self.next_proxy_run_sequence = self.next_proxy_run_sequence.saturating_add(1);
        self.proxy.run_id = Some(format!("proxy-run-{run_sequence:06}"));
        self.proxy.status = ProxyStatus::Ready;
        self.proxy.message = Some("已在可信 F0 接管，等待下一操作".to_string());
        self.schedule_proxy(0);
        Ok(())
    }

    fn confirm_paused_transaction(&mut self) -> bool {
        let Some(proof) = self.paused_transaction.take() else {
            return true;
        };
        let same_phase = self.monitor.cost_phase.is_some_and(|phase| {
            phase == proof.cost_phase && self.monitor.cost_total == proof.cost_total
        });
        let paused = self.monitor.battle_state == ObservedBattleState::Paused;
        let source_timestamp_ns = self
            .monitor
            .last_source_timestamp_ns
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| value as u64);
        if paused
            && !self.monitor.cost_full
            && same_phase
            && self.frame == proof.frame
            && let Some(timestamp) = source_timestamp_ns
        {
            self.proxy_clock.confirm_paused_transaction(timestamp);
            self.proxy.pause_proof = PauseProofStatus::Trusted;
            self.proxy.pause_proof_message = Some("事务前后费用锚点证明逻辑帧未推进".to_string());
            true
        } else {
            self.proxy.pause_proof = PauseProofStatus::Uncertain;
            self.proxy.pause_proof_message =
                Some("满费、费用遮挡、相位变化或未回到暂停，无法证明事务内时间未推进".to_string());
            false
        }
    }

    fn schedule_proxy(&mut self, current_frame: u32) {
        if !self.proxy.enabled
            || self.clock_mode != ClockMode::Proxy
            || matches!(
                self.proxy.status,
                ProxyStatus::Executing | ProxyStatus::WaitingConfirmation | ProxyStatus::Error
            )
            || self.pause_toggle_inflight
            || self.pending_pause_toggle
            || !self.pending_execution.is_empty()
        {
            return;
        }
        let Some(next) = self
            .axis
            .events
            .iter()
            .find(|event| !self.triggered.contains(&event.id))
            .cloned()
        else {
            self.proxy.status = ProxyStatus::Ready;
            self.proxy.message = Some("本局代理操作已完成；游戏保持暂停".to_string());
            return;
        };
        if self.pause_toggle_expect_paused.is_some() && current_frame <= next.frame {
            return;
        }
        if current_frame > next.frame {
            self.proxy.status = ProxyStatus::Executing;
            self.proxy.message = Some(format!("目标 F{} 已跨过，正在生成失败回执", next.frame));
            self.pending_execution.extend(
                self.axis
                    .events
                    .iter()
                    .filter(|event| event.frame == next.frame)
                    .cloned(),
            );
            return;
        }
        if self.status == BattleStatus::Paused {
            if current_frame == next.frame {
                self.proxy.status = ProxyStatus::Executing;
                self.proxy.message = Some(format!("正在代理执行 F{current_frame}"));
                self.pending_execution.extend(
                    self.axis
                        .events
                        .iter()
                        .filter(|event| {
                            event.frame == current_frame && !self.triggered.contains(&event.id)
                        })
                        .cloned(),
                );
            } else {
                self.proxy.status = ProxyStatus::Pausing;
                self.proxy.message = Some(format!(
                    "从暂停 F{current_frame} 恢复以接近 F{}",
                    next.frame
                ));
                self.pending_pause_toggle = true;
                self.pause_toggle_expect_paused = Some(false);
            }
        } else if current_frame.saturating_add(PROXY_PAUSE_LEAD_FRAMES) >= next.frame {
            self.proxy.status = ProxyStatus::Pausing;
            self.proxy.message = Some(format!("提前请求暂停以对齐 F{}", next.frame));
            self.pending_pause_toggle = true;
            self.pause_toggle_expect_paused = Some(true);
        }
    }

    pub fn set_always_on_top(&mut self, enabled: bool) {
        self.always_on_top = enabled;
    }

    pub fn set_runtime_warning(&mut self, message: String) {
        self.last_message = Some(message);
    }

    fn ensure_revision_editable(&self) -> Result<(), CommandError> {
        if self.pending_takeover.is_some() {
            return Err(CommandError::new(
                "takeover_cancelling",
                "接管仍在归并最终执行回执，请稍候",
            ));
        }
        if self.session.active_revision_is_selected() {
            Ok(())
        } else {
            Err(CommandError::new(
                "revision_read_only",
                "旧轴版本为只读；请切回当前续录版本",
            ))
        }
    }

    fn sync_active_revision(&mut self) -> Result<(), CommandError> {
        self.session.sync_active_axis(&self.axis)
    }

    pub fn set_monitor_snapshot(&mut self, monitor: MonitorSnapshot) {
        let analysis_completed = monitor.source_kind == MonitorSourceKind::Recording
            && monitor.connection_state == MonitorConnectionState::Ready
            && (self.monitor.connection_state != MonitorConnectionState::Ready
                || monitor.recording_analysis_id != self.monitor.recording_analysis_id);
        if monitor.recording_analysis_id != self.monitor.recording_analysis_id {
            self.staged_recording_events.clear();
        }
        self.monitor = monitor;
        if analysis_completed
            && let Some(segment_index) = self
                .monitor
                .recording_segments
                .iter()
                .max_by_key(|segment| {
                    (
                        segment.stage_recognition.status == crate::stage::StageMatchStatus::Matched,
                        segment
                            .source_end_frame
                            .saturating_sub(segment.source_start_frame),
                        std::cmp::Reverse(segment.index),
                    )
                })
                .map(|segment| segment.index)
            && let Err(error) = self.select_recording_segment(segment_index)
        {
            self.last_message = Some(error.message);
        }
    }

    pub fn select_recording_segment(&mut self, segment_index: u32) -> Result<(), CommandError> {
        if self.monitor.source_kind != MonitorSourceKind::Recording
            || self.monitor.connection_state != MonitorConnectionState::Ready
        {
            return Err(CommandError::new(
                "recording_analysis_not_ready",
                "录屏分析尚未完成",
            ));
        }
        if self.proxy.enabled || self.pending_takeover.is_some() {
            return Err(CommandError::new(
                "recording_axis_busy",
                "代理或接管尚未停止，不能切换录屏轴",
            ));
        }
        let analysis_id = self.monitor.recording_analysis_id.clone().ok_or_else(|| {
            CommandError::new(
                "recording_analysis_identity_missing",
                "录屏分析缺少来源标识",
            )
        })?;
        if let Some(revision_id) = self
            .session
            .revisions
            .iter()
            .rev()
            .find(|revision| {
                revision.attempt_id.is_none()
                    && revision.recording_merge.as_ref().is_some_and(|source| {
                        source.recording_analysis_id == analysis_id
                            && source.segment_index == segment_index
                    })
            })
            .map(|revision| revision.id.clone())
        {
            return self.select_axis_revision(&revision_id);
        }
        let segment = self
            .monitor
            .recording_segments
            .iter()
            .find(|segment| segment.index == segment_index)
            .ok_or_else(|| CommandError::new("recording_segment_not_found", "未找到录屏区段"))?;
        let stage = segment.stage_recognition.stage.clone();
        let segment_source_start = segment.source_start_frame;
        let segment_source_end = segment.source_end_frame;
        let candidates = self
            .monitor
            .recording_candidates
            .iter()
            .filter(|candidate| candidate.segment_index == segment_index)
            .cloned()
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Err(CommandError::new(
                "recording_candidates_empty",
                "该区段没有识别到操作",
            ));
        }
        let mut axis = DraftAxis {
            title: format!(
                "{} · 录屏轴",
                stage
                    .as_ref()
                    .map_or("未确认关卡", |stage| stage.code.as_str())
            ),
            stage_id: stage.map(|stage| stage.id),
            events: Vec::with_capacity(candidates.len()),
        };
        for candidate in &candidates {
            if candidate.game_frame_range.start > candidate.game_frame_range.end {
                return Err(CommandError::new(
                    "recording_time_invalid",
                    "录屏候选时间范围无效",
                ));
            }
            validate_frame(candidate.game_frame_range.end)?;
            let known = |field| {
                candidate.confidence >= 70 && !candidate.unconfirmed_fields.contains(&field)
            };
            let kind = if known(UnconfirmedField::ActionKind) {
                match candidate.kind {
                    Some(CandidateActionKind::Deploy) => DraftKind::Deploy,
                    Some(CandidateActionKind::Skill) => DraftKind::Skill,
                    Some(CandidateActionKind::Retreat) => DraftKind::Retreat,
                    None => DraftKind::Bookmark,
                }
            } else {
                DraftKind::Bookmark
            };
            let source_end_ns = candidate.source_end.nanoseconds().ok();
            let estimated_frame = source_end_ns
                .and_then(|timestamp| {
                    self.monitor
                        .trace_points
                        .iter()
                        .rev()
                        .find(|point| {
                            point.source_timestamp_ns <= timestamp as f64
                                && point.source_frame >= segment_source_start
                                && point.source_frame <= segment_source_end
                        })
                        .map(|point| point.game_frame)
                })
                .unwrap_or(candidate.game_frame_range.start);
            let mut event = self.allocate_draft(estimated_frame, kind);
            event.source_recording_id = Some(analysis_id.clone());
            event.source_candidate_id = Some(candidate.id.clone());
            event.source_segment_index = Some(segment_index);
            event.source_timestamp_ns = candidate
                .source_end
                .nanoseconds()
                .ok()
                .map(|value| value as f64);
            event.frame_range = EventFrameRange {
                start: candidate.game_frame_range.start,
                end: candidate.game_frame_range.end,
            };
            event.clock_quality = candidate.clock_quality;
            event.time_confirmation = if known(UnconfirmedField::GameFrame)
                && candidate.clock_quality == ClockQuality::Trusted
                && candidate.game_frame_range.start == candidate.game_frame_range.end
                && event.source_timestamp_ns.is_some()
            {
                TimeConfirmation::Observed
            } else {
                TimeConfirmation::Unconfirmed
            };
            if kind != DraftKind::Bookmark && known(UnconfirmedField::Tile) {
                event.tile = candidate.tile.clone().filter(|tile| valid_tile_code(tile));
            }
            if kind == DraftKind::Deploy {
                if known(UnconfirmedField::Operator) {
                    event.operator = candidate
                        .operator
                        .clone()
                        .filter(|operator| valid_operator_id(operator));
                }
                if known(UnconfirmedField::Direction) {
                    event.direction = candidate.direction.map(|direction| match direction {
                        FacingDirection::Up => DraftDirection::Up,
                        FacingDirection::Right => DraftDirection::Right,
                        FacingDirection::Down => DraftDirection::Down,
                        FacingDirection::Left => DraftDirection::Left,
                    });
                }
            }
            event.refresh_complete();
            axis.events.push(event);
        }
        axis.sort_events();
        let pending = axis
            .events
            .iter()
            .filter(|event| {
                !event.complete || event.time_confirmation == TimeConfirmation::Unconfirmed
            })
            .count();
        let parent_id = self.session.current_revision_id.clone();
        self.session.create_recording_merge_revision(
            &parent_id,
            None,
            0,
            axis.clone(),
            RecordingMergeProvenance {
                recording_analysis_id: analysis_id,
                segment_index,
                frame_offset: 0,
                candidate_ids: candidates
                    .into_iter()
                    .map(|candidate| candidate.id)
                    .collect(),
            },
        )?;
        self.axis = axis;
        self.stage_bounds = None;
        self.rebuild_triggered();
        self.last_message = Some(format!(
            "录屏轴已自动填充，{pending} 个操作待校对；按 H 编辑"
        ));
        Ok(())
    }

    pub fn set_manual_stage(&mut self, stage: StageCatalogEntry) -> Result<(), CommandError> {
        self.ensure_revision_editable()?;
        if self.axis.stage_id.is_none() {
            self.axis.stage_id = Some(stage.id.clone());
        }
        self.manual_stage = Some(stage);
        self.last_message = Some("已手动确认当前关卡".to_string());
        self.sync_active_revision()
    }

    pub fn set_stage_map_bounds(&mut self, stage_id: String, bounds: StageMapBounds) {
        if self.axis.stage_id.as_deref() == Some(stage_id.as_str()) {
            self.stage_bounds = Some((stage_id, bounds));
        }
    }

    pub fn reset_monitor_clock(&mut self, message: &str) {
        let source_timestamp_ns = self
            .clock_snapshot()
            .source_timestamp_ns
            .filter(|timestamp| timestamp.is_finite());
        self.end_recording_attempt(source_timestamp_ns);
        self.human_clock = HumanClock::default();
        self.proxy_clock = ProxyClock::default();
        self.clock_mode = ClockMode::Human;
        self.frame = 0;
        self.status = BattleStatus::Waiting;
        self.speed = 0;
        self.error_frames = 0;
        self.last_observation_at = None;
        self.manual_stage = None;
        self.triggered.clear();
        self.notices.clear();
        self.disable_proxy("监控源变化，代理执行已关闭");
        self.last_message = Some(message.to_string());
    }

    pub fn apply_monitor_event(&mut self, envelope: MonitorEventEnvelope, received_at: Instant) {
        if envelope.dropped_before > 0 {
            let update = self.mark_clock_gap(envelope.dropped_before);
            self.frame = update.frame;
            self.speed = 0;
            self.error_frames = update.uncertainty_frames.min(u32::from(u16::MAX)) as u16;
            self.status = if update.active {
                BattleStatus::Paused
            } else {
                BattleStatus::Waiting
            };
            self.disable_proxy("监控观测丢失，代理执行已关闭");
            self.last_message = Some(format!(
                "丢失 {} 条监控观测，计时等待锚点恢复",
                envelope.dropped_before
            ));
        }
        match envelope.event {
            MonitorEvent::Observation(observation) => {
                self.last_observation_at = Some(received_at);
                if self.session.active_revision_is_selected()
                    && self.axis.stage_id.is_none()
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
                    let _ = self.sync_active_revision();
                }
                self.update_active_attempt_stage(&observation);
                let previous_frame = self.frame;
                let isolated_transaction_state = self.paused_transaction.is_some()
                    && self.clock_mode == ClockMode::Proxy
                    && matches!(
                        observation.battle_state,
                        ObservedBattleState::Paused
                            | ObservedBattleState::PointTwoXRunning
                            | ObservedBattleState::DeployingOperator
                            | ObservedBattleState::AdjustingOperatorFacing
                    );
                let update = if isolated_transaction_state {
                    self.proxy_clock.begin_paused_transaction()
                } else {
                    self.observe_clock(&observation)
                };
                self.frame = update.frame;
                self.speed = match update.speed_fifths {
                    0 => 0,
                    10 => 2,
                    _ => 1,
                };
                self.error_frames = update.uncertainty_frames.min(u32::from(u16::MAX)) as u16;
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
                if !self.pause_toggle_inflight
                    && !self.pending_pause_toggle
                    && self
                        .pause_toggle_expect_paused
                        .is_some_and(|expect_paused| {
                            expect_paused == (self.status == BattleStatus::Paused)
                        })
                {
                    self.pause_toggle_expect_paused = None;
                    self.proxy.status = ProxyStatus::Ready;
                }
                match update.transition {
                    ClockTransition::Started => {
                        self.start_recording_attempt(&observation);
                        self.triggered.clear();
                        self.notices.clear();
                        self.last_message = Some("已识别到关卡运行，计时开始".to_string());
                        if self.proxy.enabled && self.proxy.status == ProxyStatus::Armed {
                            if let Err(message) = self.activate_proxy_at_f0() {
                                self.disable_proxy(message);
                                self.last_message = Some(message.to_string());
                            }
                        } else {
                            self.dispatch_events(0);
                        }
                    }
                    ClockTransition::Advanced
                        if self.frame > previous_frame
                            && update.quality == ClockQuality::Trusted =>
                    {
                        self.dispatch_events(self.frame);
                    }
                    ClockTransition::Exited => {
                        self.end_recording_attempt(Some(observation.capture_timestamp_ns as f64));
                        self.triggered.clear();
                        self.notices.clear();
                        self.manual_stage = None;
                        self.disable_proxy("已离开关卡，代理执行已关闭");
                        self.last_message = Some("已离开关卡，计时已归零".to_string());
                    }
                    ClockTransition::Frozen => {
                        if isolated_transaction_state {
                            self.proxy.pause_proof = PauseProofStatus::Uncertain;
                            self.proxy.pause_proof_message =
                                Some("暂停事务进行中，时间暂未确认".to_string());
                            self.last_message =
                                Some("暂停事务观测已隔离，等待后置锚点".to_string());
                        } else {
                            self.disable_proxy("游戏状态不可信，代理执行已关闭");
                            self.last_message = Some("游戏状态不可信，计时已冻结".to_string());
                        }
                    }
                    ClockTransition::Paused => {
                        self.last_message = Some("游戏已暂停".to_string());
                        self.schedule_proxy(self.frame);
                    }
                    ClockTransition::None | ClockTransition::Advanced => {}
                }
                if self.stage_safety().status == StageSafetyStatus::Mismatched {
                    self.disable_proxy("观测关卡与当前轴不一致，代理执行已关闭");
                    self.last_message =
                        Some("观测关卡与当前轴不一致，调度和录轴已停止".to_string());
                }
                if self.proxy.enabled
                    && self.clock_mode == ClockMode::Proxy
                    && update.quality == ClockQuality::Trusted
                {
                    self.schedule_proxy(self.frame);
                }
            }
            MonitorEvent::Error(message) => {
                let update = self.freeze_clock();
                if update.active {
                    self.status = BattleStatus::Paused;
                }
                self.speed = 0;
                self.error_frames = self.error_frames.max(1);
                self.last_observation_at = None;
                self.monitor.trusted = false;
                self.disable_proxy("监控错误，代理执行已关闭");
                self.last_message = Some(message);
            }
            MonitorEvent::RecordingProgress { .. } | MonitorEvent::RecordingReady { .. } => {}
        }
        if !self.proxy.enabled {
            self.try_return_to_human_clock();
        }
    }

    fn start_recording_attempt(&mut self, observation: &VisualObservation) {
        self.end_recording_attempt(Some(observation.capture_timestamp_ns as f64));
        let sequence = self.next_attempt_sequence;
        self.next_attempt_sequence = self.next_attempt_sequence.saturating_add(1);
        let id = format!("attempt-{sequence:06}");
        let stage_id = observation
            .stage_recognition
            .as_ref()
            .filter(|recognition| recognition.status == StageMatchStatus::Matched)
            .and_then(|recognition| recognition.stage.as_ref())
            .map(|stage| stage.id.clone());
        self.recording_attempts.push(RecordingAttempt {
            id: id.clone(),
            sequence,
            status: RecordingAttemptStatus::Active,
            stage_id,
            started_source_timestamp_ns: observation.capture_timestamp_ns as f64,
            ended_source_timestamp_ns: None,
            event_ids: Vec::new(),
        });
        self.active_attempt_id = Some(id);
    }

    fn update_active_attempt_stage(&mut self, observation: &VisualObservation) {
        let Some(id) = self.active_attempt_id.as_deref() else {
            return;
        };
        let Some(stage_id) = observation
            .stage_recognition
            .as_ref()
            .filter(|recognition| recognition.status == StageMatchStatus::Matched)
            .and_then(|recognition| recognition.stage.as_ref())
            .map(|stage| stage.id.clone())
        else {
            return;
        };
        if let Some(attempt) = self
            .recording_attempts
            .iter_mut()
            .find(|attempt| attempt.id == id && attempt.stage_id.is_none())
        {
            attempt.stage_id = Some(stage_id);
        }
    }

    fn end_recording_attempt(&mut self, source_timestamp_ns: Option<f64>) {
        let Some(id) = self.active_attempt_id.take() else {
            return;
        };
        if let Some(attempt) = self
            .recording_attempts
            .iter_mut()
            .find(|attempt| attempt.id == id)
        {
            attempt.status = RecordingAttemptStatus::Ended;
            attempt.ended_source_timestamp_ns = source_timestamp_ns;
        }
    }

    pub fn update_settings(&mut self, mut settings: AppSettings) -> Result<(), CommandError> {
        settings
            .validate()
            .map_err(|message| CommandError::field("invalid_settings", message, "settings"))?;
        let keys_changed = self.settings.pause_key != settings.pause_key
            || self.settings.skill_key != settings.skill_key
            || self.settings.retreat_key != settings.retreat_key;
        if keys_changed {
            settings.bindings_confirmed = false;
        }
        self.settings = settings;
        self.last_message = Some(if keys_changed {
            "键位已更新，首次执行前需要重新确认".to_string()
        } else {
            "设置已保存".to_string()
        });
        Ok(())
    }

    pub fn request_clear_axis(&mut self, now: Instant) -> Result<(), CommandError> {
        self.ensure_revision_editable()?;
        if self.axis.events.is_empty() {
            self.clear_pending_deadline = None;
            self.last_message = Some("当前轴已经为空".to_string());
            return Ok(());
        }
        if self
            .clear_pending_deadline
            .is_some_and(|deadline| now <= deadline)
        {
            self.axis.events.clear();
            for attempt in &mut self.recording_attempts {
                attempt.event_ids.clear();
            }
            self.triggered.clear();
            self.next_id = 1;
            self.next_order = 0;
            self.clear_pending_deadline = None;
            self.last_message = Some("当前轴已清空".to_string());
            self.sync_active_revision()?;
        } else {
            self.clear_pending_deadline = Some(now + CLEAR_CONFIRM_DURATION);
            self.last_message = Some("请再次点击清空以确认".to_string());
        }
        Ok(())
    }

    fn dispatch_events(&mut self, current_frame: u32) {
        if self.strategy == RunStrategy::Proxy {
            self.schedule_proxy(current_frame);
            return;
        }
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
                RunStrategy::Proxy => unreachable!("proxy handled above"),
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

    fn insert_draft(&mut self, frame: u32, kind: DraftKind) -> String {
        self.clear_pending_deadline = None;
        let event = self.allocate_draft(frame, kind);
        let id = event.id.clone();
        self.axis.events.push(event);
        self.axis.sort_events();
        self.rebuild_triggered();
        self.last_message = Some(format!("已记录操作点 {id}"));
        id
    }

    fn allocate_draft(&mut self, frame: u32, kind: DraftKind) -> DraftEvent {
        let id = loop {
            let candidate = format!("draft-{:06}", self.next_id);
            self.next_id += 1;
            if !self.axis.events.iter().any(|event| event.id == candidate)
                && !self
                    .staged_recording_events
                    .iter()
                    .any(|event| event.id == candidate)
            {
                break candidate;
            }
        };
        let order = self.next_order;
        self.next_order = self.next_order.saturating_add(1);
        DraftEvent::new(id, frame, order, kind)
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
        RunStrategy::Pause | RunStrategy::DryRun | RunStrategy::Proxy => event.frame,
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
        DraftKind::Bookmark => "书签",
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

fn recording_merge_error(error: RecordingMergeError) -> CommandError {
    match error.candidate_id {
        Some(candidate_id) => CommandError::field(
            error.code,
            format!("{}（候选 {candidate_id}）", error.message),
            "candidateId",
        ),
        None => CommandError::new(error.code, error.message),
    }
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

    fn observation(timestamp: u64, state: ObservedBattleState) -> MonitorEventEnvelope {
        MonitorEventEnvelope {
            sequence: timestamp.saturating_add(1),
            source_timestamp_ns: Some(timestamp),
            dropped_before: 0,
            event: MonitorEvent::Observation(VisualObservation {
                capture_timestamp_ns: timestamp,
                battle_state: state,
                confidence: 90,
                cost_phase: None,
                cost_total: 30,
                cost_full: false,
                stage_recognition: None,
                title_candidate: false,
            }),
        }
    }

    fn stage(id: &str) -> StageCatalogEntry {
        StageCatalogEntry {
            id: id.to_string(),
            code: "TEST-1".to_string(),
            name: "测试关卡".to_string(),
            level_path: "obt/test.json".to_string(),
        }
    }

    fn observation_with_stage(timestamp: u64, id: &str) -> MonitorEventEnvelope {
        MonitorEventEnvelope {
            sequence: timestamp.saturating_add(1),
            source_timestamp_ns: Some(timestamp),
            dropped_before: 0,
            event: MonitorEvent::Observation(VisualObservation {
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
                title_candidate: false,
            }),
        }
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
    fn recording_ready_prefers_matched_battle_over_intro() {
        let mut runner = RunnerState::new(Instant::now());
        let segments = serde_json::from_value(serde_json::json!([
            { "index": 0, "sourceStartFrame": 0, "sourceEndFrame": 126, "gameDurationFrames": 2, "stageRecognition": { "status": "unavailable", "rawText": "", "stage": null, "candidates": [], "warning": null } },
            { "index": 1, "sourceStartFrame": 459, "sourceEndFrame": 11458, "gameDurationFrames": 9000, "stageRecognition": { "status": "matched", "rawText": "SR-8", "stage": { "id": "test", "code": "SR-8", "name": "测试", "levelPath": "test.json" }, "candidates": [], "warning": null } }
        ])).unwrap();
        runner.set_monitor_snapshot(MonitorSnapshot {
            source_kind: MonitorSourceKind::Recording,
            connection_state: MonitorConnectionState::Ready,
            recording_analysis_id: Some("intro-regression".to_string()),
            recording_candidates: [0,1].into_iter().map(|index| serde_json::from_value(serde_json::json!({
                "id": format!("candidate-{index}"), "segmentIndex": index,
                "sourceStart": { "rawPts": "100", "timeBase": { "numerator": 1, "denominator": 1000 } },
                "sourceEnd": { "rawPts": "200", "timeBase": { "numerator": 1, "denominator": 1000 } },
                "gameFrameRange": { "start": 30, "end": 34 }, "clockQuality": "uncertain",
                "kind": null, "operator": null, "tile": null, "direction": null,
                "evidence": "interruptedInteraction", "confidence": 49, "unconfirmedFields": ["actionKind", "gameFrame", "tile"]
            })).unwrap()).collect(),
            recording_segments: segments,
            ..MonitorSnapshot::default()
        });
        assert_eq!(runner.axis.stage_id.as_deref(), Some("test"));
        assert_eq!(
            runner
                .session
                .revisions
                .last()
                .unwrap()
                .recording_merge
                .as_ref()
                .unwrap()
                .segment_index,
            1
        );
    }

    #[test]
    fn recording_ready_fills_known_fields_once_and_preserves_uncertainty() {
        let mut runner = RunnerState::new(Instant::now());
        let candidate: AnalysisCandidate = serde_json::from_value(serde_json::json!({
            "id": "candidate-1", "segmentIndex": 0,
            "sourceStart": { "rawPts": "100", "timeBase": { "numerator": 1, "denominator": 1000 } },
            "sourceEnd": { "rawPts": "200", "timeBase": { "numerator": 1, "denominator": 1000 } },
            "gameFrameRange": { "start": 30, "end": 30 }, "clockQuality": "trusted",
            "kind": "deploy", "operator": "char_002_amiya", "tile": "C5", "direction": "right",
            "evidence": "deploymentGesture", "confidence": 90, "unconfirmedFields": []
        }))
        .unwrap();
        let mut uncertain = candidate.clone();
        uncertain.id = "candidate-2".to_string();
        uncertain.game_frame_range.start = 0;
        uncertain.game_frame_range.end = 1200;
        uncertain.source_end.raw_pts = "400".into();
        uncertain.unconfirmed_fields = vec![UnconfirmedField::Tile];
        let mut second_segment = candidate.clone();
        second_segment.id = "candidate-3".to_string();
        second_segment.segment_index = 1;
        let segments = serde_json::from_value(serde_json::json!([
            { "index": 0, "sourceStartFrame": 0, "sourceEndFrame": 300, "gameDurationFrames": 90, "stageRecognition": { "status": "matched", "rawText": "TEST-1", "stage": { "id": "test", "code": "TEST-1", "name": "测试", "levelPath": "test.json" }, "candidates": [], "warning": null } },
            { "index": 1, "sourceStartFrame": 301, "sourceEndFrame": 600, "gameDurationFrames": 90, "stageRecognition": { "status": "unavailable", "rawText": "", "stage": null, "candidates": [], "warning": null } }
        ])).unwrap();
        let monitor = MonitorSnapshot {
            source_kind: MonitorSourceKind::Recording,
            connection_state: MonitorConnectionState::Ready,
            recording_analysis_id: Some("analysis-1".to_string()),
            trace_points: serde_json::from_value(serde_json::json!([
                { "sourceFrame": 12, "sourceTimestampNs": 200000000.0, "gameFrame": 30, "gameFrameMin": 30, "gameFrameMax": 30, "clockQuality": "trusted", "battleState": "oneXRunning", "costPhase": null },
                { "sourceFrame": 24, "sourceTimestampNs": 400000000.0, "gameFrame": 396, "gameFrameMin": 0, "gameFrameMax": 1200, "clockQuality": "uncertain", "battleState": "twoXRunning", "costPhase": null }
            ])).unwrap(),
            recording_candidates: vec![candidate, uncertain, second_segment],
            recording_segments: segments,
            ..MonitorSnapshot::default()
        };
        runner.set_monitor_snapshot(monitor.clone());
        assert_eq!(runner.axis.events.len(), 2);
        assert!(runner.axis.events[0].complete);
        assert_eq!(
            runner.axis.events[0].time_confirmation,
            TimeConfirmation::Observed
        );
        assert_eq!(
            runner.axis.events[1].frame, 396,
            "应使用估计帧，不能把误差下界 0 当作落点"
        );
        assert_eq!(runner.axis.events[1].tile, None);
        assert!(!runner.axis.events[1].complete);
        assert_eq!(
            runner.axis.events[1].time_confirmation,
            TimeConfirmation::Unconfirmed
        );
        assert!(runner.axis_json_for_export().is_err());
        assert!(runner.session.revisions[0].axis.events.is_empty());
        runner.set_monitor_snapshot(monitor);
        assert_eq!(runner.session.revisions.len(), 2);
        runner.select_recording_segment(1).unwrap();
        assert_eq!(runner.axis.events.len(), 1);
        assert_eq!(runner.axis.events[0].source_segment_index, Some(1));
        runner.select_recording_segment(0).unwrap();
        assert_eq!(runner.axis.events.len(), 2);
        assert_eq!(runner.session.revisions.len(), 3);
        assert!(runner.select_recording_segment(9).is_err());
    }

    #[test]
    fn clear_axis_requires_a_second_request() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        runner.add_event(30, DraftKind::Skill).unwrap();

        runner.request_clear_axis(start).unwrap();
        assert_eq!(runner.axis.events.len(), 1);

        runner
            .request_clear_axis(start + Duration::from_secs(1))
            .unwrap();
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
        runner.replace_axis(axis).unwrap();

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

    #[test]
    fn proxy_batches_same_frame_events_in_stable_order() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        confirm_axis_stage(&mut runner);
        runner.monitor.source_kind = MonitorSourceKind::Window;
        runner.monitor.window_id = Some("1".to_string());
        runner.monitor.trusted = true;
        runner.status = BattleStatus::Paused;
        runner.proxy.enabled = true;
        runner.proxy.status = ProxyStatus::Ready;
        runner.strategy = RunStrategy::Proxy;
        runner.clock_mode = ClockMode::Proxy;
        runner.axis.events = vec![
            DraftEvent {
                id: "second".to_string(),
                frame: 30,
                order: 2,
                kind: DraftKind::Skill,
                operator: None,
                tile: Some("A1".to_string()),
                direction: None,
                label: None,
                complete: true,
                ..DraftEvent::new("unused".to_string(), 30, 2, DraftKind::Skill)
            },
            DraftEvent {
                id: "first".to_string(),
                frame: 30,
                order: 1,
                kind: DraftKind::Retreat,
                operator: None,
                tile: Some("A1".to_string()),
                direction: None,
                label: None,
                complete: true,
                ..DraftEvent::new("unused".to_string(), 30, 1, DraftKind::Retreat)
            },
        ];
        runner.axis.sort_events();

        runner.dispatch_events(30);
        let requests = runner.take_pending_execution();

        assert_eq!(requests[0].id, "first");
        assert_eq!(requests[1].id, "second");
    }

    #[test]
    fn emergency_stop_clears_pending_proxy_batch() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        runner.proxy.enabled = true;
        runner.pending_execution.push(DraftEvent::new(
            "pending".to_string(),
            0,
            0,
            DraftKind::Skill,
        ));

        runner.emergency_stop();

        assert!(!runner.proxy.enabled);
        assert!(runner.pending_execution.is_empty());
    }

    #[test]
    fn p_recording_keeps_attempt_and_clock_evidence() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        runner.apply_monitor_event(
            observation(1_000_000_000, ObservedBattleState::OneXRunning),
            start,
        );

        runner.record_bookmark().unwrap();

        let event = runner.axis.events.last().unwrap();
        assert_eq!(event.attempt_id.as_deref(), Some("attempt-000001"));
        assert_eq!(event.source_timestamp_ns, Some(1_000_000_000.0));
        assert_eq!(event.frame_range.start, 0);
        assert_eq!(
            runner.recording_attempts[0].event_ids,
            std::slice::from_ref(&event.id)
        );
    }

    #[test]
    fn time_outside_observed_range_requires_explicit_confirmation() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        runner.add_event(30, DraftKind::Skill).unwrap();
        let id = runner.axis.events[0].id.clone();
        runner.move_event(&id, 45).unwrap();

        let error = runner
            .confirm_event_time(ConfirmEventTimeInput {
                id: id.clone(),
                frame: 45,
                manual_correction_confirmed: false,
            })
            .unwrap_err();
        assert_eq!(error.code, "manual_time_confirmation_required");

        runner
            .confirm_event_time(ConfirmEventTimeInput {
                id,
                frame: 45,
                manual_correction_confirmed: true,
            })
            .unwrap();
        assert_eq!(
            runner.axis.events[0].time_confirmation,
            TimeConfirmation::ManuallyCorrected
        );
    }

    #[test]
    fn changing_console_mode_never_arms_proxy() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        runner.proxy.enabled = true;

        runner.set_console_mode(ConsoleMode::RecordingAnalysis);

        assert_eq!(runner.console_mode, ConsoleMode::RecordingAnalysis);
        assert!(!runner.proxy.enabled);
    }

    #[test]
    fn new_recording_axis_uses_empty_base_without_live_attempt() {
        let start = Instant::now();
        let mut runner = RunnerState::new(start);
        runner.add_event(30, DraftKind::Skill).unwrap();
        let input = RecordingMergeInput {
            mode: RecordingMergeMode::NewAxis,
            parent_revision_id: runner.session.current_revision_id.clone(),
            recording_analysis_id: "recording-analysis-000001".to_string(),
            segment_index: 0,
            source_anchor_frame: 60,
            target_anchor_frame: 900,
            offset_frames: 840,
            manual_alignment_confirmed: true,
            conflict_decisions: Vec::new(),
        };

        let (base, attempt_id, created_frame) = runner.recording_merge_context(&input).unwrap();

        assert!(base.events.is_empty());
        assert_eq!(attempt_id, None);
        assert_eq!(created_frame, 900);
    }

    #[test]
    fn continuation_rejects_manual_revision_without_attempt() {
        let start = Instant::now();
        let runner = RunnerState::new(start);
        let input = RecordingMergeInput {
            mode: RecordingMergeMode::Continuation,
            parent_revision_id: runner.session.current_revision_id.clone(),
            recording_analysis_id: "recording-analysis-000001".to_string(),
            segment_index: 0,
            source_anchor_frame: 0,
            target_anchor_frame: 0,
            offset_frames: 0,
            manual_alignment_confirmed: true,
            conflict_decisions: Vec::new(),
        };

        assert_eq!(
            runner.recording_merge_context(&input).unwrap_err().code,
            "recording_merge_parent_ineligible"
        );
    }
}
