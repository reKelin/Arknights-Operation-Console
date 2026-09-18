use serde::{Deserialize, Serialize};
use std::fmt;

use super::super::ObservedBattleState;

const MIN_STABLE_OBSERVATIONS: usize = 2;
const MIN_OBSERVATION_CONFIDENCE: u8 = 70;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceTimeBase {
    pub numerator: u32,
    pub denominator: u32,
}

impl SourceTimeBase {
    pub fn parse(value: &str) -> Result<Self, SourceTimestampError> {
        let (numerator, denominator) = value
            .split_once('/')
            .ok_or(SourceTimestampError::InvalidTimeBase)?;
        let numerator = numerator
            .parse::<u32>()
            .map_err(|_| SourceTimestampError::InvalidTimeBase)?;
        let denominator = denominator
            .parse::<u32>()
            .map_err(|_| SourceTimestampError::InvalidTimeBase)?;
        if numerator == 0 || denominator == 0 {
            return Err(SourceTimestampError::InvalidTimeBase);
        }
        Ok(Self {
            numerator,
            denominator,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceTimestamp {
    pub raw_pts: i64,
    pub time_base: SourceTimeBase,
}

impl SourceTimestamp {
    pub fn parse(raw_pts: &str, time_base: SourceTimeBase) -> Result<Self, SourceTimestampError> {
        let raw_pts = raw_pts
            .parse::<i64>()
            .map_err(|_| SourceTimestampError::InvalidPts)?;
        Ok(Self { raw_pts, time_base })
    }

    pub fn nanoseconds(self) -> Result<u64, SourceTimestampError> {
        if self.raw_pts < 0 {
            return Err(SourceTimestampError::NegativePts);
        }
        let nanos = i128::from(self.raw_pts)
            .checked_mul(i128::from(self.time_base.numerator))
            .and_then(|value| value.checked_mul(1_000_000_000))
            .ok_or(SourceTimestampError::Overflow)?
            / i128::from(self.time_base.denominator);
        u64::try_from(nanos).map_err(|_| SourceTimestampError::Overflow)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceTimestampError {
    InvalidTimeBase,
    InvalidPts,
    NegativePts,
    Overflow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFrameTimeline {
    pub time_base: SourceTimeBase,
    pub timestamps: Vec<SourceTimestamp>,
}

impl SourceFrameTimeline {
    pub fn parse_ffprobe_json(bytes: &[u8]) -> Result<Self, SourceTimelineError> {
        let probe: FrameProbeOutput = serde_json::from_slice(bytes)
            .map_err(|error| SourceTimelineError::InvalidJson(error.to_string()))?;
        let stream = probe
            .streams
            .first()
            .ok_or(SourceTimelineError::MissingVideoStream)?;
        let time_base = SourceTimeBase::parse(&stream.time_base)
            .map_err(|_| SourceTimelineError::InvalidTimeBase(stream.time_base.clone()))?;
        let mut timestamps = Vec::with_capacity(probe.frames.len());
        for (ordinal, frame) in probe.frames.into_iter().enumerate() {
            let raw_pts = frame
                .best_effort_timestamp
                .ok_or(SourceTimelineError::MissingPts { ordinal })?
                .parse()
                .map_err(|_| SourceTimelineError::InvalidPts { ordinal })?;
            if timestamps
                .last()
                .is_some_and(|previous: &SourceTimestamp| previous.raw_pts >= raw_pts)
            {
                return Err(SourceTimelineError::NonIncreasingPts { ordinal });
            }
            timestamps.push(SourceTimestamp { raw_pts, time_base });
        }
        if timestamps.is_empty() {
            return Err(SourceTimelineError::NoVideoFrames);
        }
        Ok(Self {
            time_base,
            timestamps,
        })
    }

    pub fn timestamp_for_decoded_frame(
        &self,
        decoded_ordinal: usize,
    ) -> Result<SourceTimestamp, SourceTimelineError> {
        self.timestamps.get(decoded_ordinal).copied().ok_or(
            SourceTimelineError::DecodedFrameWithoutPts {
                ordinal: decoded_ordinal,
            },
        )
    }

    pub fn verify_decoded_frame_count(
        &self,
        decoded_frames: usize,
    ) -> Result<(), SourceTimelineError> {
        if decoded_frames == self.timestamps.len() {
            return Ok(());
        }
        Err(SourceTimelineError::FrameCountMismatch {
            decoded_frames,
            timestamp_frames: self.timestamps.len(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceTimelineError {
    InvalidJson(String),
    MissingVideoStream,
    InvalidTimeBase(String),
    NoVideoFrames,
    MissingPts {
        ordinal: usize,
    },
    InvalidPts {
        ordinal: usize,
    },
    NonIncreasingPts {
        ordinal: usize,
    },
    DecodedFrameWithoutPts {
        ordinal: usize,
    },
    FrameCountMismatch {
        decoded_frames: usize,
        timestamp_frames: usize,
    },
}

impl fmt::Display for SourceTimelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(error) => write!(formatter, "解析逐帧时间戳失败：{error}"),
            Self::MissingVideoStream => formatter.write_str("录屏中没有视频流时间基"),
            Self::InvalidTimeBase(value) => write!(formatter, "录屏视频时间基无效：{value}"),
            Self::NoVideoFrames => formatter.write_str("录屏中没有可分析的视频帧"),
            Self::MissingPts { ordinal } => {
                write!(formatter, "录屏第 {ordinal} 帧缺少原始展示时间戳")
            }
            Self::InvalidPts { ordinal } => {
                write!(formatter, "录屏第 {ordinal} 帧的原始展示时间戳无效")
            }
            Self::NonIncreasingPts { ordinal } => {
                write!(formatter, "录屏第 {ordinal} 帧的原始展示时间戳未递增")
            }
            Self::DecodedFrameWithoutPts { ordinal } => {
                write!(formatter, "解码第 {ordinal} 帧没有对应的原始展示时间戳")
            }
            Self::FrameCountMismatch {
                decoded_frames,
                timestamp_frames,
            } => write!(
                formatter,
                "解码帧数 {decoded_frames} 与时间戳帧数 {timestamp_frames} 不一致"
            ),
        }
    }
}

#[derive(Deserialize)]
struct FrameProbeOutput {
    streams: Vec<FrameProbeStream>,
    frames: Vec<FrameProbeFrame>,
}

#[derive(Deserialize)]
struct FrameProbeStream {
    time_base: String,
}

#[derive(Deserialize)]
struct FrameProbeFrame {
    best_effort_timestamp: Option<ProbeTimestamp>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ProbeTimestamp {
    String(String),
    Integer(i64),
}

impl ProbeTimestamp {
    fn parse(self) -> Result<i64, std::num::ParseIntError> {
        match self {
            Self::String(value) => value.parse(),
            Self::Integer(value) => Ok(value),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameFrameRange {
    pub start: u32,
    pub end: u32,
}

impl GameFrameRange {
    fn is_valid(self) -> bool {
        self.start <= self.end && self.end <= i32::MAX as u32
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateObservation {
    pub source_timestamp: SourceTimestamp,
    pub segment_index: u32,
    pub game_frame_range: GameFrameRange,
    pub battle_state: ObservedBattleState,
    pub observation_confidence: u8,
    pub mapping_trusted: bool,
    pub discontinuity_before: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CandidateActionKind {
    Deploy,
    Skill,
    Retreat,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CandidateEvidence {
    DeploymentGesture,
    SelectedUnitInteraction,
    InterruptedInteraction,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UnconfirmedField {
    ActionKind,
    GameFrame,
    Operator,
    Tile,
    Direction,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisCandidate {
    pub id: String,
    pub segment_index: u32,
    pub source_start: SourceTimestamp,
    pub source_end: SourceTimestamp,
    pub game_frame_range: GameFrameRange,
    pub kind: Option<CandidateActionKind>,
    pub operator: Option<String>,
    pub tile: Option<String>,
    pub direction: Option<FacingDirection>,
    pub evidence: CandidateEvidence,
    pub confidence: u8,
    pub unconfirmed_fields: Vec<UnconfirmedField>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FacingDirection {
    Up,
    Right,
    Down,
    Left,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateConfirmation {
    pub kind: CandidateActionKind,
    pub game_frame: u32,
    pub operator: Option<String>,
    pub tile: Option<String>,
    pub direction: Option<FacingDirection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfirmedOperation {
    pub candidate_id: String,
    pub kind: CandidateActionKind,
    pub game_frame: u32,
    pub operator: Option<String>,
    pub tile: String,
    pub direction: Option<FacingDirection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfirmationError {
    pub field: &'static str,
    pub message: &'static str,
}

pub fn confirm_candidate(
    candidate: &AnalysisCandidate,
    confirmation: CandidateConfirmation,
) -> Result<ConfirmedOperation, ConfirmationError> {
    if confirmation.game_frame > i32::MAX as u32 {
        return Err(ConfirmationError {
            field: "gameFrame",
            message: "游戏帧超出 AxisLink v2 范围",
        });
    }
    let tile = confirmation
        .tile
        .filter(|value| valid_tile_code(value))
        .ok_or(ConfirmationError {
            field: "tile",
            message: "必须填写 A1 到 I36 的合法格子短代码",
        })?;
    match confirmation.kind {
        CandidateActionKind::Deploy => {
            let operator = confirmation
                .operator
                .filter(|value| valid_operator_id(value))
                .ok_or(ConfirmationError {
                    field: "operator",
                    message: "部署操作必须填写合法干员 ID",
                })?;
            let direction = confirmation.direction.ok_or(ConfirmationError {
                field: "direction",
                message: "部署操作必须填写方向",
            })?;
            Ok(ConfirmedOperation {
                candidate_id: candidate.id.clone(),
                kind: confirmation.kind,
                game_frame: confirmation.game_frame,
                operator: Some(operator),
                tile,
                direction: Some(direction),
            })
        }
        CandidateActionKind::Skill | CandidateActionKind::Retreat => {
            if confirmation.operator.is_some() {
                return Err(ConfirmationError {
                    field: "operator",
                    message: "技能和撤退不能携带干员字段",
                });
            }
            if confirmation.direction.is_some() {
                return Err(ConfirmationError {
                    field: "direction",
                    message: "技能和撤退不能携带方向字段",
                });
            }
            Ok(ConfirmedOperation {
                candidate_id: candidate.id.clone(),
                kind: confirmation.kind,
                game_frame: confirmation.game_frame,
                operator: None,
                tile,
                direction: None,
            })
        }
    }
}

pub fn extract_operation_candidates(
    observations: &[CandidateObservation],
) -> Vec<AnalysisCandidate> {
    let mut candidates = Vec::new();
    for runs in stable_run_groups(observations) {
        extract_group_candidates(&runs, &mut candidates);
    }
    for (index, candidate) in candidates.iter_mut().enumerate() {
        candidate.id = format!("recording-{}-{index}", candidate.segment_index);
    }
    candidates
}

#[derive(Clone, Copy)]
struct StableRun {
    segment_index: u32,
    state: ObservedBattleState,
    source_start: SourceTimestamp,
    source_end: SourceTimestamp,
    game_frame_range: GameFrameRange,
    confidence: u8,
    observations: usize,
}

fn stable_run_groups(observations: &[CandidateObservation]) -> Vec<Vec<StableRun>> {
    let mut groups = Vec::new();
    let mut runs = Vec::new();
    let mut current: Option<StableRun> = None;
    let mut previous_source_ns = None;

    for observation in observations {
        let source_ns = observation.source_timestamp.nanoseconds().ok();
        let broken = observation.discontinuity_before
            || !observation.mapping_trusted
            || observation.observation_confidence < MIN_OBSERVATION_CONFIDENCE
            || !observation.game_frame_range.is_valid()
            || source_ns.is_none()
            || previous_source_ns.is_some_and(|previous| source_ns <= Some(previous));
        if broken {
            push_run(&mut runs, current.take());
            push_group(&mut groups, &mut runs);
            previous_source_ns = source_ns;
            continue;
        }
        previous_source_ns = source_ns;
        if let Some(run) = &mut current
            && run.segment_index == observation.segment_index
            && run.state == observation.battle_state
        {
            run.source_end = observation.source_timestamp;
            run.game_frame_range.end = observation.game_frame_range.end;
            run.confidence = run.confidence.min(observation.observation_confidence);
            run.observations += 1;
            continue;
        }
        if current.is_some_and(|run| run.segment_index != observation.segment_index) {
            push_run(&mut runs, current.take());
            push_group(&mut groups, &mut runs);
        } else {
            push_run(&mut runs, current.take());
        }
        current = Some(StableRun {
            segment_index: observation.segment_index,
            state: observation.battle_state,
            source_start: observation.source_timestamp,
            source_end: observation.source_timestamp,
            game_frame_range: observation.game_frame_range,
            confidence: observation.observation_confidence,
            observations: 1,
        });
    }
    push_run(&mut runs, current);
    push_group(&mut groups, &mut runs);
    groups
}

fn push_run(runs: &mut Vec<StableRun>, run: Option<StableRun>) {
    if let Some(run) = run.filter(|run| run.observations >= MIN_STABLE_OBSERVATIONS) {
        runs.push(run);
    }
}

fn push_group(groups: &mut Vec<Vec<StableRun>>, runs: &mut Vec<StableRun>) {
    if !runs.is_empty() {
        groups.push(std::mem::take(runs));
    }
}

fn extract_group_candidates(runs: &[StableRun], candidates: &mut Vec<AnalysisCandidate>) {
    let mut index = 0;
    while index < runs.len() {
        if let Some(candidate) = deployment_candidate(&runs[index..]) {
            candidates.push(candidate);
            index += 3;
            continue;
        }
        if let Some(candidate) = selected_unit_candidate(&runs[index..]) {
            candidates.push(candidate);
            index += 3;
            continue;
        }
        if let Some(candidate) = interrupted_candidate(&runs[index..]) {
            candidates.push(candidate);
        }
        index += 1;
    }
}

fn deployment_candidate(runs: &[StableRun]) -> Option<AnalysisCandidate> {
    let [deploying, facing, finished, ..] = runs else {
        return None;
    };
    if deploying.state != ObservedBattleState::DeployingOperator
        || facing.state != ObservedBattleState::AdjustingOperatorFacing
        || !is_finished_state(finished.state)
    {
        return None;
    }
    Some(candidate_from_runs(
        deploying,
        facing,
        Some(CandidateActionKind::Deploy),
        CandidateEvidence::DeploymentGesture,
        deploying.confidence.min(facing.confidence),
        vec![
            UnconfirmedField::GameFrame,
            UnconfirmedField::Operator,
            UnconfirmedField::Tile,
            UnconfirmedField::Direction,
        ],
    ))
}

fn selected_unit_candidate(runs: &[StableRun]) -> Option<AnalysisCandidate> {
    let [selecting, acting, finished, ..] = runs else {
        return None;
    };
    if selecting.state != ObservedBattleState::PointTwoXRunning
        || acting.state != ObservedBattleState::Paused
        || !is_running_state(finished.state)
    {
        return None;
    }
    Some(candidate_from_runs(
        selecting,
        acting,
        None,
        CandidateEvidence::SelectedUnitInteraction,
        selecting.confidence.min(acting.confidence).min(75),
        vec![
            UnconfirmedField::ActionKind,
            UnconfirmedField::GameFrame,
            UnconfirmedField::Tile,
        ],
    ))
}

fn interrupted_candidate(runs: &[StableRun]) -> Option<AnalysisCandidate> {
    let first = runs.first()?;
    if first.state == ObservedBattleState::DeployingOperator {
        let last = runs
            .get(1)
            .filter(|run| run.state == ObservedBattleState::AdjustingOperatorFacing)
            .unwrap_or(first);
        return Some(candidate_from_runs(
            first,
            last,
            None,
            CandidateEvidence::InterruptedInteraction,
            first.confidence.min(last.confidence).min(49),
            vec![
                UnconfirmedField::ActionKind,
                UnconfirmedField::GameFrame,
                UnconfirmedField::Tile,
            ],
        ));
    }
    if first.state == ObservedBattleState::PointTwoXRunning
        && runs
            .get(1)
            .is_some_and(|run| run.state == ObservedBattleState::Paused)
    {
        return Some(candidate_from_runs(
            first,
            &runs[1],
            None,
            CandidateEvidence::InterruptedInteraction,
            first.confidence.min(runs[1].confidence).min(49),
            vec![
                UnconfirmedField::ActionKind,
                UnconfirmedField::GameFrame,
                UnconfirmedField::Tile,
            ],
        ));
    }
    None
}

fn candidate_from_runs(
    first: &StableRun,
    last: &StableRun,
    kind: Option<CandidateActionKind>,
    evidence: CandidateEvidence,
    confidence: u8,
    unconfirmed_fields: Vec<UnconfirmedField>,
) -> AnalysisCandidate {
    AnalysisCandidate {
        id: String::new(),
        segment_index: first.segment_index,
        source_start: first.source_start,
        source_end: last.source_end,
        game_frame_range: GameFrameRange {
            start: first.game_frame_range.start,
            end: last.game_frame_range.end,
        },
        kind,
        operator: None,
        tile: None,
        direction: None,
        evidence,
        confidence,
        unconfirmed_fields,
    }
}

fn is_finished_state(state: ObservedBattleState) -> bool {
    is_running_state(state) || state == ObservedBattleState::Paused
}

fn is_running_state(state: ObservedBattleState) -> bool {
    matches!(
        state,
        ObservedBattleState::OneXRunning | ObservedBattleState::TwoXRunning
    )
}

fn valid_tile_code(value: &str) -> bool {
    let bytes = value.as_bytes();
    (2..=3).contains(&bytes.len())
        && (b'A'..=b'I').contains(&bytes[0])
        && value[1..]
            .parse::<u8>()
            .is_ok_and(|column| (1..=36).contains(&column))
}

fn valid_operator_id(value: &str) -> bool {
    value.len() <= 128
        && value.strip_prefix("char_").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CandidateFixture {
        time_base: String,
        cases: Vec<FixtureCase>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureCase {
        name: String,
        observations: Vec<FixtureObservation>,
        expected_evidence: Vec<CandidateEvidence>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureObservation {
        pts: i64,
        segment: u32,
        game_start: u32,
        game_end: u32,
        state: ObservedBattleState,
        confidence: u8,
        trusted: bool,
        discontinuity: bool,
    }

    #[test]
    fn preserves_fractional_source_pts() {
        let time_base = SourceTimeBase::parse("1/90000").unwrap();
        let timestamp = SourceTimestamp::parse("3003", time_base).unwrap();
        assert_eq!(timestamp.raw_pts, 3003);
        assert_eq!(timestamp.nanoseconds().unwrap(), 33_366_666);
    }

    #[test]
    fn pairs_variable_rate_source_pts_with_decoded_frames() {
        let timeline = SourceFrameTimeline::parse_ffprobe_json(include_bytes!(
            "../../../tests/fixtures/monitor/recording-source-pts.json"
        ))
        .unwrap();

        assert_eq!(
            timeline.time_base,
            SourceTimeBase::parse("1/90000").unwrap()
        );
        assert_eq!(timeline.timestamps[1].raw_pts, 3003);
        assert_eq!(
            timeline.timestamp_for_decoded_frame(2).unwrap().raw_pts,
            7507
        );
        assert!(timeline.verify_decoded_frame_count(4).is_ok());
        assert_eq!(
            timeline.verify_decoded_frame_count(3).unwrap_err(),
            SourceTimelineError::FrameCountMismatch {
                decoded_frames: 3,
                timestamp_frames: 4,
            }
        );
    }

    #[test]
    fn rejects_missing_or_non_increasing_source_pts() {
        let missing = br#"{
            "streams": [{ "time_base": "1/1000" }],
            "frames": [{ "best_effort_timestamp": "0" }, {}]
        }"#;
        assert_eq!(
            SourceFrameTimeline::parse_ffprobe_json(missing).unwrap_err(),
            SourceTimelineError::MissingPts { ordinal: 1 }
        );

        let repeated = br#"{
            "streams": [{ "time_base": "1/1000" }],
            "frames": [
                { "best_effort_timestamp": 20 },
                { "best_effort_timestamp": "20" }
            ]
        }"#;
        assert_eq!(
            SourceFrameTimeline::parse_ffprobe_json(repeated).unwrap_err(),
            SourceTimelineError::NonIncreasingPts { ordinal: 1 }
        );
    }

    #[test]
    fn extracts_candidates_from_temporal_fixtures() {
        let fixture: CandidateFixture = serde_json::from_str(include_str!(
            "../../../tests/fixtures/monitor/recording-candidates.json"
        ))
        .unwrap();
        let time_base = SourceTimeBase::parse(&fixture.time_base).unwrap();

        for case in fixture.cases {
            let observations = case
                .observations
                .into_iter()
                .map(|observation| CandidateObservation {
                    source_timestamp: SourceTimestamp {
                        raw_pts: observation.pts,
                        time_base,
                    },
                    segment_index: observation.segment,
                    game_frame_range: GameFrameRange {
                        start: observation.game_start,
                        end: observation.game_end,
                    },
                    battle_state: observation.state,
                    observation_confidence: observation.confidence,
                    mapping_trusted: observation.trusted,
                    discontinuity_before: observation.discontinuity,
                })
                .collect::<Vec<_>>();
            let candidates = extract_operation_candidates(&observations);
            let actual = candidates
                .iter()
                .map(|candidate| candidate.evidence)
                .collect::<Vec<_>>();
            assert_eq!(actual, case.expected_evidence, "fixture {}", case.name);
        }
    }

    #[test]
    fn manually_confirms_all_axislink_operation_kinds() {
        let candidate = sample_candidate();
        let deploy = confirm_candidate(
            &candidate,
            CandidateConfirmation {
                kind: CandidateActionKind::Deploy,
                game_frame: 123,
                operator: Some("char_002_amiya".to_string()),
                tile: Some("C5".to_string()),
                direction: Some(FacingDirection::Left),
            },
        )
        .unwrap();
        assert_eq!(deploy.kind, CandidateActionKind::Deploy);

        for kind in [CandidateActionKind::Skill, CandidateActionKind::Retreat] {
            let operation = confirm_candidate(
                &candidate,
                CandidateConfirmation {
                    kind,
                    game_frame: 124,
                    operator: None,
                    tile: Some("I36".to_string()),
                    direction: None,
                },
            )
            .unwrap();
            assert_eq!(operation.kind, kind);
        }
    }

    #[test]
    fn incomplete_confirmation_stays_blocked() {
        let error = confirm_candidate(
            &sample_candidate(),
            CandidateConfirmation {
                kind: CandidateActionKind::Deploy,
                game_frame: 123,
                operator: None,
                tile: Some("C5".to_string()),
                direction: None,
            },
        )
        .unwrap_err();
        assert_eq!(error.field, "operator");
    }

    fn sample_candidate() -> AnalysisCandidate {
        let time_base = SourceTimeBase::parse("1/1000").unwrap();
        AnalysisCandidate {
            id: "recording-0-0".to_string(),
            segment_index: 0,
            source_start: SourceTimestamp {
                raw_pts: 100,
                time_base,
            },
            source_end: SourceTimestamp {
                raw_pts: 200,
                time_base,
            },
            game_frame_range: GameFrameRange { start: 30, end: 34 },
            kind: None,
            operator: None,
            tile: None,
            direction: None,
            evidence: CandidateEvidence::SelectedUnitInteraction,
            confidence: 75,
            unconfirmed_fields: vec![UnconfirmedField::ActionKind],
        }
    }
}
