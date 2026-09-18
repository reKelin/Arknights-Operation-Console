use serde::{Deserialize, Serialize};
use specta::Type;

use super::{ObservedBattleState, VisualObservation};

const NANOS_PER_SECOND: u128 = 1_000_000_000;
const GAME_TICKS_PER_SECOND: u128 = 30;
const SPEED_DENOMINATOR: u128 = 5;
const EXIT_CONFIRM_OBSERVATIONS: u8 = 30;
const TRUSTED_CONFIDENCE: u8 = 70;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ClockMode {
    Human,
    Proxy,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ClockQuality {
    #[default]
    Waiting,
    Trusted,
    Uncertain,
    Lost,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClockAnchor {
    pub frame: u32,
    pub source_timestamp_ns: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClockSnapshot {
    pub mode: ClockMode,
    pub active: bool,
    pub receiving_observations: bool,
    pub frame: u32,
    pub speed_fifths: u8,
    pub quality: ClockQuality,
    pub uncertainty_frames: u32,
    pub source_timestamp_ns: Option<f64>,
    pub anchor: Option<ClockAnchor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockTransition {
    None,
    Started,
    Advanced,
    Paused,
    Frozen,
    Exited,
}

#[derive(Clone, Copy, Debug)]
pub struct ClockUpdate {
    pub transition: ClockTransition,
    pub active: bool,
    pub frame: u32,
    pub speed_fifths: u8,
    pub quality: ClockQuality,
    pub uncertainty_frames: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct ClockHandoff {
    frame: u32,
    remainder: u128,
    source_timestamp_ns: u64,
    speed_fifths: u8,
    last_cost_phase: Option<u16>,
    cost_cycles: u32,
    cost_origin: Option<u32>,
    quality: ClockQuality,
    uncertainty_frames: u32,
}

#[derive(Default)]
struct ClockState {
    active: bool,
    receiving_observations: bool,
    frame: u32,
    remainder: u128,
    last_timestamp_ns: Option<u64>,
    last_cost_phase: Option<u16>,
    cost_cycles: u32,
    cost_origin: Option<u32>,
    outside_streak: u8,
    uncertainty_frames: u32,
    speed_fifths: u8,
    quality: ClockQuality,
    anchor: Option<ClockAnchor>,
}

pub struct HumanClock(ClockState);
pub struct ProxyClock(ClockState);

// 录屏分析在 PR F 切换到 HumanClock 后删除该别名。
pub type ObservationClock = HumanClock;

macro_rules! impl_clock {
    ($clock:ident, $mode:expr) => {
        impl Default for $clock {
            fn default() -> Self {
                Self(ClockState {
                    receiving_observations: true,
                    ..ClockState::default()
                })
            }
        }
        impl $clock {
            pub fn observe(&mut self, observation: &VisualObservation) -> ClockUpdate {
                observe(&mut self.0, $mode, observation)
            }
            pub fn mark_observation_gap(&mut self, dropped: u32) -> ClockUpdate {
                mark_observation_gap(&mut self.0, $mode, dropped)
            }
            pub fn freeze(&mut self) -> ClockUpdate {
                freeze(&mut self.0, $mode)
            }
            pub fn snapshot(&self) -> ClockSnapshot {
                snapshot(&self.0, $mode)
            }
            pub fn trusted_handoff(&mut self) -> Option<ClockHandoff> {
                trusted_handoff(&mut self.0)
            }
            pub fn accept_handoff(&mut self, handoff: ClockHandoff) -> ClockSnapshot {
                accept_handoff(&mut self.0, $mode, handoff)
            }
        }
    };
}

impl_clock!(HumanClock, ClockMode::Human);
impl_clock!(ProxyClock, ClockMode::Proxy);

impl ProxyClock {
    pub fn takeover_handoff(&mut self, source_timestamp_ns: u64) -> Option<ClockHandoff> {
        takeover_handoff(&mut self.0, source_timestamp_ns)
    }

    pub fn begin_paused_transaction(&mut self) -> ClockUpdate {
        freeze(&mut self.0, ClockMode::Proxy)
    }

    pub fn confirm_paused_transaction(&mut self, source_timestamp_ns: u64) -> ClockSnapshot {
        self.0.last_timestamp_ns = Some(source_timestamp_ns);
        self.0.speed_fifths = 0;
        self.0.quality = ClockQuality::Trusted;
        self.0.uncertainty_frames = 0;
        self.0.anchor = Some(ClockAnchor {
            frame: self.0.frame,
            source_timestamp_ns: source_timestamp_ns as f64,
        });
        snapshot(&self.0, ClockMode::Proxy)
    }
}

fn observe(
    state: &mut ClockState,
    mode: ClockMode,
    observation: &VisualObservation,
) -> ClockUpdate {
    if !state.receiving_observations {
        return update(state, mode, ClockTransition::Frozen, 0);
    }
    if matches!(
        observation.battle_state,
        ObservedBattleState::NotInBattle | ObservedBattleState::BattleBegin
    ) {
        return observe_outside(state, mode, observation);
    }
    state.outside_streak = 0;
    if !state.active {
        if observation.confidence < TRUSTED_CONFIDENCE || !observation.battle_state.is_running() {
            state.last_timestamp_ns = Some(observation.capture_timestamp_ns);
            return update(state, mode, ClockTransition::Frozen, 0);
        }
        start(state, observation);
        return update(
            state,
            mode,
            ClockTransition::Started,
            speed_fifths(observation.battle_state),
        );
    }

    let Some(previous_timestamp_ns) = state.last_timestamp_ns else {
        state.last_timestamp_ns = Some(observation.capture_timestamp_ns);
        recover_from_anchor(state, observation);
        return update(state, mode, ClockTransition::Frozen, 0);
    };
    if observation.capture_timestamp_ns < previous_timestamp_ns {
        state.last_timestamp_ns = Some(observation.capture_timestamp_ns);
        state.quality = ClockQuality::Lost;
        state.anchor = None;
        return update(state, mode, ClockTransition::Frozen, 0);
    }
    let elapsed_ns = observation.capture_timestamp_ns - previous_timestamp_ns;
    state.last_timestamp_ns = Some(observation.capture_timestamp_ns);

    if observation.confidence < TRUSTED_CONFIDENCE
        || observation.battle_state == ObservedBattleState::Unknown
    {
        mark_uncertain(state, elapsed_ns);
        return update(state, mode, ClockTransition::Frozen, 0);
    }
    if mode == ClockMode::Proxy
        && matches!(
            observation.battle_state,
            ObservedBattleState::PointTwoXRunning | ObservedBattleState::DeployingOperator
        )
    {
        mark_uncertain(state, elapsed_ns);
        return update(state, mode, ClockTransition::Frozen, 0);
    }
    if matches!(
        observation.battle_state,
        ObservedBattleState::Paused | ObservedBattleState::AdjustingOperatorFacing
    ) {
        if state.quality == ClockQuality::Trusted {
            state.anchor = Some(ClockAnchor {
                frame: state.frame,
                source_timestamp_ns: observation.capture_timestamp_ns as f64,
            });
        }
        return update(state, mode, ClockTransition::Paused, 0);
    }

    let previous_frame = state.frame;
    (state.frame, state.remainder) = advance_fixed(
        state.frame,
        state.remainder,
        elapsed_ns,
        speed_fifths(observation.battle_state),
    );
    let anchored = !observation.cost_full && apply_cost_anchor(state, observation);
    if state.quality == ClockQuality::Trusted || anchored {
        state.quality = ClockQuality::Trusted;
        state.uncertainty_frames = 0;
        state.anchor = Some(ClockAnchor {
            frame: state.frame,
            source_timestamp_ns: observation.capture_timestamp_ns as f64,
        });
    }
    let transition = if state.frame == previous_frame {
        ClockTransition::None
    } else {
        ClockTransition::Advanced
    };
    update(
        state,
        mode,
        transition,
        speed_fifths(observation.battle_state),
    )
}

fn start(state: &mut ClockState, observation: &VisualObservation) {
    state.active = true;
    state.frame = 0;
    state.remainder = 0;
    state.last_timestamp_ns = Some(observation.capture_timestamp_ns);
    state.last_cost_phase = observation.cost_phase;
    state.cost_cycles = 0;
    state.cost_origin = observation.cost_phase.map(u32::from);
    state.outside_streak = 0;
    state.uncertainty_frames = 0;
    state.quality = ClockQuality::Trusted;
    state.anchor = Some(ClockAnchor {
        frame: 0,
        source_timestamp_ns: observation.capture_timestamp_ns as f64,
    });
}

fn observe_outside(
    state: &mut ClockState,
    mode: ClockMode,
    observation: &VisualObservation,
) -> ClockUpdate {
    state.last_timestamp_ns = Some(observation.capture_timestamp_ns);
    if observation.battle_state == ObservedBattleState::BattleBegin || !state.active {
        state.outside_streak = 0;
        return update(state, mode, ClockTransition::None, 0);
    }
    state.outside_streak = state.outside_streak.saturating_add(1);
    if state.outside_streak < EXIT_CONFIRM_OBSERVATIONS {
        return update(state, mode, ClockTransition::Frozen, 0);
    }
    let receiving_observations = state.receiving_observations;
    *state = ClockState {
        receiving_observations,
        ..ClockState::default()
    };
    update(state, mode, ClockTransition::Exited, 0)
}

fn mark_uncertain(state: &mut ClockState, elapsed_ns: u64) {
    state.uncertainty_frames = state
        .uncertainty_frames
        .saturating_add(nanos_to_frames(elapsed_ns));
    state.quality = ClockQuality::Uncertain;
    state.anchor = None;
}

fn mark_observation_gap(state: &mut ClockState, mode: ClockMode, dropped: u32) -> ClockUpdate {
    state.last_timestamp_ns = None;
    state.anchor = None;
    state.quality = ClockQuality::Lost;
    state.uncertainty_frames = state.uncertainty_frames.saturating_add(dropped.max(1));
    update(state, mode, ClockTransition::Frozen, 0)
}

fn freeze(state: &mut ClockState, mode: ClockMode) -> ClockUpdate {
    state.last_timestamp_ns = None;
    if state.active {
        state.quality = ClockQuality::Uncertain;
        state.anchor = None;
    }
    update(state, mode, ClockTransition::Frozen, 0)
}

fn recover_from_anchor(state: &mut ClockState, observation: &VisualObservation) {
    if observation.confidence >= TRUSTED_CONFIDENCE
        && observation.battle_state.is_running()
        && !observation.cost_full
        && apply_cost_anchor(state, observation)
    {
        state.quality = ClockQuality::Trusted;
        state.uncertainty_frames = 0;
        state.anchor = Some(ClockAnchor {
            frame: state.frame,
            source_timestamp_ns: observation.capture_timestamp_ns as f64,
        });
    } else if state.quality == ClockQuality::Lost {
        state.quality = ClockQuality::Uncertain;
    }
}

fn apply_cost_anchor(state: &mut ClockState, observation: &VisualObservation) -> bool {
    let (Some(phase), Some(origin)) = (observation.cost_phase, state.cost_origin) else {
        return false;
    };
    let total = u32::from(observation.cost_total.max(1));
    if let Some(previous) = state.last_cost_phase
        && u32::from(previous) * 4 >= total * 3
        && u32::from(phase) * 4 <= total
    {
        state.cost_cycles = state.cost_cycles.saturating_add(1);
    }
    state.last_cost_phase = Some(phase);
    let absolute = state
        .cost_cycles
        .saturating_mul(total)
        .saturating_add(u32::from(phase));
    if absolute < origin {
        state.uncertainty_frames = state.uncertainty_frames.max(origin - absolute);
        return false;
    }
    let target = absolute - origin;
    if target >= state.frame {
        state.frame = target;
        true
    } else {
        state.uncertainty_frames = state.uncertainty_frames.max(state.frame - target);
        false
    }
}

fn trusted_handoff(state: &mut ClockState) -> Option<ClockHandoff> {
    let source_timestamp_ns = state.last_timestamp_ns?;
    if !state.active || !state.receiving_observations || state.quality != ClockQuality::Trusted {
        return None;
    }
    state.receiving_observations = false;
    state.last_timestamp_ns = None;
    Some(ClockHandoff {
        frame: state.frame,
        remainder: state.remainder,
        source_timestamp_ns,
        speed_fifths: state.speed_fifths,
        last_cost_phase: state.last_cost_phase,
        cost_cycles: state.cost_cycles,
        cost_origin: state.cost_origin,
        quality: ClockQuality::Trusted,
        uncertainty_frames: 0,
    })
}

fn takeover_handoff(state: &mut ClockState, source_timestamp_ns: u64) -> Option<ClockHandoff> {
    if !state.active || !state.receiving_observations {
        return None;
    }
    let quality = state.quality;
    state.receiving_observations = false;
    state.last_timestamp_ns = None;
    Some(ClockHandoff {
        frame: state.frame,
        remainder: state.remainder,
        source_timestamp_ns,
        speed_fifths: 0,
        last_cost_phase: state.last_cost_phase,
        cost_cycles: state.cost_cycles,
        cost_origin: state.cost_origin,
        quality,
        uncertainty_frames: state
            .uncertainty_frames
            .max((quality != ClockQuality::Trusted) as u32),
    })
}

fn accept_handoff(state: &mut ClockState, mode: ClockMode, handoff: ClockHandoff) -> ClockSnapshot {
    *state = ClockState {
        active: true,
        receiving_observations: true,
        frame: handoff.frame,
        remainder: handoff.remainder,
        last_timestamp_ns: Some(handoff.source_timestamp_ns),
        last_cost_phase: handoff.last_cost_phase,
        cost_cycles: handoff.cost_cycles,
        cost_origin: handoff.cost_origin,
        quality: handoff.quality,
        uncertainty_frames: handoff.uncertainty_frames,
        speed_fifths: handoff.speed_fifths,
        anchor: (handoff.quality == ClockQuality::Trusted).then_some(ClockAnchor {
            frame: handoff.frame,
            source_timestamp_ns: handoff.source_timestamp_ns as f64,
        }),
        ..ClockState::default()
    };
    snapshot(state, mode)
}

fn update(
    state: &mut ClockState,
    _mode: ClockMode,
    transition: ClockTransition,
    speed_fifths: u8,
) -> ClockUpdate {
    state.speed_fifths = speed_fifths;
    ClockUpdate {
        transition,
        active: state.active,
        frame: state.frame,
        speed_fifths,
        quality: state.quality,
        uncertainty_frames: state.uncertainty_frames,
    }
}

fn snapshot(state: &ClockState, mode: ClockMode) -> ClockSnapshot {
    ClockSnapshot {
        mode,
        active: state.active,
        receiving_observations: state.receiving_observations,
        frame: state.frame,
        speed_fifths: state.speed_fifths,
        quality: state.quality,
        uncertainty_frames: state.uncertainty_frames,
        source_timestamp_ns: state.last_timestamp_ns.map(|timestamp| timestamp as f64),
        anchor: state.anchor,
    }
}

fn advance_fixed(frame: u32, remainder: u128, elapsed_ns: u64, speed_fifths: u8) -> (u32, u128) {
    let progress =
        remainder + u128::from(elapsed_ns) * GAME_TICKS_PER_SECOND * u128::from(speed_fifths);
    let unit = NANOS_PER_SECOND * SPEED_DENOMINATOR;
    (
        frame.saturating_add((progress / unit).min(u128::from(u32::MAX)) as u32),
        progress % unit,
    )
}

fn speed_fifths(state: ObservedBattleState) -> u8 {
    match state {
        ObservedBattleState::TwoXRunning => 10,
        ObservedBattleState::PointTwoXRunning | ObservedBattleState::DeployingOperator => 1,
        ObservedBattleState::Paused | ObservedBattleState::AdjustingOperatorFacing => 0,
        _ => 5,
    }
}

fn nanos_to_frames(nanos: u64) -> u32 {
    ((u128::from(nanos) * GAME_TICKS_PER_SECOND) / NANOS_PER_SECOND).min(u128::from(u32::MAX))
        as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    fn observation(timestamp: u64, state: ObservedBattleState) -> VisualObservation {
        VisualObservation {
            capture_timestamp_ns: timestamp,
            battle_state: state,
            confidence: 90,
            cost_phase: None,
            cost_total: 30,
            cost_full: false,
            stage_recognition: None,
            title_candidate: false,
        }
    }
    #[test]
    fn human_clock_reports_exact_speeds() {
        let mut clock = HumanClock::default();
        clock.observe(&observation(0, ObservedBattleState::OneXRunning));
        let slow = clock.observe(&observation(
            1_000_000_000,
            ObservedBattleState::PointTwoXRunning,
        ));
        let fast = clock.observe(&observation(
            2_000_000_000,
            ObservedBattleState::TwoXRunning,
        ));
        assert_eq!((slow.frame, slow.speed_fifths), (6, 1));
        assert_eq!((fast.frame, fast.speed_fifths), (66, 10));
    }
    #[test]
    fn gap_stays_uncertain_without_forward_cost_anchor() {
        let mut clock = HumanClock::default();
        let mut first = observation(0, ObservedBattleState::OneXRunning);
        first.cost_phase = Some(20);
        clock.observe(&first);
        clock.mark_observation_gap(3);
        let mut after_hidden_wrap = observation(1_000_000_000, ObservedBattleState::OneXRunning);
        after_hidden_wrap.cost_phase = Some(5);
        let next = clock.observe(&after_hidden_wrap);
        assert_eq!(next.quality, ClockQuality::Uncertain);
        assert_eq!(next.frame, 0);
    }
    #[test]
    fn proxy_rejects_slow_deploy_observation() {
        let mut human = HumanClock::default();
        human.observe(&observation(0, ObservedBattleState::OneXRunning));
        let mut proxy = ProxyClock::default();
        proxy.accept_handoff(human.trusted_handoff().unwrap());
        let update = proxy.observe(&observation(
            1_000_000_000,
            ObservedBattleState::DeployingOperator,
        ));
        assert_eq!(update.transition, ClockTransition::Frozen);
        assert_eq!(update.quality, ClockQuality::Uncertain);
    }
    #[test]
    fn trusted_handoff_preserves_frame_and_suspends_source() {
        let mut human = HumanClock::default();
        human.observe(&observation(0, ObservedBattleState::OneXRunning));
        human.observe(&observation(
            1_000_000_000,
            ObservedBattleState::OneXRunning,
        ));
        let handoff = human.trusted_handoff().unwrap();
        assert!(!human.snapshot().receiving_observations);
        let mut proxy = ProxyClock::default();
        let accepted = proxy.accept_handoff(handoff);
        assert_eq!(accepted.frame, 30);
        assert_eq!(accepted.source_timestamp_ns, Some(1_000_000_000.0));
    }
    #[test]
    fn leaving_battle_resets_after_stable_observations() {
        let mut clock = HumanClock::default();
        clock.observe(&observation(0, ObservedBattleState::OneXRunning));
        let mut update = clock.freeze();
        for index in 0..EXIT_CONFIRM_OBSERVATIONS {
            update = clock.observe(&observation(
                1_100_000_000 + u64::from(index) * 33_333_333,
                ObservedBattleState::NotInBattle,
            ));
        }
        assert_eq!(update.transition, ClockTransition::Exited);
        assert_eq!(update.quality, ClockQuality::Waiting);
    }
}
