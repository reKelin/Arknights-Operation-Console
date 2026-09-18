use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    axis::{DraftAxis, DraftEvent, DraftKind, EventFrameRange, TimeConfirmation},
    monitor::ClockQuality,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordingAlignment {
    pub source_anchor_frame: u32,
    pub target_anchor_frame: u32,
    pub offset_frames: i32,
    pub quality: ClockQuality,
    pub manual_confirmation: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordingMergeConflict {
    pub candidate_id: String,
    pub existing_event_id: String,
    pub aligned_frame: u32,
    pub tile: String,
    pub kind: DraftKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RecordingConflictDecisionKind {
    KeepCandidate,
    ExcludeCandidate,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordingConflictDecision {
    pub candidate_id: String,
    pub decision: RecordingConflictDecisionKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RecordingMergeMode {
    NewAxis,
    Continuation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordingMergeInput {
    pub mode: RecordingMergeMode,
    pub parent_revision_id: String,
    pub recording_analysis_id: String,
    pub segment_index: u32,
    pub source_anchor_frame: u32,
    pub target_anchor_frame: u32,
    pub offset_frames: i32,
    pub manual_alignment_confirmed: bool,
    pub conflict_decisions: Vec<RecordingConflictDecision>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordingMergePreview {
    pub conflicts: Vec<RecordingMergeConflict>,
    pub skipped_before_anchor: u32,
    pub candidate_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordingMergeError {
    pub code: &'static str,
    pub message: String,
    pub candidate_id: Option<String>,
}

#[derive(Debug)]
pub struct RecordingMergePlan {
    parent_axis: DraftAxis,
    aligned_candidates: Vec<DraftEvent>,
    pub conflicts: Vec<RecordingMergeConflict>,
    pub recording_analysis_id: String,
    pub segment_index: u32,
    pub offset_frames: i32,
    pub skipped_before_anchor: u32,
}

#[derive(Debug)]
pub struct RecordingMergeResult {
    pub axis: DraftAxis,
    pub recording_analysis_id: String,
    pub candidate_ids: Vec<String>,
    pub segment_index: u32,
    pub offset_frames: i32,
}

pub fn plan_recording_merge(
    parent_axis: &DraftAxis,
    recording_analysis_id: &str,
    segment_index: u32,
    alignment: RecordingAlignment,
    candidates: &[DraftEvent],
) -> Result<RecordingMergePlan, RecordingMergeError> {
    validate_alignment(alignment)?;
    let parent_candidate_sources = parent_axis
        .events
        .iter()
        .filter_map(candidate_source)
        .collect::<HashSet<_>>();
    let parent_event_ids = parent_axis
        .events
        .iter()
        .map(|event| event.id.as_str())
        .collect::<HashSet<_>>();
    let mut batch_candidate_ids = HashSet::new();
    let mut skipped_before_anchor = 0_u32;
    let mut aligned_candidates = Vec::new();

    for candidate in candidates {
        if candidate.source_segment_index != Some(segment_index) {
            return Err(error(
                "recording_segment_mismatch",
                "候选不属于所选录屏区段",
                candidate.source_candidate_id.clone(),
            ));
        }
        if candidate.source_recording_id.as_deref() != Some(recording_analysis_id) {
            return Err(error(
                "recording_analysis_source_mismatch",
                "候选不属于所选录屏分析任务",
                candidate.source_candidate_id.clone(),
            ));
        }
        let candidate_id = candidate.source_candidate_id.as_ref().ok_or_else(|| {
            error(
                "recording_candidate_source_missing",
                "候选缺少稳定来源标识",
                None,
            )
        })?;
        let source = (
            recording_analysis_id.to_string(),
            segment_index,
            candidate_id.clone(),
        );
        if parent_candidate_sources.contains(&source) || !batch_candidate_ids.insert(source) {
            return Err(error(
                "recording_candidate_duplicate",
                "同一录屏候选不能重复合并",
                Some(candidate_id.clone()),
            ));
        }
        if parent_event_ids.contains(candidate.id.as_str()) {
            return Err(error(
                "recording_event_id_conflict",
                "候选事件 ID 已存在于父版本",
                Some(candidate_id.clone()),
            ));
        }
        if candidate.frame <= alignment.source_anchor_frame {
            skipped_before_anchor += 1;
            continue;
        }
        if candidate.kind == DraftKind::Bookmark
            || !candidate.complete
            || candidate.time_confirmation == TimeConfirmation::Unconfirmed
        {
            return Err(error(
                "recording_candidate_needs_review",
                "候选仍有未分类、缺失参数或未确认时间",
                Some(candidate_id.clone()),
            ));
        }
        let mut aligned = candidate.clone();
        aligned.frame = shifted_frame(candidate.frame, alignment.offset_frames, candidate_id)?;
        aligned.frame_range =
            shifted_range(candidate.frame_range, alignment.offset_frames, candidate_id)?;
        aligned_candidates.push(aligned);
    }

    aligned_candidates.sort_by(|left, right| {
        left.frame
            .cmp(&right.frame)
            .then(left.order.cmp(&right.order))
            .then(left.source_candidate_id.cmp(&right.source_candidate_id))
    });
    let mut conflicts = Vec::new();
    for (index, candidate) in aligned_candidates.iter().enumerate() {
        for existing in parent_axis
            .events
            .iter()
            .chain(aligned_candidates[..index].iter())
        {
            if is_semantic_conflict(existing, candidate) {
                conflicts.push(RecordingMergeConflict {
                    candidate_id: candidate
                        .source_candidate_id
                        .clone()
                        .expect("validated candidate source"),
                    existing_event_id: existing.id.clone(),
                    aligned_frame: candidate.frame,
                    tile: candidate.tile.clone().expect("complete operation has tile"),
                    kind: candidate.kind,
                });
            }
        }
    }

    Ok(RecordingMergePlan {
        parent_axis: parent_axis.clone(),
        aligned_candidates,
        conflicts,
        recording_analysis_id: recording_analysis_id.to_string(),
        segment_index,
        offset_frames: alignment.offset_frames,
        skipped_before_anchor,
    })
}

impl RecordingMergePlan {
    pub fn preview(&self) -> RecordingMergePreview {
        RecordingMergePreview {
            conflicts: self.conflicts.clone(),
            skipped_before_anchor: self.skipped_before_anchor,
            candidate_count: self.aligned_candidates.len() as u32,
        }
    }
}

pub fn resolve_recording_merge(
    plan: RecordingMergePlan,
    decisions: &[RecordingConflictDecision],
) -> Result<RecordingMergeResult, RecordingMergeError> {
    let mut decision_by_candidate = HashMap::new();
    for decision in decisions {
        if decision_by_candidate
            .insert(decision.candidate_id.as_str(), decision.decision)
            .is_some()
        {
            return Err(error(
                "recording_conflict_decision_duplicate",
                "同一候选只能提交一个冲突决议",
                Some(decision.candidate_id.clone()),
            ));
        }
    }
    let conflicted_ids = plan
        .conflicts
        .iter()
        .map(|conflict| conflict.candidate_id.as_str())
        .collect::<HashSet<_>>();
    for candidate_id in &conflicted_ids {
        if !decision_by_candidate.contains_key(candidate_id) {
            return Err(error(
                "recording_conflict_resolution_required",
                "存在同帧同格同类操作，请先确认保留或排除",
                Some((*candidate_id).to_string()),
            ));
        }
    }
    if let Some(unknown) = decision_by_candidate
        .keys()
        .find(|candidate_id| !conflicted_ids.contains(**candidate_id))
    {
        return Err(error(
            "recording_conflict_not_found",
            "冲突决议不属于当前合并计划",
            Some((*unknown).to_string()),
        ));
    }

    let mut axis = plan.parent_axis;
    let mut next_order = axis
        .events
        .iter()
        .map(|event| event.order)
        .max()
        .map_or(0, |order| order.saturating_add(1));
    let mut candidate_ids = Vec::new();
    for mut candidate in plan.aligned_candidates {
        let candidate_id = candidate
            .source_candidate_id
            .clone()
            .expect("validated candidate source");
        if decision_by_candidate.get(candidate_id.as_str())
            == Some(&RecordingConflictDecisionKind::ExcludeCandidate)
        {
            continue;
        }
        candidate.order = next_order;
        next_order = next_order.saturating_add(1);
        candidate_ids.push(candidate_id);
        axis.events.push(candidate);
    }
    axis.sort_events();
    Ok(RecordingMergeResult {
        axis,
        recording_analysis_id: plan.recording_analysis_id,
        candidate_ids,
        segment_index: plan.segment_index,
        offset_frames: plan.offset_frames,
    })
}

fn validate_alignment(alignment: RecordingAlignment) -> Result<(), RecordingMergeError> {
    let expected = i64::from(alignment.target_anchor_frame)
        .checked_sub(i64::from(alignment.source_anchor_frame))
        .ok_or_else(|| error("recording_alignment_overflow", "录屏对齐偏移超出范围", None))?;
    if expected != i64::from(alignment.offset_frames) {
        return Err(error(
            "recording_alignment_mismatch",
            "帧偏移与源锚点、目标锚点不一致",
            None,
        ));
    }
    if alignment.quality != ClockQuality::Trusted && !alignment.manual_confirmation {
        return Err(error(
            "recording_alignment_confirmation_required",
            "不可信时间对齐必须经过人工确认",
            None,
        ));
    }
    Ok(())
}

fn shifted_frame(frame: u32, offset: i32, candidate_id: &str) -> Result<u32, RecordingMergeError> {
    let shifted = i64::from(frame) + i64::from(offset);
    if !(0..=i64::from(i32::MAX)).contains(&shifted) {
        return Err(error(
            "recording_aligned_frame_out_of_range",
            "校正后的游戏帧超出 AxisLink v2 范围",
            Some(candidate_id.to_string()),
        ));
    }
    Ok(shifted as u32)
}

fn shifted_range(
    range: EventFrameRange,
    offset: i32,
    candidate_id: &str,
) -> Result<EventFrameRange, RecordingMergeError> {
    let start = shifted_frame(range.start, offset, candidate_id)?;
    let end = shifted_frame(range.end, offset, candidate_id)?;
    if start > end {
        return Err(error(
            "recording_aligned_range_invalid",
            "校正后的候选帧范围无效",
            Some(candidate_id.to_string()),
        ));
    }
    Ok(EventFrameRange { start, end })
}

fn is_semantic_conflict(existing: &DraftEvent, candidate: &DraftEvent) -> bool {
    existing.frame == candidate.frame
        && existing.kind == candidate.kind
        && existing.tile.is_some()
        && existing.tile == candidate.tile
        && candidate_source(existing) != candidate_source(candidate)
}

fn candidate_source(event: &DraftEvent) -> Option<(String, u32, String)> {
    Some((
        event.source_recording_id.clone()?,
        event.source_segment_index?,
        event.source_candidate_id.clone()?,
    ))
}

fn error(
    code: &'static str,
    message: impl Into<String>,
    candidate_id: Option<String>,
) -> RecordingMergeError {
    RecordingMergeError {
        code,
        message: message.into(),
        candidate_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::axis::{DraftDirection, DraftKind};

    #[test]
    fn keeps_prefix_and_same_frame_candidates_in_source_order() {
        let parent = axis(vec![event("executed", 120, 2, None, "C5")]);
        let candidates = vec![
            event("draft-b", 30, 8, Some("candidate-b"), "D5"),
            event("draft-a", 30, 4, Some("candidate-a"), "E5"),
        ];
        let plan = plan_recording_merge(&parent, "analysis-a", 3, alignment(20, 110), &candidates)
            .unwrap();
        let result = resolve_recording_merge(plan, &[]).unwrap();

        assert_eq!(
            result
                .axis
                .events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            ["executed", "draft-a", "draft-b"]
        );
    }

    #[test]
    fn requires_explicit_resolution_without_dropping_same_frame_operation() {
        let parent = axis(vec![event("executed", 120, 2, None, "C5")]);
        let candidates = vec![event("draft-a", 30, 4, Some("candidate-a"), "C5")];
        let plan = plan_recording_merge(&parent, "analysis-a", 3, alignment(20, 110), &candidates)
            .unwrap();
        assert_eq!(plan.conflicts.len(), 1);

        let error = resolve_recording_merge(plan, &[]).unwrap_err();
        assert_eq!(error.code, "recording_conflict_resolution_required");
    }

    #[test]
    fn rejects_cross_segment_and_duplicate_sources() {
        let parent = axis(vec![]);
        let mut wrong_segment = event("draft-a", 30, 0, Some("candidate-a"), "C5");
        wrong_segment.source_segment_index = Some(4);
        assert_eq!(
            plan_recording_merge(
                &parent,
                "analysis-a",
                3,
                alignment(20, 110),
                &[wrong_segment],
            )
            .unwrap_err()
            .code,
            "recording_segment_mismatch"
        );

        let candidate = event("draft-a", 30, 0, Some("candidate-a"), "C5");
        let duplicate = event("draft-b", 31, 1, Some("candidate-a"), "D5");
        assert_eq!(
            plan_recording_merge(
                &parent,
                "analysis-a",
                3,
                alignment(20, 110),
                &[candidate, duplicate],
            )
            .unwrap_err()
            .code,
            "recording_candidate_duplicate"
        );
    }

    #[test]
    fn same_candidate_id_from_another_analysis_is_not_a_duplicate() {
        let mut previous = event("previous", 20, 0, Some("candidate-a"), "D5");
        previous.source_recording_id = Some("analysis-b".to_string());
        let parent = axis(vec![previous]);
        let candidates = [event("draft-a", 30, 1, Some("candidate-a"), "C5")];

        assert!(
            plan_recording_merge(&parent, "analysis-a", 3, alignment(20, 110), &candidates,)
                .is_ok()
        );
    }

    #[test]
    fn requires_manual_confirmation_for_uncertain_alignment() {
        let parent = axis(vec![]);
        let candidates = [event("draft-a", 30, 0, Some("candidate-a"), "C5")];
        let mut uncertain = alignment(20, 110);
        uncertain.quality = ClockQuality::Uncertain;

        assert_eq!(
            plan_recording_merge(&parent, "analysis-a", 3, uncertain, &candidates)
                .unwrap_err()
                .code,
            "recording_alignment_confirmation_required"
        );
        uncertain.manual_confirmation = true;
        assert!(plan_recording_merge(&parent, "analysis-a", 3, uncertain, &candidates).is_ok());
    }

    #[test]
    fn rejects_aligned_frame_beyond_axislink_range() {
        let parent = axis(vec![]);
        let candidates = [event("draft-a", 21, 0, Some("candidate-a"), "C5")];

        assert_eq!(
            plan_recording_merge(
                &parent,
                "analysis-a",
                3,
                alignment(20, i32::MAX as u32),
                &candidates,
            )
            .unwrap_err()
            .code,
            "recording_aligned_frame_out_of_range"
        );
    }

    fn alignment(source: u32, target: u32) -> RecordingAlignment {
        RecordingAlignment {
            source_anchor_frame: source,
            target_anchor_frame: target,
            offset_frames: target as i32 - source as i32,
            quality: ClockQuality::Trusted,
            manual_confirmation: false,
        }
    }

    fn axis(events: Vec<DraftEvent>) -> DraftAxis {
        DraftAxis {
            title: "接续轴".to_string(),
            stage_id: Some("main_00-01".to_string()),
            events,
        }
    }

    fn event(
        id: &str,
        frame: u32,
        order: u32,
        candidate_id: Option<&str>,
        tile: &str,
    ) -> DraftEvent {
        DraftEvent {
            id: id.to_string(),
            frame,
            order,
            kind: DraftKind::Deploy,
            operator: Some("char_002_amiya".to_string()),
            tile: Some(tile.to_string()),
            direction: Some(DraftDirection::Right),
            label: None,
            complete: true,
            attempt_id: None,
            source_recording_id: candidate_id.map(|_| "analysis-a".to_string()),
            source_candidate_id: candidate_id.map(str::to_string),
            source_segment_index: candidate_id.map(|_| 3),
            source_timestamp_ns: Some(1_000_000_000.0),
            frame_range: EventFrameRange {
                start: frame,
                end: frame,
            },
            clock_quality: ClockQuality::Trusted,
            time_confirmation: TimeConfirmation::Observed,
        }
    }
}
