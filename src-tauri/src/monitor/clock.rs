use super::{ObservedBattleState, VisualObservation};

const NANOS_PER_SECOND: u128 = 1_000_000_000;
const GAME_TICKS_PER_SECOND: u128 = 30;
const SPEED_DENOMINATOR: u128 = 5;
const EXIT_CONFIRM_OBSERVATIONS: u8 = 30;
const TRUSTED_CONFIDENCE: u8 = 70;

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
    pub speed: u8,
    pub error_frames: u32,
}

#[derive(Default)]
pub struct ObservationClock {
    active: bool,
    frame: u32,
    remainder: u128,
    last_timestamp_ns: Option<u64>,
    last_cost_phase: Option<u16>,
    cost_cycles: u32,
    cost_origin: Option<u32>,
    outside_streak: u8,
    error_frames: u32,
}

impl ObservationClock {
    pub fn observe(&mut self, observation: &VisualObservation) -> ClockUpdate {
        if matches!(
            observation.battle_state,
            ObservedBattleState::NotInBattle | ObservedBattleState::BattleBegin
        ) {
            return self.observe_outside(observation);
        }
        self.outside_streak = 0;

        if !self.active {
            if observation.confidence < TRUSTED_CONFIDENCE || !observation.battle_state.is_running()
            {
                self.last_timestamp_ns = Some(observation.capture_timestamp_ns);
                return self.update(ClockTransition::Frozen, 0);
            }
            self.start(observation);
            return self.update(
                ClockTransition::Started,
                speed_label(observation.battle_state),
            );
        }

        let elapsed_ns = self
            .last_timestamp_ns
            .map(|previous| observation.capture_timestamp_ns.saturating_sub(previous))
            .unwrap_or(0);
        self.last_timestamp_ns = Some(observation.capture_timestamp_ns);

        if observation.confidence < TRUSTED_CONFIDENCE
            || observation.battle_state == ObservedBattleState::Unknown
        {
            self.error_frames = self
                .error_frames
                .saturating_add(nanos_to_frames(elapsed_ns));
            return self.update(ClockTransition::Frozen, 0);
        }
        if matches!(
            observation.battle_state,
            ObservedBattleState::Paused | ObservedBattleState::AdjustingOperatorFacing
        ) {
            return self.update(ClockTransition::Paused, 0);
        }

        let previous = self.frame;
        self.advance(elapsed_ns, speed_fifths(observation.battle_state));
        if !observation.cost_full {
            self.apply_cost_anchor(observation);
        }
        let transition = if self.frame == previous {
            ClockTransition::None
        } else {
            ClockTransition::Advanced
        };
        self.update(transition, speed_label(observation.battle_state))
    }

    pub fn freeze(&mut self) -> ClockUpdate {
        self.last_timestamp_ns = None;
        self.update(ClockTransition::Frozen, 0)
    }

    fn observe_outside(&mut self, observation: &VisualObservation) -> ClockUpdate {
        self.last_timestamp_ns = Some(observation.capture_timestamp_ns);
        if observation.battle_state == ObservedBattleState::BattleBegin || !self.active {
            self.outside_streak = 0;
            return self.update(ClockTransition::None, 0);
        }
        self.outside_streak = self.outside_streak.saturating_add(1);
        if self.outside_streak < EXIT_CONFIRM_OBSERVATIONS {
            return self.update(ClockTransition::Frozen, 0);
        }
        self.reset();
        self.update(ClockTransition::Exited, 0)
    }

    fn start(&mut self, observation: &VisualObservation) {
        self.active = true;
        self.frame = 0;
        self.remainder = 0;
        self.last_timestamp_ns = Some(observation.capture_timestamp_ns);
        self.last_cost_phase = observation.cost_phase;
        self.cost_cycles = 0;
        self.cost_origin = observation.cost_phase.map(u32::from);
        self.outside_streak = 0;
        self.error_frames = 0;
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn advance(&mut self, elapsed_ns: u64, speed_fifths: u128) {
        let progress =
            self.remainder + u128::from(elapsed_ns) * GAME_TICKS_PER_SECOND * speed_fifths;
        let unit = NANOS_PER_SECOND * SPEED_DENOMINATOR;
        self.frame = self
            .frame
            .saturating_add((progress / unit).min(u128::from(u32::MAX)) as u32);
        self.remainder = progress % unit;
    }

    fn apply_cost_anchor(&mut self, observation: &VisualObservation) {
        let (Some(phase), Some(origin)) = (observation.cost_phase, self.cost_origin) else {
            return;
        };
        let total = u32::from(observation.cost_total.max(1));
        if let Some(previous) = self.last_cost_phase
            && u32::from(previous) * 4 >= total * 3
            && u32::from(phase) * 4 <= total
        {
            self.cost_cycles = self.cost_cycles.saturating_add(1);
        }
        self.last_cost_phase = Some(phase);
        let absolute = self
            .cost_cycles
            .saturating_mul(total)
            .saturating_add(u32::from(phase));
        let target = absolute.saturating_sub(origin);
        if target >= self.frame {
            self.frame = target;
            self.error_frames = 0;
        } else {
            self.error_frames = self.error_frames.max(self.frame - target);
        }
    }

    fn update(&self, transition: ClockTransition, speed: u8) -> ClockUpdate {
        ClockUpdate {
            transition,
            active: self.active,
            frame: self.frame,
            speed,
            error_frames: self.error_frames,
        }
    }
}

fn speed_fifths(state: ObservedBattleState) -> u128 {
    match state {
        ObservedBattleState::TwoXRunning => 10,
        ObservedBattleState::PointTwoXRunning | ObservedBattleState::DeployingOperator => 1,
        _ => 5,
    }
}

fn speed_label(state: ObservedBattleState) -> u8 {
    match state {
        ObservedBattleState::TwoXRunning => 2,
        ObservedBattleState::Paused => 0,
        _ => 1,
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
        }
    }

    #[test]
    fn starts_only_on_running_observation() {
        let mut clock = ObservationClock::default();
        let paused = clock.observe(&observation(0, ObservedBattleState::Paused));
        assert_eq!(paused.transition, ClockTransition::Frozen);

        let started = clock.observe(&observation(1, ObservedBattleState::OneXRunning));
        assert_eq!(started.transition, ClockTransition::Started);
        assert_eq!(started.frame, 0);
    }

    #[test]
    fn follows_one_and_two_x_speed() {
        let mut clock = ObservationClock::default();
        clock.observe(&observation(0, ObservedBattleState::OneXRunning));

        let one_x = clock.observe(&observation(
            1_000_000_000,
            ObservedBattleState::OneXRunning,
        ));
        let two_x = clock.observe(&observation(
            2_000_000_000,
            ObservedBattleState::TwoXRunning,
        ));

        assert_eq!(one_x.frame, 30);
        assert_eq!(two_x.frame, 90);
    }

    #[test]
    fn facing_adjustment_freezes_clock() {
        let mut clock = ObservationClock::default();
        clock.observe(&observation(0, ObservedBattleState::OneXRunning));

        let update = clock.observe(&observation(
            1_000_000_000,
            ObservedBattleState::AdjustingOperatorFacing,
        ));

        assert_eq!(update.transition, ClockTransition::Paused);
        assert_eq!(update.frame, 0);
    }

    #[test]
    fn leaving_battle_resets_after_stable_observations() {
        let mut clock = ObservationClock::default();
        clock.observe(&observation(0, ObservedBattleState::OneXRunning));
        clock.observe(&observation(
            1_000_000_000,
            ObservedBattleState::OneXRunning,
        ));

        let mut update = clock.freeze();
        for index in 0..EXIT_CONFIRM_OBSERVATIONS {
            update = clock.observe(&observation(
                1_100_000_000 + u64::from(index) * 33_333_333,
                ObservedBattleState::NotInBattle,
            ));
        }

        assert_eq!(update.transition, ClockTransition::Exited);
        assert_eq!(update.frame, 0);
    }

    #[test]
    fn cost_phase_wrap_advances_anchor_without_going_backwards() {
        let mut clock = ObservationClock::default();
        let mut first = observation(0, ObservedBattleState::OneXRunning);
        first.cost_phase = Some(25);
        clock.observe(&first);

        let mut before_wrap = observation(100_000_000, ObservedBattleState::OneXRunning);
        before_wrap.cost_phase = Some(29);
        clock.observe(&before_wrap);
        let mut after_wrap = observation(200_000_000, ObservedBattleState::OneXRunning);
        after_wrap.cost_phase = Some(1);
        let update = clock.observe(&after_wrap);

        assert_eq!(update.frame, 7);
        assert_eq!(update.error_frames, 1);
    }

    #[test]
    fn full_cost_does_not_create_false_wrap() {
        let mut clock = ObservationClock::default();
        let mut first = observation(0, ObservedBattleState::OneXRunning);
        first.cost_phase = Some(29);
        clock.observe(&first);

        let mut full = observation(1_000_000_000, ObservedBattleState::OneXRunning);
        full.cost_phase = Some(0);
        full.cost_full = true;
        let update = clock.observe(&full);

        assert_eq!(update.frame, 30);
    }
}
