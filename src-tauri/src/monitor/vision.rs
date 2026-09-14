use super::ObservedBattleState;
use crate::settings::AppSettings;

#[derive(Clone, Copy, Debug)]
pub struct VisionConfig {
    pub frames_per_cost: u16,
    pub game_ui_scale: u8,
}

impl Default for VisionConfig {
    fn default() -> Self {
        Self {
            frames_per_cost: 30,
            game_ui_scale: 100,
        }
    }
}

impl From<&AppSettings> for VisionConfig {
    fn from(settings: &AppSettings) -> Self {
        Self {
            frames_per_cost: settings.frames_per_cost,
            game_ui_scale: settings.game_ui_scale,
        }
    }
}

#[derive(Clone, Debug)]
pub struct VisualObservation {
    pub capture_timestamp_ns: u64,
    pub battle_state: ObservedBattleState,
    pub confidence: u8,
    pub cost_phase: Option<u16>,
    pub cost_total: u16,
    pub cost_full: bool,
}

#[derive(Clone, Copy)]
struct Rect {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

struct FrameView<'a> {
    data: &'a [u8],
    width: u32,
    height: u32,
    row_pitch: u32,
    scale: f64,
    offset_x: f64,
    offset_y: f64,
}

#[derive(Clone, Copy, Debug)]
struct VisualFeatures {
    gear_ratio: f64,
    speed_bright: f64,
    pause_bright: f64,
    pause_overlay: f64,
    sampled_luma: f64,
    title_bright: f64,
}

pub fn analyze_bgra(
    data: &[u8],
    width: u32,
    height: u32,
    row_pitch: u32,
    capture_timestamp_ns: u64,
    config: VisionConfig,
) -> Result<VisualObservation, String> {
    if width < 640 || height < 360 {
        return Err(format!("游戏画面尺寸过小：{width}×{height}"));
    }
    if row_pitch < width.saturating_mul(4) || data.len() < row_pitch as usize * height as usize {
        return Err("捕获帧缓冲区尺寸无效".to_string());
    }
    let scale = (width as f64 / 1920.0).min(height as f64 / 1080.0);
    let viewport_width = 1920.0 * scale;
    let viewport_height = 1080.0 * scale;
    let frame = FrameView {
        data,
        width,
        height,
        row_pitch,
        scale,
        offset_x: (width as f64 - viewport_width) / 2.0,
        offset_y: (height as f64 - viewport_height) / 2.0,
    };

    let gear_ratio = frame.threshold_ratio(frame.reference_rect(20, 10, 90, 90), 80);
    let has_battle_anchor = gear_ratio >= 0.04;
    let speed_rect = frame.reference_rect(1609, 42, 1691, 119);
    let pause_rect = frame.reference_rect(1782, 57, 1845, 104);
    let speed_bright = frame.threshold_ratio(speed_rect, 180);
    let pause_bright = frame.threshold_ratio(pause_rect, 180);
    let features = VisualFeatures {
        gear_ratio,
        speed_bright,
        pause_bright,
        pause_overlay: frame.threshold_ratio(frame.reference_rect(650, 390, 1270, 660), 180),
        sampled_luma: frame.sampled_average_luma(16),
        title_bright: frame.threshold_ratio(frame.reference_rect(500, 300, 1420, 760), 180),
    };
    let (battle_state, confidence) = classify_battle(features);

    let (cost_phase, cost_full) = if has_battle_anchor {
        frame
            .cost_phase(config)
            .map(|(phase, full)| (Some(phase), full))
            .unwrap_or((None, false))
    } else {
        (None, false)
    };

    Ok(VisualObservation {
        capture_timestamp_ns,
        battle_state,
        confidence,
        cost_phase,
        cost_total: config.frames_per_cost,
        cost_full,
    })
}

fn classify_battle(features: VisualFeatures) -> (ObservedBattleState, u8) {
    let has_battle_anchor = features.gear_ratio >= 0.04;
    let controls_visible = features.pause_bright >= 0.12;
    if has_battle_anchor && controls_visible {
        if features.pause_bright < 0.265 {
            (ObservedBattleState::Paused, 92)
        } else if features.speed_bright >= 0.075 {
            (ObservedBattleState::TwoXRunning, 95)
        } else if features.speed_bright >= 0.035 {
            (ObservedBattleState::OneXRunning, 90)
        } else {
            (ObservedBattleState::PointTwoXRunning, 78)
        }
    } else if has_battle_anchor {
        if features.pause_overlay >= 0.012 {
            (ObservedBattleState::Paused, 82)
        } else {
            (ObservedBattleState::DeployingOperator, 74)
        }
    } else if features.sampled_luma < 75.0 && features.title_bright >= 0.02 {
        (ObservedBattleState::BattleBegin, 86)
    } else {
        (ObservedBattleState::NotInBattle, 82)
    }
}

impl FrameView<'_> {
    fn reference_rect(&self, left: u32, top: u32, right: u32, bottom: u32) -> Rect {
        Rect {
            left: self.reference_x(left).min(self.width),
            top: self.reference_y(top).min(self.height),
            right: self.reference_x(right).min(self.width),
            bottom: self.reference_y(bottom).min(self.height),
        }
    }

    fn reference_x(&self, value: u32) -> u32 {
        (self.offset_x + value as f64 * self.scale).round().max(0.0) as u32
    }

    fn reference_y(&self, value: u32) -> u32 {
        (self.offset_y + value as f64 * self.scale).round().max(0.0) as u32
    }

    fn pixel(&self, x: u32, y: u32) -> Option<(u8, u8, u8)> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = y as usize * self.row_pitch as usize + x as usize * 4;
        let pixel = self.data.get(offset..offset + 4)?;
        Some((pixel[2], pixel[1], pixel[0]))
    }

    fn threshold_ratio(&self, rect: Rect, threshold: u8) -> f64 {
        if rect.left >= rect.right || rect.top >= rect.bottom {
            return 0.0;
        }
        let mut matching = 0_u64;
        let mut total = 0_u64;
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                if let Some((r, g, b)) = self.pixel(x, y) {
                    total += 1;
                    if r.max(g).max(b) >= threshold {
                        matching += 1;
                    }
                }
            }
        }
        if total == 0 {
            0.0
        } else {
            matching as f64 / total as f64
        }
    }

    fn sampled_average_luma(&self, reference_step: u32) -> f64 {
        let step = (reference_step as f64 * self.scale).round().max(1.0) as usize;
        let mut sum = 0_u64;
        let mut total = 0_u64;
        for y in (0..self.height).step_by(step) {
            for x in (0..self.width).step_by(step) {
                if let Some((r, g, b)) = self.pixel(x, y) {
                    sum += u64::from(r) + u64::from(g) + u64::from(b);
                    total += 3;
                }
            }
        }
        if total == 0 {
            0.0
        } else {
            sum as f64 / total as f64
        }
    }

    fn cost_phase(&self, config: VisionConfig) -> Option<(u16, bool)> {
        let edge_scale = 0.9 + f64::from(config.game_ui_scale.min(100)) / 1000.0;
        let right = self.reference_x(1919);
        let total_width = (180.0 * self.scale * edge_scale).round().max(20.0) as u32;
        let left = right.saturating_sub(total_width);
        let y = (self.offset_y + (1080.0 - 266.0 * edge_scale) * self.scale)
            .round()
            .max(0.0) as u32;
        let mut widths = Vec::with_capacity(5);
        for row in y.saturating_sub(2)..=y.saturating_add(2).min(self.height - 1) {
            let mut grayscale = 0_u32;
            let mut filled = 0_u32;
            for x in left..right {
                let (r, g, b) = self.pixel(x, row)?;
                let color_spread = r.max(g).max(b) - r.min(g).min(b);
                if color_spread <= 22 {
                    grayscale += 1;
                    if r > 245 && g > 245 && b > 245 {
                        filled = x - left + 1;
                    }
                }
            }
            if grayscale * 10 >= total_width * 8 {
                widths.push(filled);
            }
        }
        if widths.len() < 3 {
            return None;
        }
        widths.sort_unstable();
        let filled = widths[widths.len() / 2];
        let frames = u32::from(config.frames_per_cost);
        let phase = ((filled * frames + total_width / 2) / total_width)
            .min(frames.saturating_sub(1)) as u16;
        Some((phase, filled + 1 >= total_width))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FeatureFixture {
        source: String,
        expected: ObservedBattleState,
        gear_ratio: f64,
        speed_bright: f64,
        pause_bright: f64,
        pause_overlay: f64,
        sampled_luma: f64,
        title_bright: f64,
    }

    #[test]
    fn rejects_invalid_frame_buffer() {
        let error =
            analyze_bgra(&[], 1920, 1080, 1920 * 4, 0, VisionConfig::default()).unwrap_err();

        assert!(error.contains("缓冲区"));
    }

    #[test]
    fn dark_frame_is_outside_battle() {
        let frame = vec![0; 640 * 360 * 4];
        let observation =
            analyze_bgra(&frame, 640, 360, 640 * 4, 0, VisionConfig::default()).unwrap();

        assert_eq!(observation.battle_state, ObservedBattleState::NotInBattle);
    }

    #[test]
    fn classifies_features_measured_from_obs_recording() {
        let fixtures: Vec<FeatureFixture> = serde_json::from_str(include_str!(
            "../../tests/fixtures/monitor/obs-features.json"
        ))
        .unwrap();

        for fixture in fixtures {
            let (actual, _) = classify_battle(VisualFeatures {
                gear_ratio: fixture.gear_ratio,
                speed_bright: fixture.speed_bright,
                pause_bright: fixture.pause_bright,
                pause_overlay: fixture.pause_overlay,
                sampled_luma: fixture.sampled_luma,
                title_bright: fixture.title_bright,
            });
            assert_eq!(actual, fixture.expected, "sample {}", fixture.source);
        }
    }
}
