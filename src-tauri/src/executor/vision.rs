use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use image::{GrayImage, ImageBuffer, Luma};

use super::resources::resize_gray;

#[derive(Clone)]
pub struct CapturedFrame {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub struct ExecutionVision {
    enabled: AtomicBool,
    latest: Mutex<Option<CapturedFrame>>,
}

impl ExecutionVision {
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            latest: Mutex::new(None),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
        if !enabled && let Ok(mut latest) = self.latest.lock() {
            *latest = None;
        }
    }

    pub fn publish(&self, data: &[u8], width: u32, height: u32, row_pitch: u32) {
        if !self.enabled.load(Ordering::Acquire) {
            return;
        }
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let start = (y * row_pitch) as usize;
            let end = start + (width * 4) as usize;
            let Some(row) = data.get(start..end) else {
                return;
            };
            pixels.extend_from_slice(row);
        }
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(CapturedFrame {
                pixels,
                width,
                height,
            });
        }
    }

    pub fn latest(&self) -> Option<CapturedFrame> {
        self.latest.lock().ok()?.clone()
    }
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
}

pub fn locate_operator(
    frame: &CapturedFrame,
    templates: &[GrayImage],
) -> Result<(i32, i32), String> {
    let slots = detect_slots(frame);
    if slots.is_empty() {
        return Err("未识别到部署栏卡位".to_string());
    }
    let mut scores = Vec::new();
    for slot in slots {
        let avatar = crop_gray(frame, slot)?;
        let best = templates
            .iter()
            .map(|template| {
                normalized_correlation(&avatar, &resize_gray(template, slot.width, slot.height))
            })
            .fold(f64::NEG_INFINITY, f64::max);
        scores.push((best, slot));
    }
    scores.sort_by(|left, right| right.0.total_cmp(&left.0));
    let Some((best, slot)) = scores.first().copied() else {
        return Err("部署栏头像匹配失败".to_string());
    };
    let second = scores.get(1).map(|value| value.0).unwrap_or(-1.0);
    if best < 0.72 || best - second < 0.06 {
        return Err(format!(
            "部署栏头像匹配不唯一（最高 {:.2}，次高 {:.2}）",
            best, second
        ));
    }
    Ok((
        (slot.left + slot.width / 2) as i32,
        (slot.top + slot.height / 2) as i32,
    ))
}

fn detect_slots(frame: &CapturedFrame) -> Vec<Rect> {
    let scale = (frame.width as f64 / 1280.0).min(frame.height as f64 / 720.0);
    let offset_x = (frame.width as f64 - 1280.0 * scale) / 2.0;
    let offset_y = (frame.height as f64 - 720.0 * scale) / 2.0;
    let sx = |value: f64| (offset_x + value * scale).round().max(0.0) as u32;
    let sy = |value: f64| (offset_y + value * scale).round().max(0.0) as u32;
    let left = sx(20.0);
    let right = sx(1278.0).min(frame.width);
    let top = sy(590.0);
    let bottom = sy(625.0).min(frame.height);
    let mut active = Vec::new();
    for x in left..right {
        let bright = (top..bottom)
            .filter(|y| pixel_luma(frame, x, *y).is_some_and(|value| value >= 180))
            .count();
        active.push((x, bright >= 3));
    }
    let mut ranges = Vec::new();
    let mut start = None;
    for (x, on) in active {
        match (start, on) {
            (None, true) => start = Some(x),
            (Some(from), false) => {
                if x.saturating_sub(from) >= 2 {
                    ranges.push((from, x - 1));
                }
                start = None;
            }
            _ => {}
        }
    }
    ranges
        .into_iter()
        .filter_map(|(from, to)| {
            let flag_width = to.saturating_sub(from) + 1;
            let avatar_left = from.saturating_sub((39.0 * scale).round() as u32);
            let avatar_top = top.saturating_add((35.0 * scale).round() as u32);
            let width = flag_width.saturating_add((53.0 * scale).round() as u32);
            let height = (54.0 * scale).round().max(8.0) as u32;
            (avatar_left + width <= frame.width && avatar_top + height <= frame.height).then_some(
                Rect {
                    left: avatar_left,
                    top: avatar_top,
                    width,
                    height,
                },
            )
        })
        .collect()
}

fn crop_gray(frame: &CapturedFrame, rect: Rect) -> Result<GrayImage, String> {
    let mut image = ImageBuffer::<Luma<u8>, Vec<u8>>::new(rect.width, rect.height);
    for y in 0..rect.height {
        for x in 0..rect.width {
            let value = pixel_luma(frame, rect.left + x, rect.top + y)
                .ok_or_else(|| "部署栏头像区域超出画面".to_string())?;
            image.put_pixel(x, y, Luma([value]));
        }
    }
    Ok(image)
}

fn pixel_luma(frame: &CapturedFrame, x: u32, y: u32) -> Option<u8> {
    let offset = ((y * frame.width + x) * 4) as usize;
    let pixel = frame.pixels.get(offset..offset + 3)?;
    Some(
        ((u16::from(pixel[2]) * 77 + u16::from(pixel[1]) * 150 + u16::from(pixel[0]) * 29) >> 8)
            as u8,
    )
}

fn normalized_correlation(left: &GrayImage, right: &GrayImage) -> f64 {
    if left.dimensions() != right.dimensions() || left.is_empty() {
        return -1.0;
    }
    let count = f64::from(left.width() * left.height());
    let left_mean = left.pixels().map(|pixel| f64::from(pixel[0])).sum::<f64>() / count;
    let right_mean = right.pixels().map(|pixel| f64::from(pixel[0])).sum::<f64>() / count;
    let mut numerator = 0.0;
    let mut left_power = 0.0;
    let mut right_power = 0.0;
    for (a, b) in left.pixels().zip(right.pixels()) {
        let a = f64::from(a[0]) - left_mean;
        let b = f64::from(b[0]) - right_mean;
        numerator += a * b;
        left_power += a * a;
        right_power += b * b;
    }
    let denominator = (left_power * right_power).sqrt();
    if denominator <= f64::EPSILON {
        -1.0
    } else {
        numerator / denominator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_images_have_perfect_correlation() {
        let image = ImageBuffer::from_fn(8, 8, |x, y| Luma([((x * 17 + y * 9) % 255) as u8]));
        assert!((normalized_correlation(&image, &image) - 1.0).abs() < 0.000_1);
    }
}
