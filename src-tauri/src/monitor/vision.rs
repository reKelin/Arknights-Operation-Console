use super::ObservedBattleState;
use crate::settings::AppSettings;
use crate::stage::StageRecognition;

#[derive(Clone, Copy, Debug)]
pub struct VisionConfig {
    pub frames_per_cost: u16,
    pub game_ui_scale: u8,
    pub recording_analysis: bool,
}

impl Default for VisionConfig {
    fn default() -> Self {
        Self {
            frames_per_cost: 30,
            game_ui_scale: 100,
            recording_analysis: false,
        }
    }
}

impl From<&AppSettings> for VisionConfig {
    fn from(settings: &AppSettings) -> Self {
        Self {
            frames_per_cost: settings.frames_per_cost,
            game_ui_scale: settings.game_ui_scale,
            recording_analysis: false,
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
    pub stage_recognition: Option<StageRecognition>,
    pub title_candidate: bool,
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
    green_ratio: f64,
    sampled_luma: f64,
    title_bright: f64,
    selected_panel: bool,
    deployment_tiles: bool,
    recording: bool,
    pause_shape: Option<f64>,
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

    let gear_ratio = frame
        .threshold_ratio(frame.reference_rect(20, 10, 90, 90), 80)
        .max(frame.threshold_ratio(frame.reference_rect(60, 20, 150, 125), 80));
    let has_battle_anchor = gear_ratio >= 0.04;
    let speed_rect = frame.reference_rect(1609, 42, 1691, 119);
    let pause_rect = frame.reference_rect(1782, 57, 1845, 104);
    let speed_bright = frame.threshold_ratio(speed_rect, 180);
    let pause_bright = frame.threshold_ratio(pause_rect, 180);
    let features = VisualFeatures {
        pause_shape: config
            .recording_analysis
            .then(|| frame.normalized_shape(pause_rect)),
        recording: config.recording_analysis,
        selected_panel: config.recording_analysis
            && frame.color_ratio(frame.reference_rect(0, 490, 450, 510), false) > 0.12,
        deployment_tiles: config.recording_analysis
            && frame.color_ratio(frame.reference_rect(600, 200, 1650, 800), true) > 0.04,
        gear_ratio,
        speed_bright,
        pause_bright,
        pause_overlay: frame.threshold_ratio(frame.reference_rect(650, 390, 1270, 660), 180),
        green_ratio: frame.green_ratio(frame.reference_rect(100, 120, 1820, 900), 4),
        sampled_luma: frame.sampled_average_luma(16),
        title_bright: frame.threshold_ratio(frame.reference_rect(500, 300, 1420, 760), 180),
    };
    let (battle_state, confidence) = if config.recording_analysis {
        frame.recording_state(features)
    } else {
        classify_battle(features)
    };
    let title_candidate = features.gear_ratio < 0.04
        && features.sampled_luma < 105.0
        && features.title_bright >= 0.01;

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
        stage_recognition: None,
        title_candidate,
    })
}

fn classify_battle(features: VisualFeatures) -> (ObservedBattleState, u8) {
    let has_battle_anchor = features.gear_ratio >= 0.04;
    let controls_visible = features.pause_bright >= 0.12;
    if has_battle_anchor && features.selected_panel {
        if features.pause_shape.is_some_and(|shape| shape < 0.265) {
            return (ObservedBattleState::Paused, 92);
        }
        if features.deployment_tiles {
            return (ObservedBattleState::DeployingOperator, 84);
        }
        if !controls_visible {
            return (ObservedBattleState::AdjustingOperatorFacing, 78);
        }
        if features.pause_bright < 0.265 {
            return (ObservedBattleState::Paused, 92);
        }
        return (ObservedBattleState::PointTwoXRunning, 84);
    }
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
        if features.recording {
            return (ObservedBattleState::Unknown, 40);
        }
        if features.green_ratio >= 0.02 {
            (ObservedBattleState::DeployingOperator, 84)
        } else {
            (
                ObservedBattleState::AdjustingOperatorFacing,
                if features.pause_overlay >= 0.01 {
                    82
                } else {
                    78
                },
            )
        }
    } else if features.sampled_luma < 75.0 && features.title_bright >= 0.02 {
        (ObservedBattleState::BattleBegin, 86)
    } else {
        (ObservedBattleState::NotInBattle, 82)
    }
}

// 连通块坐标使用调用方的采样网格，避免把按钮的屏幕位置当作按钮形状。
#[derive(Debug)]
pub(super) struct Component {
    pub left: usize,
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
    pub area: usize,
}

pub(super) fn components(mut mask: Vec<bool>, width: usize) -> Vec<Component> {
    let mut result = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] {
            continue;
        }
        let mut stack = vec![start];
        mask[start] = false;
        let mut part = Component {
            left: start % width,
            right: start % width + 1,
            top: start / width,
            bottom: start / width + 1,
            area: 0,
        };
        while let Some(i) = stack.pop() {
            let x = i % width;
            let y = i / width;
            part.left = part.left.min(x);
            part.right = part.right.max(x + 1);
            part.top = part.top.min(y);
            part.bottom = part.bottom.max(y + 1);
            part.area += 1;
            for next in [
                (x > 0).then(|| i - 1),
                (x + 1 < width).then_some(i + 1),
                (y > 0).then(|| i - width),
                (i + width < mask.len()).then_some(i + width),
            ]
            .into_iter()
            .flatten()
            {
                if mask[next] {
                    mask[next] = false;
                    stack.push(next);
                }
            }
        }
        if part.area >= 5 {
            result.push(part);
        }
    }
    result
}

impl FrameView<'_> {
    fn white_components(&self, left: u32, top: u32, right: u32, bottom: u32) -> Vec<Component> {
        let mut colors = Vec::new();
        for y in (top..bottom).step_by(2) {
            for x in (left..right).step_by(2) {
                colors.push(
                    self.pixel(self.reference_x(x), self.reference_y(y))
                        .unwrap_or_default(),
                );
            }
        }
        let mut brightness = colors
            .iter()
            .map(|&(r, g, b)| r.max(g).max(b))
            .collect::<Vec<_>>();
        brightness.sort_unstable();
        let threshold = (f64::from(brightness[brightness.len() * 99 / 100]) * 0.7).max(60.0);
        components(
            colors
                .into_iter()
                .map(|(r, g, b)| {
                    f64::from(r.max(g).max(b)) > threshold && r.max(g).max(b) - r.min(g).min(b) < 45
                })
                .collect(),
            ((right - left) / 2) as usize,
        )
    }

    fn recording_state(&self, features: VisualFeatures) -> (ObservedBattleState, u8) {
        use ObservedBattleState::*;
        if features.gear_ratio < 0.04 {
            return classify_battle(features);
        }
        let pause = self.white_components(1740, 30, 1890, 130);
        let bars = pause
            .iter()
            .filter(|c| {
                let w = c.right - c.left;
                let h = c.bottom - c.top;
                (14..=28).contains(&h)
                    && w * 4 >= h
                    && w * 4 <= h * 3
                    && c.area * 100 >= w * h * 45
                    && c.top > 0
                    && c.bottom < 50
            })
            .collect::<Vec<_>>();
        let running_pair = bars.iter().find(|a| {
            bars.iter().any(|b| {
                a.left < b.left
                    && b.left - a.left < 25
                    && a.top.abs_diff(b.top) <= 2
                    && a.bottom.abs_diff(b.bottom) <= 2
            })
        });
        let paused = pause.iter().find(|c| {
            let w = c.right - c.left;
            let h = c.bottom - c.top;
            (14..=28).contains(&h)
                && w * 10 >= h * 9
                && w * 10 <= h * 15
                && c.area * 100 >= w * h * 35
                && c.area * 100 <= w * h * 70
                && c.top > 0
                && c.bottom < 50
        });
        // 紧凑 HUD 保留已有独立帧级校准（含拖动变暗/朝向冻结）；
        // 根据成对竖线或三角图标的位置选择布局，忽略附近无关亮色。
        if running_pair.is_some_and(|c| c.left >= 30) || paused.is_some_and(|c| c.left >= 30) {
            return classify_battle(features);
        }
        let running = running_pair.is_some();
        if paused.is_some() && !running {
            return (Paused, 92);
        }
        if running {
            if features.selected_panel {
                return (
                    if features.deployment_tiles {
                        DeployingOperator
                    } else {
                        PointTwoXRunning
                    },
                    84,
                );
            }
            let speed = self.white_components(1560, 30, 1760, 130);
            if let Some(arrow) = speed.iter().find(|c| {
                let w = c.right - c.left;
                let h = c.bottom - c.top;
                c.top >= 24
                    && c.bottom < 48
                    && (7..=15).contains(&h)
                    && (8..=30).contains(&w)
                    && c.area * 100 >= w * h * 35
            }) {
                return (
                    if (arrow.right - arrow.left) * 10 > (arrow.bottom - arrow.top) * 16 {
                        TwoXRunning
                    } else {
                        OneXRunning
                    },
                    95,
                );
            }
            return (Unknown, 40);
        }
        // 装载/结算画面可能碰巧有亮色齿轮区域，但没有有效控制图标。
        (NotInBattle, 82)
    }

    // 按图标自身对比度判断三角/双竖线；拖动时图标会变暗，绝对亮度不能判暂停。
    fn normalized_shape(&self, rect: Rect) -> f64 {
        let mut values = Vec::new();
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                if let Some((r, g, b)) = self.pixel(x, y) {
                    values.push(r.max(g).max(b));
                }
            }
        }
        if values.is_empty() {
            return 0.0;
        }
        values.sort_unstable();
        let low = f64::from(values[values.len() * 15 / 100]);
        let high = f64::from(values[values.len() * 90 / 100]);
        let threshold = low + (high - low) * 0.65;
        values.iter().filter(|&&v| f64::from(v) > threshold).count() as f64 / values.len() as f64
    }
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
        let step = if (rect.right - rect.left) * (rect.bottom - rect.top) > 10_000 {
            (4.0 * self.scale).round().max(1.0) as usize
        } else {
            1
        };
        for y in (rect.top..rect.bottom).step_by(step) {
            for x in (rect.left..rect.right).step_by(step) {
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

    fn green_ratio(&self, rect: Rect, reference_step: u32) -> f64 {
        let step = (reference_step as f64 * self.scale).round().max(1.0) as usize;
        let mut matching = 0_u64;
        let mut total = 0_u64;
        for y in (rect.top..rect.bottom).step_by(step) {
            for x in (rect.left..rect.right).step_by(step) {
                if let Some((r, g, b)) = self.pixel(x, y) {
                    total += 1;
                    if g > 120
                        && f64::from(g) > f64::from(r) * 1.25
                        && f64::from(g) > f64::from(b) * 1.15
                    {
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

    fn color_ratio(&self, rect: Rect, yellow: bool) -> f64 {
        let step = (4.0 * self.scale).round().max(1.0) as usize;
        let mut matching = 0;
        let mut total = 0;
        for y in (rect.top..rect.bottom).step_by(step) {
            for x in (rect.left..rect.right).step_by(step) {
                if let Some((r, g, b)) = self.pixel(x, y) {
                    total += if yellow { 4 } else { 1 };
                    matching += if yellow {
                        if r > 140 && g > 160 && b < 90 {
                            4
                        } else {
                            i32::from(
                                g > 120
                                    && u16::from(g) * 10 > u16::from(r) * 13
                                    && u16::from(g) * 10 > u16::from(b) * 14,
                            )
                        }
                    } else {
                        i32::from(b > 150 && g > 90 && r < 70)
                    };
                }
            }
        }
        if total == 0 {
            0.0
        } else {
            f64::from(matching) / f64::from(total)
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
        green_ratio: f64,
        sampled_luma: f64,
        title_bright: f64,
    }

    #[test]
    fn recording_panel_distinguishes_selection_from_green_background() {
        let mut features = VisualFeatures {
            pause_shape: None,
            recording: true,
            gear_ratio: 0.2,
            speed_bright: 0.082,
            pause_bright: 0.274,
            pause_overlay: 0.02,
            green_ratio: 0.3,
            sampled_luma: 100.0,
            title_bright: 0.02,
            selected_panel: false,
            deployment_tiles: false,
        };
        assert_eq!(
            classify_battle(features).0,
            ObservedBattleState::TwoXRunning
        );
        features.selected_panel = true;
        features.speed_bright = 0.0;
        assert_eq!(
            classify_battle(features).0,
            ObservedBattleState::PointTwoXRunning
        );
        features.deployment_tiles = true;
        assert_eq!(
            classify_battle(features).0,
            ObservedBattleState::DeployingOperator
        );
        features.pause_shape = Some(0.23);
        assert_eq!(classify_battle(features).0, ObservedBattleState::Paused);
        features.pause_shape = Some(0.29);
        assert_eq!(
            classify_battle(features).0,
            ObservedBattleState::DeployingOperator
        );
    }

    #[test]
    fn recording_controls_follow_icons_across_layouts() {
        use ObservedBattleState::*;
        for (bytes, selected, expected) in [
            (
                include_bytes!("../../tests/fixtures/monitor/controls/one-x.png").as_slice(),
                false,
                OneXRunning,
            ),
            (
                include_bytes!("../../tests/fixtures/monitor/controls/two-x.png").as_slice(),
                false,
                TwoXRunning,
            ),
            (
                include_bytes!("../../tests/fixtures/monitor/controls/paused.png").as_slice(),
                false,
                Paused,
            ),
            (
                include_bytes!("../../tests/fixtures/monitor/controls/selected-paused.png")
                    .as_slice(),
                true,
                Paused,
            ),
            (
                include_bytes!("../../tests/fixtures/monitor/controls/loading.png").as_slice(),
                false,
                NotInBattle,
            ),
            (
                include_bytes!("../../tests/fixtures/monitor/controls/sr8-one-x.png").as_slice(),
                false,
                OneXRunning,
            ),
        ] {
            let patch = image::load_from_memory(bytes).unwrap().to_rgba8();
            let mut data = vec![0; 960 * 540 * 4];
            for (x, y, p) in patch.enumerate_pixels() {
                let i = (((y + 15) * 960 + x + 780) * 4) as usize;
                data[i..i + 4].copy_from_slice(&[p[2], p[1], p[0], 255]);
            }
            let frame = FrameView {
                data: &data,
                width: 960,
                height: 540,
                row_pitch: 3840,
                scale: 0.5,
                offset_x: 0.0,
                offset_y: 0.0,
            };
            let features = VisualFeatures {
                gear_ratio: 0.1,
                speed_bright: frame.threshold_ratio(frame.reference_rect(1609, 42, 1691, 119), 180),
                pause_bright: frame.threshold_ratio(frame.reference_rect(1782, 57, 1845, 104), 180),
                pause_overlay: 0.0,
                green_ratio: 0.0,
                sampled_luma: 100.0,
                title_bright: 0.0,
                selected_panel: selected,
                deployment_tiles: false,
                recording: true,
                pause_shape: None,
            };
            assert_eq!(frame.recording_state(features).0, expected);
        }
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
    fn maps_synthetic_cost_bar_to_configured_phase() {
        let width = 640_u32;
        let height = 360_u32;
        let mut frame = vec![0; (width * height * 4) as usize];
        for y in 3..30 {
            for x in 7..30 {
                let offset = ((y * width + x) * 4) as usize;
                frame[offset..offset + 3].fill(80);
            }
        }
        for y in 269..=273 {
            for x in 580..610 {
                let offset = ((y * width + x) * 4) as usize;
                frame[offset..offset + 3].fill(255);
            }
        }

        let observation =
            analyze_bgra(&frame, width, height, width * 4, 0, VisionConfig::default()).unwrap();

        assert_eq!(observation.cost_phase, Some(15));
        assert!(!observation.cost_full);
    }

    #[test]
    fn classifies_features_measured_from_obs_recording() {
        let fixtures: Vec<FeatureFixture> = serde_json::from_str(include_str!(
            "../../tests/fixtures/monitor/obs-features.json"
        ))
        .unwrap();

        for fixture in fixtures {
            let (actual, _) = classify_battle(VisualFeatures {
                pause_shape: None,
                gear_ratio: fixture.gear_ratio,
                speed_bright: fixture.speed_bright,
                pause_bright: fixture.pause_bright,
                pause_overlay: fixture.pause_overlay,
                green_ratio: fixture.green_ratio,
                sampled_luma: fixture.sampled_luma,
                title_bright: fixture.title_bright,
                selected_panel: false,
                deployment_tiles: false,
                recording: false,
            });
            assert_eq!(actual, fixture.expected, "sample {}", fixture.source);
        }
    }
}
