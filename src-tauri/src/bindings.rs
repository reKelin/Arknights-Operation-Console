use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;
use thiserror::Error;

use crate::{
    axis::{DraftAxis, DraftDirection, DraftEvent, DraftKind, DraftTile},
    settings::AppSettings,
};

#[derive(Clone, Debug, Error, Serialize, Type)]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
    pub field: Option<String>,
}

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            field: None,
        }
    }

    pub fn field(
        code: impl Into<String>,
        message: impl Into<String>,
        field: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            field: Some(field.into()),
        }
    }
}

impl From<std::io::Error> for CommandError {
    fn from(error: std::io::Error) -> Self {
        Self::new("io_error", error.to_string())
    }
}

impl From<serde_json::Error> for CommandError {
    fn from(error: serde_json::Error) -> Self {
        Self::new("invalid_json", format!("JSON 解析失败：{error}"))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BattleStatus {
    Waiting,
    Running,
    Paused,
    Ended,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RunStrategy {
    Notify,
    Pause,
    DryRun,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum NoticeKind {
    Info,
    Notify,
    DryRun,
    Paused,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunNotice {
    pub sequence: u32,
    pub kind: NoticeKind,
    pub message: String,
    pub event_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AxisMetadataInput {
    pub title: String,
    pub stage_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AddEventInput {
    pub frame: u32,
    pub kind: DraftKind,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEventInput {
    pub id: String,
    pub frame: u32,
    pub kind: DraftKind,
    pub operator: Option<String>,
    pub tile: Option<DraftTile>,
    pub direction: Option<DraftDirection>,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunnerSnapshot {
    pub axis: DraftAxis,
    pub settings: AppSettings,
    pub frame: u32,
    pub time: String,
    pub speed: u8,
    pub status: BattleStatus,
    pub recording: bool,
    pub strategy: RunStrategy,
    pub next_event: Option<DraftEvent>,
    pub countdown_frames: Option<i32>,
    pub error_frames: u16,
    pub last_message: Option<String>,
    pub notices: Vec<RunNotice>,
    pub always_on_top: bool,
    pub clear_pending: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "runnerSnapshot")]
pub struct RunnerSnapshotEvent(pub RunnerSnapshot);
