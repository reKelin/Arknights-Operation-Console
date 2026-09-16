mod geometry;
mod resources;
mod touch;
mod vision;

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    axis::{DraftAxis, DraftEvent, DraftKind},
    monitor::MonitorSnapshot,
    stage::StageCatalog,
};

use geometry::{ProjectionMap, direction_target, project_tile};
use resources::ExecutionResources;
use touch::{ClientSize, TouchInjector};
pub use vision::ExecutionVision;
use vision::locate_operator;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProxyStatus {
    #[default]
    Disabled,
    Confirming,
    Ready,
    Executing,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyExecutionRecord {
    pub sequence: u32,
    pub frame: u32,
    pub event_id: String,
    pub kind: DraftKind,
    pub success: bool,
    pub message: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxySnapshot {
    pub enabled: bool,
    pub status: ProxyStatus,
    pub message: Option<String>,
    pub records: Vec<ProxyExecutionRecord>,
}

pub struct SharedExecutor {
    abort_generation: Arc<AtomicU64>,
    inner: Mutex<ProxyExecutor>,
    vision: Arc<ExecutionVision>,
}

pub struct ProxyBatchResult {
    pub records: Vec<ProxyExecutionRecord>,
    pub error: Option<String>,
}

impl SharedExecutor {
    pub fn new(cache_root: std::path::PathBuf, catalog: Arc<StageCatalog>) -> Result<Self, String> {
        let abort_generation = Arc::new(AtomicU64::new(0));
        let vision = Arc::new(ExecutionVision::new());
        Ok(Self {
            inner: Mutex::new(ProxyExecutor::new(
                cache_root,
                catalog,
                Arc::clone(&abort_generation),
                Arc::clone(&vision),
            )?),
            abort_generation,
            vision,
        })
    }

    pub fn emergency_stop(&self) {
        self.abort_generation.fetch_add(1, Ordering::AcqRel);
        self.vision.set_enabled(false);
    }

    pub fn set_capture_enabled(&self, enabled: bool) {
        self.vision.set_enabled(enabled);
    }

    pub fn vision(&self) -> Arc<ExecutionVision> {
        Arc::clone(&self.vision)
    }

    pub fn prepare(&self, axis: &DraftAxis) -> Result<(), String> {
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
        Ok(())
    }

    pub fn execute(
        &self,
        events: &[DraftEvent],
        frame: u32,
        stage_id: &str,
        window_id: &str,
        monitor: &MonitorSnapshot,
    ) -> ProxyBatchResult {
        match self.inner.lock() {
            Ok(mut executor) => executor.execute(events, frame, stage_id, window_id, monitor),
            Err(_) => ProxyBatchResult {
                records: Vec::new(),
                error: Some("代理执行器状态不可用".to_string()),
            },
        }
    }
}

struct ProxyExecutor {
    resources: ExecutionResources,
    catalog: Arc<StageCatalog>,
    abort_generation: Arc<AtomicU64>,
    vision: Arc<ExecutionVision>,
    touch: TouchInjector,
    next_record: u32,
}

impl ProxyExecutor {
    fn new(
        cache_root: std::path::PathBuf,
        catalog: Arc<StageCatalog>,
        abort_generation: Arc<AtomicU64>,
        vision: Arc<ExecutionVision>,
    ) -> Result<Self, String> {
        Ok(Self {
            resources: ExecutionResources::new(cache_root)?,
            catalog,
            abort_generation,
            vision,
            touch: TouchInjector::new()?,
            next_record: 1,
        })
    }

    fn execute(
        &mut self,
        events: &[DraftEvent],
        frame: u32,
        stage_id: &str,
        window_id: &str,
        monitor: &MonitorSnapshot,
    ) -> ProxyBatchResult {
        let prepared = (|| {
            if !monitor.trusted {
                return Err("监控状态不可信".to_string());
            }
            let (hwnd, client) = TouchInjector::validate_window(window_id)?;
            let captured = self
                .vision
                .latest()
                .ok_or_else(|| "尚未取得代理执行视觉帧".to_string())?;
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
                    records: Vec::new(),
                    error: Some(message),
                };
            }
        };
        let generation = self.abort_generation.load(Ordering::Acquire);
        let abort_generation = Arc::clone(&self.abort_generation);
        let allowed = move || abort_generation.load(Ordering::Acquire) == generation;
        let mut records = Vec::new();
        for event in events {
            let result = self.execute_event(event, &map, &captured, hwnd, client, &allowed);
            records.push(ProxyExecutionRecord {
                sequence: self.next_record,
                frame,
                event_id: event.id.clone(),
                kind: event.kind,
                success: result.is_ok(),
                message: result
                    .as_ref()
                    .map(|_| "代理执行完成".to_string())
                    .unwrap_or_else(|message| message.clone()),
            });
            self.next_record = self.next_record.saturating_add(1);
            if let Err(message) = result {
                self.touch.cancel();
                return ProxyBatchResult {
                    records,
                    error: Some(message),
                };
            }
        }
        ProxyBatchResult {
            records,
            error: None,
        }
    }

    fn execute_event(
        &mut self,
        event: &DraftEvent,
        map: &ProjectionMap,
        captured: &vision::CapturedFrame,
        hwnd: windows::Win32::Foundation::HWND,
        client: ClientSize,
        allowed: &impl Fn() -> bool,
    ) -> Result<(), String> {
        let tile = event
            .tile
            .as_deref()
            .ok_or_else(|| "操作点缺少格子".to_string())?;
        let front = client_point(project_tile(tile, map, false)?, client);
        match event.kind {
            DraftKind::Deploy => {
                let operator = event
                    .operator
                    .as_deref()
                    .ok_or_else(|| "部署操作缺少干员".to_string())?;
                let templates = self.resources.load_avatars(operator)?;
                let avatar = locate_operator(captured, &templates)?;
                let side = client_point(project_tile(tile, map, true)?, client);
                self.touch.drag(hwnd, avatar, side, allowed)?;
                let direction = event
                    .direction
                    .ok_or_else(|| "部署操作缺少朝向".to_string())?;
                let distance = (client.height.min(client.width) as f64 * 0.08) as i32;
                self.touch.drag(
                    hwnd,
                    side,
                    direction_target(side, direction, distance),
                    allowed,
                )
            }
            DraftKind::Skill | DraftKind::Retreat => {
                let pause_left = ratio_point((0.94, 0.07), client);
                let pause_right = ratio_point((0.965, 0.07), client);
                self.touch.tap(hwnd, pause_left, allowed)?;
                self.touch.tap(hwnd, front, allowed)?;
                self.touch.tap(hwnd, pause_right, allowed)?;
                thread::sleep(Duration::from_millis(50));
                let offset = (client.height.min(client.width) as f64 * 0.11) as i32;
                let button = if event.kind == DraftKind::Skill {
                    (front.0 + offset, front.1 + offset)
                } else {
                    (front.0 - offset, front.1 - offset)
                };
                self.touch.tap(hwnd, button, allowed)
            }
        }
    }
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
