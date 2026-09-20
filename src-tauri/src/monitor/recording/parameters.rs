use std::collections::{BTreeMap, BTreeSet};

use super::analysis::FacingDirection;
use super::{OperatorNames, reference_pixel};
use crate::executor::{
    geometry::{ProjectionMap, project_tile},
    resources::ExecutionResources,
};
use crate::stage::StageCatalog;

fn cache_root() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("io.github.kelin.arknights-operation-console")
        .join("execution")
}

#[derive(Default)]
pub(super) struct Parameters {
    pub roster: BTreeSet<String>,
    pub map: Option<ProjectionMap>,
    pub map_attempted: bool,
    pub bar_checked: bool,
    pub squad_attempts: u8,
    deployed: BTreeMap<String, String>,
    name_glyphs: BTreeMap<String, [bool; 256]>,
}

impl Parameters {
    pub fn verify_bar(&mut self, data: &[u8], width: u32, height: u32, names: &OperatorNames) {
        self.bar_checked = true;
        for id in super::portraits::deployment_bar(data, width, height) {
            self.roster.insert(id.clone());
            if let Some(unit) = names.operators.iter().find(|u| u.id == id) {
                self.roster.extend(unit.tokens.iter().cloned());
            }
        }
        crate::diagnostics::info(
            "recording.roster",
            &format!("开场部署栏及关联召唤物={:?}", self.roster),
        );
    }
    pub fn read_squad(
        &mut self,
        data: &[u8],
        width: u32,
        height: u32,
        ocr: &crate::monitor::ocr::StageOcrRecognizer,
        names: &OperatorNames,
    ) {
        self.squad_attempts += 1;
        for (left_edge, step, bottom) in [(280, 196, 878), (160, 232, 926)] {
            for y in [486, bottom] {
                for column in 0..if y == 486 { 7 } else { 6 } {
                    let left = left_edge + column * step;
                    if let Ok(image) = crate::monitor::ocr::crop_region(
                        data,
                        width,
                        height,
                        width * 4,
                        [left, y, left + 180, y + 42],
                    ) && let Ok(text) = ocr.recognize_text(image.enlarged())
                        && let Some(id) = self.selected_unit(&text, names)
                    {
                        if column == 6 {
                            crate::diagnostics::debug(
                                "recording.support",
                                &format!("助战候选={id}"),
                            );
                        }
                        self.roster.insert(id.clone());
                        if let Some(unit) = names.operators.iter().find(|u| u.id == id) {
                            self.roster.extend(unit.tokens.iter().cloned());
                        }
                    }
                }
            }
        }
        crate::diagnostics::debug(
            "recording.squad",
            &format!("编队及助战位候选={:?}", self.roster),
        );
    }
    pub fn read_roster(&mut self, text: &str, names: &OperatorNames) {
        let text = text.replace(char::is_whitespace, "");
        if !text.contains("开始行动") {
            return;
        }
        let matches = names
            .operators
            .iter()
            .filter(|u| text.contains(&u.name))
            .collect::<Vec<_>>();
        for unit in &matches {
            if !matches
                .iter()
                .any(|other| other.name != unit.name && other.name.contains(&unit.name))
            {
                self.roster.insert(unit.id.clone());
                self.roster.extend(unit.tokens.iter().cloned());
            }
        }
    }

    pub fn load_map(&mut self, catalog: &StageCatalog, id: &str) {
        self.map_attempted = true;
        let root = cache_root();
        match ExecutionResources::new(root)
            .and_then(|resources| resources.load_projection(catalog, id))
        {
            Ok((_, map)) => self.map = Some(map),
            Err(error) => crate::diagnostics::info(
                "recording.map",
                &format!("地图未载入，格子留待校对：{error}"),
            ),
        }
    }

    pub fn selected_unit(&self, text: &str, names: &OperatorNames) -> Option<String> {
        let text = text.replace(char::is_whitespace, "");
        let mut matches = names
            .operators
            .iter()
            .filter(|unit| {
                (unit.name.chars().count() >= 2 && text.contains(&unit.name)) || text == unit.name
            })
            .collect::<Vec<_>>();
        let longest = matches.iter().map(|u| u.name.chars().count()).max()?;
        matches.retain(|u| u.name.chars().count() == longest);
        if matches.len() > 1 && !self.roster.is_empty() {
            matches.retain(|u| self.roster.contains(&u.id));
        }
        (matches.len() == 1).then(|| matches[0].id.clone())
    }

    pub fn remember_name(
        &mut self,
        unit: &str,
        data: &[u8],
        w: u32,
        h: u32,
        names: &OperatorNames,
    ) {
        if names
            .operators
            .iter()
            .any(|u| u.id == unit && u.name.chars().count() == 1)
            && let Some(glyph) = name_glyph(data, w, h)
        {
            self.name_glyphs.insert(unit.to_string(), glyph);
        }
    }

    pub fn match_name(&self, data: &[u8], w: u32, h: u32) -> Option<String> {
        let glyph = name_glyph(data, w, h)?;
        let mut scores = self
            .name_glyphs
            .iter()
            .map(|(id, known)| (glyph.iter().zip(known).filter(|(a, b)| a == b).count(), id))
            .collect::<Vec<_>>();
        scores.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        let (score, id) = scores.first()?;
        (*score >= 230 && *score >= scores.get(1).map_or(0, |p| p.0) + 20).then(|| (*id).clone())
    }

    pub fn tile(
        &self,
        data: &[u8],
        width: u32,
        height: u32,
        unit: &str,
        deploy: bool,
    ) -> Option<String> {
        if !deploy {
            return self.deployed.get(unit).cloned();
        }
        let map = self.map.as_ref()?;
        let center = if unit == "token_10064_wang_stone1" {
            None
        } else {
            Some(diamond_center(data, width, height)?)
        };
        let mut scores = Vec::new();
        for row in 0..map.height.min(9) {
            for col in 0..map.width.min(36) {
                if map
                    .tiles
                    .get(map.height - 1 - row)
                    .and_then(|line| line.get(col))
                    .is_none_or(|tile| tile.buildable_type == 0)
                {
                    continue;
                }
                let tile = format!("{}{}", (b'A' + row as u8) as char, col + 1);
                let (x, y) = project_tile(&tile, map, true).ok()?;
                let x = (x * 1920.0).round() as i32;
                let y = (y * 1080.0).round() as i32;
                if !(400..1870).contains(&x) || !(180..880).contains(&y) {
                    continue;
                }
                let score = if unit == "token_10064_wang_stone1" {
                    orb_score(data, width, height, x, y)
                } else {
                    {
                        let (cx, cy) = center?;
                        (1.0 - ((f64::from(x) - cx).hypot(f64::from(y) - cy)) / 65.0).max(0.0)
                    }
                };
                scores.push((score, tile));
            }
        }
        scores.sort_by(|a, b| b.0.total_cmp(&a.0));
        let (best, tile) = scores.first()?;
        let second = scores.get(1).map_or(0.0, |s| s.0);
        if *best >= 0.50 && best - second >= 0.12 {
            Some(tile.clone())
        } else {
            None
        }
    }

    pub fn complete(&mut self, unit: &str, tile: &str, deploy: bool, retreat: bool) {
        if deploy {
            self.deployed.insert(unit.to_string(), tile.to_string());
        }
        if retreat {
            self.deployed.remove(unit);
        }
    }
}

// 字形只从已由 OCR 或部署栏变化确定身份的单位学习，不能凭单字长度猜名字。
fn name_glyph(data: &[u8], w: u32, h: u32) -> Option<[bool; 256]> {
    let bright = |x, y| {
        let (r, g, b) = reference_pixel(data, w, h, x, y);
        r.min(g).min(b) > 85 && r.max(g).max(b) - r.min(g).min(b) < 25
    };
    let points = (302..340)
        .step_by(2)
        .flat_map(|y| (8..70).step_by(2).map(move |x| (x, y)))
        .filter(|&(x, y)| bright(x, y))
        .collect::<Vec<_>>();
    let left = points.iter().map(|p| p.0).min()?;
    let right = points.iter().map(|p| p.0).max()?;
    let top = points.iter().map(|p| p.1).min()?;
    let bottom = points.iter().map(|p| p.1).max()?;
    if !(16..=36).contains(&(right - left)) || !(16..=36).contains(&(bottom - top)) {
        return None;
    }
    Some(std::array::from_fn(|i| {
        bright(
            left + (i as u32 % 16) * (right - left) / 15,
            top + (i as u32 / 16) * (bottom - top) / 15,
        )
    }))
}

fn pixel(data: &[u8], w: u32, h: u32, x: i32, y: i32) -> (u8, u8, u8) {
    if !(0..1920).contains(&x) || !(0..1080).contains(&y) {
        return (0, 0, 0);
    }
    reference_pixel(data, w, h, x as u32, y as u32)
}

fn white(data: &[u8], w: u32, h: u32, x: i32, y: i32) -> bool {
    let (r, g, b) = pixel(data, w, h, x, y);
    r.min(g).min(b) > 150 && r.max(g).max(b) - r.min(g).min(b) < 55
}

// 用四条长白边的对角线投票定位朝向菱形；截断或短文本不构成落点证据。
fn diamond_center(data: &[u8], w: u32, h: u32) -> Option<(f64, f64)> {
    let mut sum = vec![0usize; 1600];
    let mut diff = vec![0usize; 1600];
    for y in 95..490 {
        for x in 180..960 {
            if white(data, w, h, x * 2, y * 2) {
                sum[(x + y) as usize] += 1;
                diff[(x - y + 540) as usize] += 1;
            }
        }
    }
    let pair = |hist: &[usize]| {
        let smooth = (0..hist.len())
            .map(|i| {
                hist[i.saturating_sub(2)..(i + 3).min(hist.len())]
                    .iter()
                    .sum::<usize>()
            })
            .collect::<Vec<_>>();
        let mut best = (0usize, 0usize, 0usize);
        for distance in 220..300 {
            for left in 0..smooth.len() - distance {
                let score = smooth[left].min(smooth[left + distance]);
                if score > best.0 {
                    best = (score, left, left + distance);
                }
            }
        }
        best
    };
    let u = pair(&sum);
    let v = pair(&diff);
    if u.0 < 65 || v.0 < 65 || (u.2 - u.1).abs_diff(v.2 - v.1) > 60 {
        return None;
    }
    let u = (u.1 + u.2) as f64 / 2.0;
    let v = (v.1 + v.2) as f64 / 2.0 - 540.0;
    let (cx, cy) = ((u + v) as i32, (u - v) as i32);
    // PAUSE 字样也会产生长白线；部署菱形左上必须同时出现红色取消区域。
    let cancel = (-330..-50)
        .step_by(4)
        .flat_map(|dx| (-300..-100).step_by(4).map(move |dy| (dx, dy)))
        .filter(|(dx, dy)| {
            let (r, g, b) = pixel(data, w, h, cx + dx, cy + dy);
            r > 100 && g < 60 && b < 60
        })
        .count();
    (cancel > 150).then_some((u + v, u - v))
}

fn orb_score(data: &[u8], w: u32, h: u32, x: i32, y: i32) -> f64 {
    let mut best: f64 = 0.0;
    for dx in (-24..=24).step_by(6) {
        for dy in (-24..=24).step_by(6) {
            let (r, g, b) = pixel(data, w, h, x + dx, y + dy);
            if r.max(g).max(b) > 45 {
                continue;
            }
            let disk = [-6, 0, 6]
                .into_iter()
                .flat_map(|ox| [-6, 0, 6].into_iter().map(move |oy| (ox, oy)))
                .filter(|(ox, oy)| {
                    let (r, g, b) = pixel(data, w, h, x + dx + ox, y + dy + oy);
                    r.max(g).max(b) < 65
                })
                .count();
            if disk < 6 {
                continue;
            }
            // 有效落点出现橙色斜纹，不能把已部署棋子或人物阴影当作拖动物。
            let orange = (-100..=100)
                .step_by(16)
                .flat_map(|oy| (-100..=100).step_by(16).map(move |ox| (ox, oy)))
                .filter(|(ox, oy)| {
                    let (r, g, b) = pixel(data, w, h, x + dx + ox, y + dy + oy);
                    r > 130 && g > 65 && g < 180 && b < 100 && u32::from(r) * 10 > u32::from(g) * 14
                })
                .count();
            if orange < 18 {
                continue;
            }
            let ring = [
                (-12, 0),
                (12, 0),
                (0, -12),
                (0, 12),
                (-8, -8),
                (8, 8),
                (-8, 8),
                (8, -8),
            ];
            let bright = ring
                .iter()
                .filter(|(rx, ry)| {
                    let (r, g, b) = pixel(data, w, h, x + dx + rx, y + dy + ry);
                    u32::from(r) + u32::from(g) + u32::from(b) > 330
                })
                .count();
            // 指针的白色斜轴及其暗边必须同时位于黑球下方，文字和头像白块不构成指针。
            let mut cursor = false;
            for px in (-10..=14).step_by(4) {
                for py in (30..=62).step_by(4) {
                    let line = (0..=32)
                        .step_by(4)
                        .filter(|k| white(data, w, h, x + dx + px + k, y + dy + py + k * 3 / 2))
                        .count();
                    let edge = (0..=32)
                        .step_by(4)
                        .filter(|k| {
                            let (r, g, b) =
                                pixel(data, w, h, x + dx + px + k + 6, y + dy + py + k * 3 / 2 - 6);
                            r.max(g).max(b) < 110
                        })
                        .count();
                    cursor |= line >= 6 && edge >= 5;
                }
            }
            if cursor {
                best = best.max(bright as f64 / 8.0);
            }
        }
    }
    best
}

pub(super) fn facing(
    data: &[u8],
    w: u32,
    h: u32,
    map: &ProjectionMap,
    tile: &str,
) -> Option<FacingDirection> {
    let (x, y) = project_tile(tile, map, true).ok()?;
    let (x, y) = ((x * 1920.0) as i32, (y * 1080.0) as i32);
    let mut votes = [0usize; 4];
    for dy in -90i32..=90 {
        for dx in -90i32..=90 {
            let radius = dx.abs().max(dy.abs());
            if !(45..=85).contains(&radius) {
                continue;
            }
            let (r, g, b) = pixel(data, w, h, x + dx, y + dy);
            if r > 140 && g > 100 && b < 100 && r > g {
                let side = if dx.abs() > dy.abs() {
                    if dx > 0 { 1 } else { 3 }
                } else if dy > 0 {
                    2
                } else {
                    0
                };
                votes[side] += 1;
            }
        }
    }
    let mut ordered = votes.iter().copied().enumerate().collect::<Vec<_>>();
    ordered.sort_by_key(|v| std::cmp::Reverse(v.1));
    (ordered[0].1 > 70 && ordered[0].1 > ordered[1].1 * 2).then(|| {
        [
            FacingDirection::Up,
            FacingDirection::Right,
            FacingDirection::Down,
            FacingDirection::Left,
        ][ordered[0].0]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn squad_candidates_do_not_exclude_support_or_match_short_name_fragments() {
        let names: OperatorNames =
            serde_json::from_str(include_str!("../../../data/operators.json")).unwrap();
        let mut parameters = Parameters::default();
        parameters.read_roster("开始行动 赤刃明霄陈", &names);
        assert!(parameters.roster.contains("char_1050_chen3"));
        assert!(!parameters.roster.contains("char_010_chen"));
        assert_eq!(
            parameters.selected_unit("望", &names).as_deref(),
            Some("char_2027_wang")
        );
        assert_eq!(parameters.selected_unit("赤刀明霄陈", &names), None);
        assert!(
            parameters
                .match_name(&vec![0; 960 * 540 * 4], 960, 540)
                .is_none()
        );
        parameters.complete("char_2027_wang", "C4", true, false);
        assert_eq!(
            parameters
                .tile(&[], 960, 540, "char_2027_wang", false)
                .as_deref(),
            Some("C4")
        );
        parameters.complete("char_2027_wang", "C4", false, true);
        assert!(
            parameters
                .tile(&[], 960, 540, "char_2027_wang", false)
                .is_none()
        );
    }
}
