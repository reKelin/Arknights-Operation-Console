mod geometry;
mod keyboard;
mod resources;
mod touch;
mod vision;

use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    axis::{DraftAxis, DraftEvent, DraftKind},
    monitor::{ClockQuality, MonitorSnapshot, MonitorSourceKind, ObservedBattleState},
    stage::StageCatalog,
};

use geometry::{ProjectionMap, direction_target, project_tile};
use keyboard::{KeyboardInjector, parse_virtual_key};
use resources::ExecutionResources;
use touch::{ClientSize, TouchInjector};
pub use vision::ExecutionVision;
use vision::{OperatorMatch, changed_ratio, locate_operator, match_operator};

const FRESH_FRAME_MAX_AGE: Duration = Duration::from_millis(350);
const NEXT_FRAME_TIMEOUT: Duration = Duration::from_millis(750);

pub fn validate_key_name(name: &str) -> Result<(), String> {
    parse_virtual_key(name).map(|_| ())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionReceiptStatus {
    Confirmed,
    Uncertain,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PauseProofStatus {
    #[default]
    None,
    Trusted,
    Uncertain,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionReceipt {
    pub run_id: String,
    pub event_id: String,
    pub receipt_sequence: u32,
    pub planned_frame: u32,
    pub observed_frame: Option<u32>,
    pub source_timestamp_ns: Option<f64>,
    pub status: ExecutionReceiptStatus,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionBindings {
    pub pause_key: String,
    pub skill_key: String,
    pub retreat_key: String,
    pub bindings_confirmed: bool,
}

impl Default for ExecutionBindings {
    fn default() -> Self {
        Self {
            pause_key: "Escape".to_string(),
            skill_key: "D".to_string(),
            retreat_key: "A".to_string(),
            bindings_confirmed: false,
        }
    }
}

impl ExecutionBindings {
    pub fn validate(&self) -> Result<(), String> {
        parse_virtual_key(&self.pause_key)?;
        parse_virtual_key(&self.skill_key)?;
        parse_virtual_key(&self.retreat_key)?;
        if self.pause_key.eq_ignore_ascii_case(&self.skill_key)
            || self.pause_key.eq_ignore_ascii_case(&self.retreat_key)
            || self.skill_key.eq_ignore_ascii_case(&self.retreat_key)
        {
            return Err("暂停、技能和撤退必须使用不同键位".to_string());
        }
        Ok(())
    }
}

impl From<&crate::settings::AppSettings> for ExecutionBindings {
    fn from(settings: &crate::settings::AppSettings) -> Self {
        Self {
            pause_key: settings.pause_key.clone(),
            skill_key: settings.skill_key.clone(),
            retreat_key: settings.retreat_key.clone(),
            bindings_confirmed: settings.bindings_confirmed,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProxyStatus {
    #[default]
    Disabled,
    Confirming,
    Armed,
    Ready,
    Pausing,
    Executing,
    WaitingConfirmation,
    Error,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxySnapshot {
    pub enabled: bool,
    pub status: ProxyStatus,
    pub message: Option<String>,
    pub receipts: Vec<ExecutionReceipt>,
    pub stop_reason: Option<String>,
    pub pause_proof: PauseProofStatus,
    pub pause_proof_message: Option<String>,
    pub run_id: Option<String>,
}

pub struct SharedExecutor {
    accepting: AtomicBool,
    abort_generation: Arc<AtomicU64>,
    inner: Arc<Mutex<ProxyExecutor>>,
    vision: Arc<ExecutionVision>,
    worker: Arc<Mutex<WorkerState>>,
    safety: Arc<SharedSafetyState>,
}

pub struct ProxyBatchResult {
    pub kind: ProxyBatchKind,
    pub receipts: Vec<ExecutionReceipt>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProxyBatchKind {
    Action,
    PauseToggle,
    TakeoverPause,
}

pub enum CancelDrain {
    Pending,
    Complete(Option<ProxyBatchResult>),
}

#[derive(Default)]
struct WorkerState {
    busy: bool,
    result: Option<ProxyBatchResult>,
}

#[derive(Clone, Debug, Default)]
struct ExecutionSafetyState {
    monitor: MonitorSnapshot,
    clock_quality: ClockQuality,
    clock_active: bool,
    stage_matched: bool,
    proxy_enabled: bool,
}

#[derive(Default)]
struct SharedSafetyState {
    inner: Mutex<ExecutionSafetyState>,
    changed: Condvar,
}

impl SharedExecutor {
    pub fn new(cache_root: std::path::PathBuf, catalog: Arc<StageCatalog>) -> Result<Self, String> {
        let abort_generation = Arc::new(AtomicU64::new(0));
        let vision = Arc::new(ExecutionVision::new());
        let safety = Arc::new(SharedSafetyState::default());
        Ok(Self {
            accepting: AtomicBool::new(false),
            inner: Arc::new(Mutex::new(ProxyExecutor::new(
                cache_root,
                catalog,
                Arc::clone(&abort_generation),
                Arc::clone(&vision),
                Arc::clone(&safety),
            )?)),
            abort_generation,
            vision,
            worker: Arc::new(Mutex::new(WorkerState::default())),
            safety,
        })
    }

    pub fn emergency_stop(&self) {
        self.accepting.store(false, Ordering::Release);
        self.abort_generation.fetch_add(1, Ordering::AcqRel);
        self.vision.set_enabled(false);
    }

    pub fn cancel_and_drain(&self) -> CancelDrain {
        self.emergency_stop();
        let mut worker = match self.worker.lock() {
            Ok(worker) => worker,
            Err(poisoned) => poisoned.into_inner(),
        };
        if worker.busy {
            CancelDrain::Pending
        } else {
            CancelDrain::Complete(worker.result.take())
        }
    }

    pub fn set_capture_enabled(&self, enabled: bool) {
        self.vision.set_enabled(enabled);
    }

    pub fn vision(&self) -> Arc<ExecutionVision> {
        Arc::clone(&self.vision)
    }

    pub fn prepare(&self, axis: &DraftAxis) -> Result<(), String> {
        let generation = self.abort_generation.load(Ordering::Acquire);
        let stage_id = axis
            .stage_id
            .as_deref()
            .ok_or_else(|| "当前轴没有关卡".to_string())?;
        let executor = self
            .inner
            .lock()
            .map_err(|_| "代理执行器状态不可用".to_string())?;
        executor
            .resources
            .load_projection(&executor.catalog, stage_id)?;
        let mut loaded = std::collections::HashSet::new();
        for operator in axis
            .events
            .iter()
            .filter(|event| event.kind == DraftKind::Deploy)
            .filter_map(|event| event.operator.as_deref())
        {
            if loaded.insert(operator) {
                executor.resources.load_avatars(operator)?;
            }
        }
        self.accepting.store(true, Ordering::Release);
        if self.abort_generation.load(Ordering::Acquire) != generation {
            self.accepting.store(false, Ordering::Release);
            return Err("代理准备已取消".to_string());
        }
        Ok(())
    }

    pub fn submit(
        &self,
        events: &[DraftEvent],
        run_id: &str,
        frame: u32,
        stage_id: &str,
        window_id: &str,
        monitor: &MonitorSnapshot,
    ) -> Result<(), String> {
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| "代理执行任务状态不可用".to_string())?;
        let generation = self.abort_generation.load(Ordering::Acquire);
        if !self.accepting.load(Ordering::Acquire) {
            return Err("代理执行已取消，拒绝启动新的事务".to_string());
        }
        if worker.busy {
            return Err("已有代理执行事务正在运行".to_string());
        }
        worker.busy = true;
        worker.result = None;
        drop(worker);

        let inner = Arc::clone(&self.inner);
        let worker = Arc::clone(&self.worker);
        let events = events.to_vec();
        let run_id = run_id.to_string();
        let stage_id = stage_id.to_string();
        let window_id = window_id.to_string();
        let monitor = monitor.clone();
        thread::Builder::new()
            .name("paused-execution".to_string())
            .spawn(move || {
                let result = match inner.lock() {
                    Ok(mut executor) => executor.execute(
                        &events, &run_id, frame, &stage_id, &window_id, &monitor, generation,
                    ),
                    Err(_) => ProxyBatchResult {
                        kind: ProxyBatchKind::Action,
                        receipts: Vec::new(),
                        error: Some("代理执行器状态不可用".to_string()),
                    },
                };
                if let Ok(mut state) = worker.lock() {
                    state.busy = false;
                    state.result = Some(result);
                }
            })
            .map_err(|error| {
                if let Ok(mut worker) = self.worker.lock() {
                    worker.busy = false;
                }
                format!("启动代理执行线程失败：{error}")
            })?;
        Ok(())
    }

    pub fn submit_pause_toggle(
        &self,
        window_id: &str,
        monitor: &MonitorSnapshot,
    ) -> Result<(), String> {
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| "代理执行任务状态不可用".to_string())?;
        let generation = self.abort_generation.load(Ordering::Acquire);
        if !self.accepting.load(Ordering::Acquire) {
            return Err("代理执行已取消，拒绝启动新的暂停事务".to_string());
        }
        if worker.busy {
            return Err("已有代理执行事务正在运行".to_string());
        }
        worker.busy = true;
        worker.result = None;
        drop(worker);

        let inner = Arc::clone(&self.inner);
        let worker = Arc::clone(&self.worker);
        let window_id = window_id.to_string();
        let monitor = monitor.clone();
        thread::Builder::new()
            .name("pause-toggle".to_string())
            .spawn(move || {
                let result = match inner.lock() {
                    Ok(mut executor) => executor.pause_toggle(&window_id, &monitor, generation),
                    Err(_) => ProxyBatchResult {
                        kind: ProxyBatchKind::PauseToggle,
                        receipts: Vec::new(),
                        error: Some("代理执行器状态不可用".to_string()),
                    },
                };
                if let Ok(mut state) = worker.lock() {
                    state.busy = false;
                    state.result = Some(result);
                }
            })
            .map_err(|error| {
                if let Ok(mut worker) = self.worker.lock() {
                    worker.busy = false;
                }
                format!("启动暂停事务线程失败：{error}")
            })?;
        Ok(())
    }

    pub fn submit_takeover_pause(
        &self,
        window_id: &str,
        monitor: &MonitorSnapshot,
    ) -> Result<(), String> {
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| "代理执行任务状态不可用".to_string())?;
        if worker.busy || worker.result.is_some() {
            return Err("旧代理事务尚未完成，不能确认接管暂停".to_string());
        }
        let generation = self.abort_generation.load(Ordering::Acquire);
        worker.busy = true;
        drop(worker);

        self.vision.set_enabled(true);
        let inner = Arc::clone(&self.inner);
        let worker = Arc::clone(&self.worker);
        let window_id = window_id.to_string();
        let monitor = monitor.clone();
        thread::Builder::new()
            .name("takeover-pause".to_string())
            .spawn(move || {
                let result = match inner.lock() {
                    Ok(mut executor) => {
                        executor.ensure_takeover_paused(&window_id, &monitor, generation)
                    }
                    Err(_) => ProxyBatchResult {
                        kind: ProxyBatchKind::TakeoverPause,
                        receipts: Vec::new(),
                        error: Some("代理执行器状态不可用".to_string()),
                    },
                };
                if let Ok(mut state) = worker.lock() {
                    state.busy = false;
                    state.result = Some(result);
                }
            })
            .map_err(|error| {
                if let Ok(mut worker) = self.worker.lock() {
                    worker.busy = false;
                }
                format!("启动接管暂停确认线程失败：{error}")
            })?;
        Ok(())
    }

    pub fn take_result(&self) -> Option<ProxyBatchResult> {
        self.worker.lock().ok()?.result.take()
    }

    pub fn configure_bindings(&self, bindings: ExecutionBindings) -> Result<(), String> {
        bindings.validate()?;
        let mut executor = self
            .inner
            .lock()
            .map_err(|_| "代理执行器状态不可用".to_string())?;
        executor.bindings = bindings;
        Ok(())
    }

    pub fn update_safety(
        &self,
        monitor: &MonitorSnapshot,
        clock_quality: ClockQuality,
        clock_active: bool,
        stage_matched: bool,
        proxy_enabled: bool,
    ) {
        if let Ok(mut safety) = self.safety.inner.lock() {
            *safety = ExecutionSafetyState {
                monitor: monitor.clone(),
                clock_quality,
                clock_active,
                stage_matched,
                proxy_enabled,
            };
            self.safety.changed.notify_all();
        }
    }
}

struct ProxyExecutor {
    resources: ExecutionResources,
    catalog: Arc<StageCatalog>,
    abort_generation: Arc<AtomicU64>,
    vision: Arc<ExecutionVision>,
    touch: TouchInjector,
    keyboard: KeyboardInjector,
    bindings: ExecutionBindings,
    next_record: u32,
    safety: Arc<SharedSafetyState>,
}

impl ProxyExecutor {
    fn new(
        cache_root: std::path::PathBuf,
        catalog: Arc<StageCatalog>,
        abort_generation: Arc<AtomicU64>,
        vision: Arc<ExecutionVision>,
        safety: Arc<SharedSafetyState>,
    ) -> Result<Self, String> {
        Ok(Self {
            resources: ExecutionResources::new(cache_root)?,
            catalog,
            abort_generation,
            vision,
            touch: TouchInjector::new()?,
            keyboard: KeyboardInjector::new(),
            bindings: ExecutionBindings::default(),
            next_record: 1,
            safety,
        })
    }

    fn execute(
        &mut self,
        events: &[DraftEvent],
        run_id: &str,
        frame: u32,
        stage_id: &str,
        window_id: &str,
        monitor: &MonitorSnapshot,
        generation: u64,
    ) -> ProxyBatchResult {
        if events.iter().any(|event| event.frame != frame) {
            let mut receipts = Vec::with_capacity(events.len());
            for (index, event) in events.iter().enumerate() {
                receipts.push(ExecutionReceipt {
                    run_id: run_id.to_string(),
                    event_id: event.id.clone(),
                    receipt_sequence: self.next_record,
                    planned_frame: event.frame,
                    observed_frame: Some(frame),
                    source_timestamp_ns: monitor.last_source_timestamp_ns,
                    status: if index == 0 {
                        ExecutionReceiptStatus::Failed
                    } else {
                        ExecutionReceiptStatus::Cancelled
                    },
                    reason: if index == 0 {
                        format!("目标 F{} 已跨过，当前为 F{frame}", event.frame)
                    } else {
                        "同帧前序操作已错过，未发送输入".to_string()
                    },
                });
                self.next_record = self.next_record.saturating_add(1);
            }
            return ProxyBatchResult {
                kind: ProxyBatchKind::Action,
                receipts,
                error: Some("目标帧已跨过，代理已停止且不会补发".to_string()),
            };
        }
        let prepared = (|| {
            if !monitor.trusted {
                return Err("监控状态不可信".to_string());
            }
            if monitor.battle_state != crate::monitor::ObservedBattleState::Paused {
                return Err("代理操作只能从可信暂停状态开始".to_string());
            }
            if monitor.cost_full || monitor.cost_phase.is_none() {
                return Err("满费或费用不可见时无法证明暂停事务未推进".to_string());
            }
            let (hwnd, client) = TouchInjector::validate_window(window_id)?;
            let captured = self
                .vision
                .latest()
                .ok_or_else(|| "尚未取得代理执行视觉帧".to_string())?;
            if !captured.is_fresh(FRESH_FRAME_MAX_AGE) {
                return Err("代理执行视觉帧已过期".to_string());
            }
            if (captured.width as i32 - client.width).abs() > 2
                || (captured.height as i32 - client.height).abs() > 2
            {
                return Err("游戏窗口尺寸已变化".to_string());
            }
            let (_, map) = self.resources.load_projection(&self.catalog, stage_id)?;
            Ok((hwnd, client, captured, map))
        })();
        let (hwnd, client, captured, map) = match prepared {
            Ok(prepared) => prepared,
            Err(message) => {
                return ProxyBatchResult {
                    kind: ProxyBatchKind::Action,
                    receipts: Vec::new(),
                    error: Some(message),
                };
            }
        };
        let abort_generation = Arc::clone(&self.abort_generation);
        let allowed = move || abort_generation.load(Ordering::Acquire) == generation;
        let mut receipts = Vec::new();
        for (event_index, event) in events.iter().enumerate() {
            let event_frame = self
                .vision
                .latest()
                .filter(|latest| latest.sequence >= captured.sequence)
                .unwrap_or_else(|| captured.clone());
            let result = self.execute_event(
                event,
                EventContext {
                    map: &map,
                    captured: &event_frame,
                    hwnd,
                    client,
                    window_id,
                },
                &allowed,
            );
            let receipt_status =
                result
                    .as_ref()
                    .map(|result| result.status)
                    .unwrap_or_else(|message| {
                        if message.contains("输入结果未知") {
                            ExecutionReceiptStatus::Uncertain
                        } else if message.contains("取消") || message.contains("急停") {
                            ExecutionReceiptStatus::Cancelled
                        } else {
                            ExecutionReceiptStatus::Failed
                        }
                    });
            let reason = result
                .as_ref()
                .map(|result| result.reason.clone())
                .unwrap_or_else(|message| message.clone());
            let source_timestamp_ns = result
                .as_ref()
                .ok()
                .map(|result| result.source_timestamp_ns as f64)
                .or(Some(event_frame.source_timestamp_ns as f64));
            receipts.push(ExecutionReceipt {
                run_id: run_id.to_string(),
                event_id: event.id.clone(),
                receipt_sequence: self.next_record,
                planned_frame: event.frame,
                observed_frame: Some(frame),
                source_timestamp_ns,
                status: receipt_status,
                reason: reason.clone(),
            });
            self.next_record = self.next_record.saturating_add(1);
            if receipt_status != ExecutionReceiptStatus::Confirmed {
                self.touch.cancel();
                self.keyboard.cancel();
                for remaining in &events[event_index + 1..] {
                    receipts.push(ExecutionReceipt {
                        run_id: run_id.to_string(),
                        event_id: remaining.id.clone(),
                        receipt_sequence: self.next_record,
                        planned_frame: remaining.frame,
                        observed_frame: Some(frame),
                        source_timestamp_ns: Some(event_frame.source_timestamp_ns as f64),
                        status: ExecutionReceiptStatus::Cancelled,
                        reason: "同帧前序操作未确认，未发送输入".to_string(),
                    });
                    self.next_record = self.next_record.saturating_add(1);
                }
                return ProxyBatchResult {
                    kind: ProxyBatchKind::Action,
                    receipts,
                    error: Some(reason),
                };
            }
        }
        ProxyBatchResult {
            kind: ProxyBatchKind::Action,
            receipts,
            error: None,
        }
    }

    fn pause_toggle(
        &mut self,
        window_id: &str,
        monitor: &MonitorSnapshot,
        generation: u64,
    ) -> ProxyBatchResult {
        let result = (|| {
            self.validate_dynamic_safety(window_id, false)?;
            if !monitor.trusted {
                return Err("监控状态不可信".to_string());
            }
            if !self.bindings.bindings_confirmed {
                return Err("发送暂停键前必须确认游戏键位".to_string());
            }
            let (_, client) = TouchInjector::validate_window(window_id)?;
            let frame = self
                .vision
                .latest()
                .ok_or_else(|| "尚未取得代理执行视觉帧".to_string())?;
            if !frame.is_fresh(FRESH_FRAME_MAX_AGE) {
                return Err("代理执行视觉帧已过期".to_string());
            }
            if frame.width.abs_diff(client.width as u32) > 2
                || frame.height.abs_diff(client.height as u32) > 2
            {
                return Err("游戏窗口尺寸已变化".to_string());
            }
            let abort_generation = Arc::clone(&self.abort_generation);
            let key = parse_virtual_key(&self.bindings.pause_key)?;
            self.keyboard.press(key, || {
                abort_generation.load(Ordering::Acquire) == generation
            })
        })();
        ProxyBatchResult {
            kind: ProxyBatchKind::PauseToggle,
            receipts: Vec::new(),
            error: result.err(),
        }
    }

    fn ensure_takeover_paused(
        &mut self,
        window_id: &str,
        monitor: &MonitorSnapshot,
        generation: u64,
    ) -> ProxyBatchResult {
        let result = (|| {
            if !self.bindings.bindings_confirmed {
                return Err("接管暂停前必须确认游戏键位".to_string());
            }
            if monitor.source_kind != MonitorSourceKind::Window
                || monitor.window_id.as_deref() != Some(window_id)
                || !monitor.trusted
            {
                return Err("接管时窗口或监控状态不可信，未发送暂停键".to_string());
            }
            {
                let safety = self
                    .safety
                    .inner
                    .lock()
                    .map_err(|_| "执行安全状态不可用".to_string())?;
                if !safety.stage_matched || !safety.clock_active {
                    return Err("接管时关卡或时钟状态不可信，未发送暂停键".to_string());
                }
            }
            let (_, client) = TouchInjector::validate_window(window_id)?;
            let frame = self
                .vision
                .latest()
                .or_else(|| self.vision.wait_after(0, NEXT_FRAME_TIMEOUT))
                .ok_or_else(|| "接管时没有新鲜游戏画面，未发送暂停键".to_string())?;
            validate_frame_size(&frame, client)?;

            if monitor.battle_state == ObservedBattleState::Paused {
                return Ok(());
            }
            if !matches!(
                monitor.battle_state,
                ObservedBattleState::OneXRunning
                    | ObservedBattleState::TwoXRunning
                    | ObservedBattleState::PointTwoXRunning
            ) {
                return Err("接管时无法确认游戏正在推进，未发送暂停键".to_string());
            }

            let abort_generation = Arc::clone(&self.abort_generation);
            let allowed = || abort_generation.load(Ordering::Acquire) == generation;
            let key = parse_virtual_key(&self.bindings.pause_key)?;
            self.keyboard.press(key, &allowed)?;

            let deadline = Instant::now() + NEXT_FRAME_TIMEOUT;
            let mut seen_sequence = monitor.last_event_sequence.unwrap_or(0.0);
            let mut safety = self
                .safety
                .inner
                .lock()
                .map_err(|_| "执行安全状态不可用".to_string())?;
            loop {
                if !allowed() {
                    return Err("接管暂停已取消（输入结果未知）".to_string());
                }
                if safety
                    .monitor
                    .last_event_sequence
                    .is_some_and(|sequence| sequence > seen_sequence)
                {
                    let sequence = safety.monitor.last_event_sequence.unwrap_or(seen_sequence);
                    seen_sequence = sequence;
                    if !safety.stage_matched
                        || !safety.clock_active
                        || safety.monitor.source_kind != MonitorSourceKind::Window
                        || safety.monitor.window_id.as_deref() != Some(window_id)
                        || !safety.monitor.trusted
                    {
                        return Err("接管暂停后的窗口、关卡或监控状态不可信".to_string());
                    }
                    match safety.monitor.battle_state {
                        ObservedBattleState::Paused => {
                            drop(safety);
                            let (_, current) = TouchInjector::validate_window(window_id)?;
                            if current != client {
                                return Err("接管暂停时游戏窗口尺寸已变化".to_string());
                            }
                            let confirmed = self
                                .vision
                                .latest()
                                .ok_or_else(|| "接管暂停确认缺少对应的新鲜画面".to_string())?;
                            validate_frame_size(&confirmed, client)?;
                            return Ok(());
                        }
                        ObservedBattleState::OneXRunning
                        | ObservedBattleState::TwoXRunning
                        | ObservedBattleState::PointTwoXRunning => {}
                        _ => return Err("暂停键发送后未能确认游戏已暂停".to_string()),
                    }
                }

                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err("等待接管暂停确认超时".to_string());
                }
                let (next, wait) = self
                    .safety
                    .changed
                    .wait_timeout(safety, remaining)
                    .map_err(|_| "执行安全状态不可用".to_string())?;
                safety = next;
                if wait.timed_out() {
                    return Err("等待接管暂停确认超时".to_string());
                }
            }
        })();
        ProxyBatchResult {
            kind: ProxyBatchKind::TakeoverPause,
            receipts: Vec::new(),
            error: result.err(),
        }
    }

    fn execute_event(
        &mut self,
        event: &DraftEvent,
        context: EventContext<'_>,
        allowed: &impl Fn() -> bool,
    ) -> Result<ActionResult, String> {
        let EventContext {
            map,
            captured,
            hwnd,
            client,
            window_id,
        } = context;
        let tile = event
            .tile
            .as_deref()
            .ok_or_else(|| "操作点缺少格子".to_string())?;
        let front = client_point(project_tile(tile, map, false)?, client);
        match event.kind {
            DraftKind::Bookmark => Err("未分类书签不能代理执行".to_string()),
            DraftKind::Deploy => {
                let operator = event
                    .operator
                    .as_deref()
                    .ok_or_else(|| "部署操作缺少干员".to_string())?;
                let templates = self.resources.load_avatars(operator)?;
                let avatar = locate_operator(captured, &templates)?;
                let side = client_point(project_tile(tile, map, true)?, client);
                self.touch.drag(hwnd, avatar, side, allowed)?;
                let after_drop =
                    self.fresh_frame_after(captured.sequence, client, window_id, allowed)?;
                let direction = event
                    .direction
                    .ok_or_else(|| "部署操作缺少朝向".to_string())?;
                let distance = (client.height.min(client.width) as f64 * 0.08) as i32;
                self.touch.drag(
                    hwnd,
                    side,
                    direction_target(side, direction, distance),
                    allowed,
                )?;
                let after =
                    self.fresh_frame_after(after_drop.sequence, client, window_id, allowed)?;
                let target_changed = changed_ratio(captured, &after, front, result_radius(client))?;
                let match_result = match_operator(&after, &templates)?;
                if target_changed >= 0.08 && match_result == OperatorMatch::Absent {
                    Ok(ActionResult::confirmed(
                        after.source_timestamp_ns,
                        "部署结果已由部署栏和目标格子变化确认",
                    ))
                } else {
                    Ok(ActionResult::uncertain(
                        after.source_timestamp_ns,
                        format!(
                            "部署输入已发送，但结果证据不足（格子变化 {:.0}%，部署栏 {:?}）",
                            target_changed * 100.0,
                            match_result
                        ),
                    ))
                }
            }
            DraftKind::Skill | DraftKind::Retreat => {
                if !self.bindings.bindings_confirmed {
                    return Err("首次执行技能或撤退前必须确认游戏键位".to_string());
                }
                self.touch.tap(hwnd, front, allowed)?;
                let selected =
                    self.fresh_frame_after(captured.sequence, client, window_id, allowed)?;
                let key = if event.kind == DraftKind::Skill {
                    parse_virtual_key(&self.bindings.skill_key)?
                } else {
                    parse_virtual_key(&self.bindings.retreat_key)?
                };
                self.keyboard.press(key, allowed)?;
                let after =
                    self.fresh_frame_after(selected.sequence, client, window_id, allowed)?;
                let next = self.fresh_frame_after(after.sequence, client, window_id, allowed)?;
                let changed = changed_ratio(&selected, &after, front, result_radius(client))?;
                let stable = changed_ratio(&after, &next, front, result_radius(client))?;
                Ok(ActionResult::uncertain(
                    next.source_timestamp_ns,
                    format!(
                        "键盘输入已发送；当前视觉只能确认选中区间，不能区分技能与撤退结果（变化 {:.0}%，后续变化 {:.0}%），请人工确认",
                        changed * 100.0,
                        stable * 100.0
                    ),
                ))
            }
        }
    }

    fn fresh_frame_after(
        &self,
        sequence: u64,
        client: ClientSize,
        window_id: &str,
        allowed: &impl Fn() -> bool,
    ) -> Result<vision::CapturedFrame, String> {
        if !allowed() {
            return Err("代理执行已取消（输入结果未知）".to_string());
        }
        self.validate_dynamic_safety(window_id, true)?;
        let (_, current) = TouchInjector::validate_window(window_id)?;
        if current != client {
            return Err("游戏窗口尺寸已变化".to_string());
        }
        let frame = match self.vision.wait_after(sequence, NEXT_FRAME_TIMEOUT) {
            Some(frame) => frame,
            None if !allowed() => {
                return Err("代理执行已取消（输入结果未知）".to_string());
            }
            None => return Err("等待新鲜代理执行画面超时".to_string()),
        };
        if !frame.is_fresh(FRESH_FRAME_MAX_AGE) {
            return Err("代理执行视觉帧已过期".to_string());
        }
        if !allowed() {
            return Err("代理执行已取消（输入结果未知）".to_string());
        }
        self.validate_dynamic_safety(window_id, true)?;
        if frame.width.abs_diff(client.width as u32) > 2
            || frame.height.abs_diff(client.height as u32) > 2
        {
            return Err("游戏窗口尺寸已变化".to_string());
        }
        Ok(frame)
    }

    fn validate_dynamic_safety(
        &self,
        window_id: &str,
        transaction_in_progress: bool,
    ) -> Result<(), String> {
        let safety = self
            .safety
            .inner
            .lock()
            .map_err(|_| "执行安全状态不可用".to_string())?;
        if !safety.proxy_enabled
            || !safety.stage_matched
            || !safety.clock_active
            || safety.monitor.source_kind != MonitorSourceKind::Window
            || safety.monitor.window_id.as_deref() != Some(window_id)
            || !safety.monitor.trusted
        {
            return Err("窗口、关卡或监控安全状态已变化".to_string());
        }
        if safety.clock_quality == ClockQuality::Trusted {
            return Ok(());
        }
        if transaction_in_progress
            && safety.clock_quality == ClockQuality::Uncertain
            && matches!(
                safety.monitor.battle_state,
                ObservedBattleState::Paused
                    | ObservedBattleState::PointTwoXRunning
                    | ObservedBattleState::DeployingOperator
                    | ObservedBattleState::AdjustingOperatorFacing
            )
        {
            return Ok(());
        }
        Err("可信时钟或暂停事务状态已变化".to_string())
    }
}

struct EventContext<'a> {
    map: &'a ProjectionMap,
    captured: &'a vision::CapturedFrame,
    hwnd: windows::Win32::Foundation::HWND,
    client: ClientSize,
    window_id: &'a str,
}

struct ActionResult {
    status: ExecutionReceiptStatus,
    source_timestamp_ns: u64,
    reason: String,
}

impl ActionResult {
    fn confirmed(source_timestamp_ns: u64, reason: impl Into<String>) -> Self {
        Self {
            status: ExecutionReceiptStatus::Confirmed,
            source_timestamp_ns,
            reason: reason.into(),
        }
    }

    fn uncertain(source_timestamp_ns: u64, reason: impl Into<String>) -> Self {
        Self {
            status: ExecutionReceiptStatus::Uncertain,
            source_timestamp_ns,
            reason: reason.into(),
        }
    }
}

fn validate_frame_size(frame: &vision::CapturedFrame, client: ClientSize) -> Result<(), String> {
    if !frame.is_fresh(FRESH_FRAME_MAX_AGE) {
        return Err("代理执行视觉帧已过期".to_string());
    }
    if frame.width.abs_diff(client.width as u32) > 2
        || frame.height.abs_diff(client.height as u32) > 2
    {
        return Err("游戏窗口尺寸已变化".to_string());
    }
    Ok(())
}

fn result_radius(client: ClientSize) -> u32 {
    (client.width.min(client.height) as f64 * 0.06).round() as u32
}

fn client_point(point: (f64, f64), client: ClientSize) -> (i32, i32) {
    ratio_point(point, client)
}

fn ratio_point(point: (f64, f64), client: ClientSize) -> (i32, i32) {
    let scale = (f64::from(client.width) / 1280.0).min(f64::from(client.height) / 720.0);
    let width = 1280.0 * scale;
    let height = 720.0 * scale;
    let offset_x = (f64::from(client.width) - width) / 2.0;
    let offset_y = (f64::from(client.height) - height) / 2.0;
    (
        (offset_x + point.0.clamp(0.0, 1.0) * width) as i32,
        (offset_y + point.1.clamp(0.0, 1.0) * height) as i32,
    )
}
