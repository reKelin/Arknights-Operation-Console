use super::reference_pixel;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Deserialize)]
struct Index {
    entries: Vec<(String, String)>,
}
struct Portrait {
    unit: String,
    feature: [f32; 256],
}

fn normalized(mut values: [f32; 256]) -> Option<[f32; 256]> {
    let mean = values.iter().sum::<f32>() / 256.0;
    for v in &mut values {
        *v -= mean;
    }
    let norm = values.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm < 100.0 {
        return None;
    }
    for v in &mut values {
        *v /= norm;
    }
    Some(values)
}

fn library() -> &'static [Portrait] {
    static LIBRARY: OnceLock<Vec<Portrait>> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let index: Index = serde_json::from_str(include_str!("../../../data/portrait-atlas.json"))
            .expect("内置头像索引无效");
        let image = image::load_from_memory(include_bytes!("../../../data/portrait-atlas.png"))
            .expect("内置头像特征无效")
            .to_luma8();
        index
            .entries
            .into_iter()
            .enumerate()
            .filter_map(|(i, (unit, _))| {
                let mut feature = [0.0; 256];
                for (j, value) in feature.iter_mut().enumerate() {
                    *value = f32::from(image.as_raw()[i * 256 + j]);
                }
                normalized(feature).map(|feature| Portrait { unit, feature })
            })
            .collect()
    })
}

// 先定位职业/费用旗标，再比较局部头像；全目录只在开场运行一次。
pub(super) fn deployment_bar(data: &[u8], w: u32, h: u32) -> Vec<String> {
    match_bar(data, w, h, None)
}

pub(super) fn match_bar(
    data: &[u8],
    w: u32,
    h: u32,
    allowed: Option<&std::collections::BTreeSet<String>>,
) -> Vec<String> {
    let mut starts = Vec::new();
    let mut start = None;
    for x in (100..1918).step_by(2) {
        let bright = (878..938)
            .step_by(2)
            .filter(|&y| {
                let (r, g, b) = reference_pixel(data, w, h, x, y);
                r.min(g).min(b) > 90 && r.max(g).max(b) - r.min(g).min(b) < 45
            })
            .count();
        if bright >= 4 {
            if start.is_none() {
                start = Some(x);
            }
        } else if let Some(from) = start.take()
            && x - from >= 8
        {
            starts.extend((from.saturating_sub(32)..x.saturating_sub(32)).step_by(24));
        }
    }
    if let Some(from) = start
        && 1918 - from >= 8
    {
        starts.extend((from.saturating_sub(32)..1886).step_by(24));
    }
    let templates = library()
        .iter()
        .filter(|p| allowed.is_none_or(|ids| ids.contains(&p.unit)))
        .collect::<Vec<_>>();
    let mut found = BTreeMap::<String, f32>::new();
    for x in starts {
        let mut scores = BTreeMap::<&str, f32>::new();
        for dx in [-12, 0, 12] {
            for y in (872..=936).step_by(8) {
                for size in (136..=200).step_by(8) {
                    let left = (x as i32 + dx).max(0) as u32;
                    if left + size > 1930 {
                        continue;
                    }
                    let patch = image::GrayImage::from_fn(size / 3, size / 4, |px, py| {
                        let (r, g, b) = reference_pixel(
                            data,
                            w,
                            h,
                            (left + size / 6 + px * 2).min(1919),
                            (y + size / 4 + py * 2).min(1079),
                        );
                        image::Luma([
                            ((u32::from(r) * 77 + u32::from(g) * 150 + u32::from(b) * 29) / 256)
                                as u8,
                        ])
                    });
                    let patch = image::imageops::resize(
                        &patch,
                        16,
                        16,
                        image::imageops::FilterType::Triangle,
                    );
                    let feature = std::array::from_fn(|i| f32::from(patch.as_raw()[i]));
                    let Some(feature) = normalized(feature) else {
                        continue;
                    };
                    for template in &templates {
                        let coarse = (0..256)
                            .step_by(4)
                            .map(|i| feature[i] * template.feature[i])
                            .sum::<f32>()
                            * 4.0;
                        if coarse < 0.65 {
                            continue;
                        }
                        let score = feature
                            .iter()
                            .zip(template.feature)
                            .map(|(a, b)| a * b)
                            .sum::<f32>();
                        if score > 0.72 {
                            scores
                                .entry(&template.unit)
                                .and_modify(|v| *v = v.max(score))
                                .or_insert(score);
                        }
                    }
                }
            }
        }
        let mut ranked = scores.into_iter().collect::<Vec<_>>();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        if let Some((unit, score)) = ranked.first()
            && *score >= 0.80
            && *score - ranked.get(1).map_or(0.0, |v| v.1) >= 0.06
        {
            found
                .entry(unit.to_string())
                .and_modify(|v| *v = v.max(*score))
                .or_insert(*score);
        }
    }
    crate::diagnostics::debug("recording.portraits", &format!("matches={found:?}"));
    found.into_keys().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portrait_atlas_covers_distinct_units_and_ignores_flat_frames() {
        let lib = library();
        assert!(lib.iter().any(|p| p.unit == "char_2027_wang"));
        assert!(lib.iter().any(|p| p.unit == "char_1050_chen3"));
        assert!(deployment_bar(&vec![0; 960 * 540 * 4], 960, 540).is_empty());
    }
}
