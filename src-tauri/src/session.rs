use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    axis::{DraftAxis, DraftEvent},
    bindings::CommandError,
    executor::{ExecutionReceipt, ExecutionReceiptStatus},
    monitor::ClockAnchor,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AxisRevisionSource {
    Imported,
    Manual,
    Takeover,
    RecordingMerge,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TakeoverRevisionProvenance {
    pub run_id: String,
    pub receipt_sequences: Vec<u32>,
    pub uncertain_receipt_sequence: Option<u32>,
    pub uncertain_event_id: Option<String>,
    pub time_trusted: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordingMergeProvenance {
    pub recording_analysis_id: String,
    pub segment_index: u32,
    pub frame_offset: i32,
    pub candidate_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AxisRevision {
    pub id: String,
    pub sequence: u32,
    pub parent_revision_id: Option<String>,
    pub source: AxisRevisionSource,
    pub attempt_id: Option<String>,
    pub created_frame: u32,
    pub axis: DraftAxis,
    pub takeover: Option<TakeoverRevisionProvenance>,
    pub recording_merge: Option<RecordingMergeProvenance>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TakeoverStatus {
    #[default]
    Idle,
    Cancelling,
    AwaitingPauseProof,
    Recording,
    Unknown,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TakeoverState {
    pub status: TakeoverStatus,
    pub generation: u32,
    pub base_revision_id: Option<String>,
    pub new_revision_id: Option<String>,
    pub requested_source_timestamp_ns: Option<f64>,
    pub inherited_anchor: Option<ClockAnchor>,
    pub uncertain_receipt_sequences: Vec<u32>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperationSession {
    pub id: String,
    pub revisions: Vec<AxisRevision>,
    pub current_revision_id: String,
    pub active_recording_revision_id: String,
    pub armed_revision_id: Option<String>,
    pub takeover: TakeoverState,
    #[serde(skip)]
    #[specta(skip)]
    next_revision_sequence: u32,
}

impl OperationSession {
    pub fn new(id: String, axis: DraftAxis, source: AxisRevisionSource) -> Self {
        let revision = AxisRevision {
            id: "revision-000001".to_string(),
            sequence: 1,
            parent_revision_id: None,
            source,
            attempt_id: None,
            created_frame: 0,
            axis,
            takeover: None,
            recording_merge: None,
        };
        Self {
            id,
            revisions: vec![revision],
            current_revision_id: "revision-000001".to_string(),
            active_recording_revision_id: "revision-000001".to_string(),
            armed_revision_id: None,
            takeover: TakeoverState::default(),
            next_revision_sequence: 2,
        }
    }

    pub fn current_revision(&self) -> &AxisRevision {
        self.revision(&self.current_revision_id)
            .expect("current revision must remain in the session")
    }

    pub fn current_axis(&self) -> &DraftAxis {
        &self.current_revision().axis
    }

    pub fn active_revision_is_selected(&self) -> bool {
        self.current_revision_id == self.active_recording_revision_id
    }

    pub fn active_axis(&self) -> &DraftAxis {
        &self
            .revision(&self.active_recording_revision_id)
            .expect("active recording revision must remain in the session")
            .axis
    }

    fn active_axis_mut(&mut self) -> &mut DraftAxis {
        let id = self.active_recording_revision_id.clone();
        &mut self
            .revision_mut(&id)
            .expect("active recording revision must remain in the session")
            .axis
    }

    pub fn revision(&self, id: &str) -> Option<&AxisRevision> {
        self.revisions.iter().find(|revision| revision.id == id)
    }

    pub fn select_revision(&mut self, id: &str) -> Result<(), CommandError> {
        if self.revision(id).is_none() {
            return Err(CommandError::new("revision_not_found", "未找到轴版本"));
        }
        self.current_revision_id = id.to_string();
        Ok(())
    }

    pub fn sync_active_axis(&mut self, axis: &DraftAxis) -> Result<(), CommandError> {
        if !self.active_revision_is_selected() {
            return Err(CommandError::new(
                "revision_read_only",
                "旧轴版本为只读；请切回当前续录版本",
            ));
        }
        *self.active_axis_mut() = axis.clone();
        Ok(())
    }

    pub fn create_imported_revision(
        &mut self,
        parent_revision_id: &str,
        created_frame: u32,
        axis: DraftAxis,
    ) -> Result<String, CommandError> {
        let id = self.create_revision(
            parent_revision_id,
            AxisRevisionSource::Imported,
            None,
            created_frame,
            axis,
            None,
            None,
        )?;
        self.takeover = TakeoverState::default();
        Ok(id)
    }

    #[expect(dead_code, reason = "PR G consumes the recording merge boundary")]
    pub fn create_recording_merge_revision(
        &mut self,
        parent_revision_id: &str,
        attempt_id: Option<String>,
        created_frame: u32,
        axis: DraftAxis,
        provenance: RecordingMergeProvenance,
    ) -> Result<String, CommandError> {
        if provenance.recording_analysis_id.trim().is_empty() || provenance.candidate_ids.is_empty()
        {
            return Err(CommandError::new(
                "recording_merge_empty",
                "录屏合并版本必须保留分析 ID 和至少一个候选来源",
            ));
        }
        let unique_candidates: HashSet<&str> = provenance
            .candidate_ids
            .iter()
            .map(String::as_str)
            .collect();
        if unique_candidates.len() != provenance.candidate_ids.len() {
            return Err(CommandError::new(
                "recording_merge_duplicate",
                "录屏合并版本包含重复候选来源",
            ));
        }
        let id = self.create_revision(
            parent_revision_id,
            AxisRevisionSource::RecordingMerge,
            attempt_id,
            created_frame,
            axis,
            None,
            Some(provenance),
        )?;
        self.takeover = TakeoverState::default();
        Ok(id)
    }

    pub fn create_takeover_revision(
        &mut self,
        parent_revision_id: &str,
        run_id: &str,
        attempt_id: Option<String>,
        created_frame: u32,
        receipts: &[ExecutionReceipt],
    ) -> Result<String, CommandError> {
        validate_receipts(run_id, receipts)?;
        let parent = self
            .revision(parent_revision_id)
            .ok_or_else(|| CommandError::new("revision_not_found", "未找到接管前的轴版本"))?;
        let mut events = Vec::new();
        let mut receipt_sequences = Vec::new();
        let mut uncertain_receipt_sequence = None;
        let mut uncertain_event_id = None;
        for receipt in receipts {
            let source = parent
                .axis
                .events
                .iter()
                .find(|event| event.id == receipt.event_id)
                .ok_or_else(|| {
                    CommandError::new(
                        "receipt_event_not_found",
                        format!("执行回执无法映射操作点 {}", receipt.event_id),
                    )
                })?;
            match receipt.status {
                ExecutionReceiptStatus::Confirmed => {
                    push_prefix_event(&mut events, source);
                    receipt_sequences.push(receipt.receipt_sequence);
                }
                ExecutionReceiptStatus::Uncertain => {
                    push_prefix_event(&mut events, source);
                    receipt_sequences.push(receipt.receipt_sequence);
                    uncertain_receipt_sequence = Some(receipt.receipt_sequence);
                    uncertain_event_id = Some(receipt.event_id.clone());
                    break;
                }
                ExecutionReceiptStatus::Failed | ExecutionReceiptStatus::Cancelled => break,
            }
        }
        let mut axis = DraftAxis {
            title: parent.axis.title.clone(),
            stage_id: parent.axis.stage_id.clone(),
            events,
        };
        axis.sort_events();
        self.create_revision(
            parent_revision_id,
            AxisRevisionSource::Takeover,
            attempt_id,
            created_frame,
            axis,
            Some(TakeoverRevisionProvenance {
                run_id: run_id.to_string(),
                receipt_sequences,
                uncertain_receipt_sequence,
                uncertain_event_id,
                time_trusted: false,
            }),
            None,
        )
    }

    pub fn arm_revision(&mut self, id: &str) -> Result<(), CommandError> {
        let revision = self
            .revision(id)
            .ok_or_else(|| CommandError::new("revision_not_found", "未找到要武装的轴版本"))?;
        revision.axis_json_for_use()?;
        self.armed_revision_id = Some(id.to_string());
        Ok(())
    }

    pub fn disarm(&mut self) {
        self.armed_revision_id = None;
    }

    pub fn resolve_uncertain_receipt(
        &mut self,
        receipt_sequence: u32,
        confirmed: bool,
    ) -> Result<(), CommandError> {
        let active_id = self.active_recording_revision_id.clone();
        let revision = self
            .revision_mut(&active_id)
            .ok_or_else(|| CommandError::new("revision_not_found", "未找到当前续录版本"))?;
        let provenance = revision.takeover.as_mut().ok_or_else(|| {
            CommandError::new("takeover_receipt_not_found", "当前版本没有接管待确认回执")
        })?;
        if provenance.uncertain_receipt_sequence != Some(receipt_sequence) {
            return Err(CommandError::new(
                "takeover_receipt_not_found",
                "待确认回执不属于当前接管版本",
            ));
        }
        if !confirmed {
            if let Some(event_id) = provenance.uncertain_event_id.as_deref() {
                revision.axis.events.retain(|event| event.id != event_id);
            }
            provenance
                .receipt_sequences
                .retain(|sequence| *sequence != receipt_sequence);
        }
        provenance.uncertain_receipt_sequence = None;
        provenance.uncertain_event_id = None;
        Ok(())
    }

    pub fn confirm_takeover_time(&mut self, revision_id: &str) -> Result<(), CommandError> {
        let revision = self
            .revision_mut(revision_id)
            .ok_or_else(|| CommandError::new("revision_not_found", "未找到接管轴版本"))?;
        let provenance = revision.takeover.as_mut().ok_or_else(|| {
            CommandError::new("takeover_revision_required", "当前版本不是接管轴版本")
        })?;
        provenance.time_trusted = true;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn create_revision(
        &mut self,
        parent_revision_id: &str,
        source: AxisRevisionSource,
        attempt_id: Option<String>,
        created_frame: u32,
        axis: DraftAxis,
        takeover: Option<TakeoverRevisionProvenance>,
        recording_merge: Option<RecordingMergeProvenance>,
    ) -> Result<String, CommandError> {
        if self.revision(parent_revision_id).is_none() {
            return Err(CommandError::new("revision_not_found", "未找到父轴版本"));
        }
        let sequence = self.next_revision_sequence;
        self.next_revision_sequence = self.next_revision_sequence.saturating_add(1);
        let id = format!("revision-{sequence:06}");
        self.revisions.push(AxisRevision {
            id: id.clone(),
            sequence,
            parent_revision_id: Some(parent_revision_id.to_string()),
            source,
            attempt_id,
            created_frame,
            axis,
            takeover,
            recording_merge,
        });
        self.current_revision_id = id.clone();
        self.active_recording_revision_id = id.clone();
        self.armed_revision_id = None;
        Ok(id)
    }

    fn revision_mut(&mut self, id: &str) -> Option<&mut AxisRevision> {
        self.revisions.iter_mut().find(|revision| revision.id == id)
    }
}

impl AxisRevision {
    pub fn axis_json_for_use(&self) -> Result<serde_json::Value, CommandError> {
        if self
            .takeover
            .as_ref()
            .is_some_and(|takeover| takeover.uncertain_receipt_sequence.is_some())
        {
            return Err(CommandError::new(
                "takeover_receipt_unconfirmed",
                "接管版本仍有执行结果待确认",
            ));
        }
        if self
            .takeover
            .as_ref()
            .is_some_and(|takeover| !takeover.time_trusted)
        {
            return Err(CommandError::new(
                "takeover_time_untrusted",
                "接管版本缺少可信暂停时间锚点",
            ));
        }
        self.axis.to_axis_json()
    }
}

fn validate_receipts(run_id: &str, receipts: &[ExecutionReceipt]) -> Result<(), CommandError> {
    if run_id.is_empty() {
        return Err(CommandError::new(
            "proxy_run_missing",
            "接管缺少本次代理运行 ID",
        ));
    }
    let mut previous_sequence = None;
    let mut event_ids = HashSet::new();
    for receipt in receipts {
        if receipt.run_id != run_id {
            return Err(CommandError::new(
                "receipt_run_mismatched",
                "执行回执不属于本次代理运行",
            ));
        }
        if previous_sequence.is_some_and(|previous| receipt.receipt_sequence <= previous) {
            return Err(CommandError::new(
                "receipt_sequence_invalid",
                "执行回执序号必须严格递增",
            ));
        }
        if !event_ids.insert(receipt.event_id.as_str()) {
            return Err(CommandError::new(
                "receipt_event_duplicate",
                format!("操作点 {} 出现重复执行回执", receipt.event_id),
            ));
        }
        previous_sequence = Some(receipt.receipt_sequence);
    }
    Ok(())
}

fn push_prefix_event(events: &mut Vec<DraftEvent>, source: &DraftEvent) {
    let mut event = source.clone();
    event.order = events.len() as u32;
    events.push(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::axis::DraftKind;

    fn axis_with_same_frame_events() -> DraftAxis {
        let mut first = DraftEvent::new("event-a".to_string(), 30, 0, DraftKind::Skill);
        first.tile = Some("A1".to_string());
        first.refresh_complete();
        let mut second = DraftEvent::new("event-b".to_string(), 30, 1, DraftKind::Retreat);
        second.tile = Some("A1".to_string());
        second.refresh_complete();
        DraftAxis {
            title: "接管测试".to_string(),
            stage_id: Some("main_00-01".to_string()),
            events: vec![first, second],
        }
    }

    #[test]
    fn takeover_prefix_uses_receipts_instead_of_frame_cutoff() {
        let mut session = OperationSession::new(
            "session-1".to_string(),
            axis_with_same_frame_events(),
            AxisRevisionSource::Manual,
        );
        let id = session
            .create_takeover_revision(
                "revision-000001",
                "proxy-run-000001",
                Some("attempt-000001".to_string()),
                30,
                &[
                    ExecutionReceipt {
                        run_id: "proxy-run-000001".to_string(),
                        event_id: "event-a".to_string(),
                        receipt_sequence: 10,
                        planned_frame: 30,
                        observed_frame: Some(30),
                        source_timestamp_ns: Some(1_000_000_000.0),
                        status: ExecutionReceiptStatus::Confirmed,
                        reason: "confirmed".to_string(),
                    },
                    ExecutionReceipt {
                        run_id: "proxy-run-000001".to_string(),
                        event_id: "event-b".to_string(),
                        receipt_sequence: 11,
                        planned_frame: 30,
                        observed_frame: None,
                        source_timestamp_ns: None,
                        status: ExecutionReceiptStatus::Uncertain,
                        reason: "uncertain".to_string(),
                    },
                ],
            )
            .unwrap();

        let revision = session.revision(&id).unwrap();
        assert_eq!(revision.axis.events.len(), 2);
        assert_eq!(revision.axis.events[0].id, "event-a");
        assert_eq!(revision.axis.events[1].id, "event-b");
        assert_eq!(
            revision
                .takeover
                .as_ref()
                .unwrap()
                .uncertain_receipt_sequence,
            Some(11)
        );
    }

    #[test]
    fn invalid_receipt_order_does_not_create_revision() {
        let mut session = OperationSession::new(
            "session-1".to_string(),
            axis_with_same_frame_events(),
            AxisRevisionSource::Manual,
        );
        let error = session
            .create_takeover_revision(
                "revision-000001",
                "proxy-run-000001",
                None,
                30,
                &[
                    ExecutionReceipt {
                        run_id: "proxy-run-000001".to_string(),
                        event_id: "event-a".to_string(),
                        receipt_sequence: 2,
                        planned_frame: 30,
                        observed_frame: Some(30),
                        source_timestamp_ns: Some(1_000_000_000.0),
                        status: ExecutionReceiptStatus::Confirmed,
                        reason: "confirmed".to_string(),
                    },
                    ExecutionReceipt {
                        run_id: "proxy-run-000001".to_string(),
                        event_id: "event-b".to_string(),
                        receipt_sequence: 1,
                        planned_frame: 30,
                        observed_frame: Some(30),
                        source_timestamp_ns: Some(1_000_000_000.0),
                        status: ExecutionReceiptStatus::Confirmed,
                        reason: "confirmed".to_string(),
                    },
                ],
            )
            .unwrap_err();

        assert_eq!(error.code, "receipt_sequence_invalid");
        assert_eq!(session.revisions.len(), 1);
    }
}
