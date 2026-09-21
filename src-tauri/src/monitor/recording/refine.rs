use super::*;

// 只补读交互结束附近的原始帧，无操作区段不进入精查。
pub(super) fn responses(
    path: &Path,
    metadata: &RecordingMetadata,
    candidates: &mut [analysis::AnalysisCandidate],
    trace: &mut Vec<RecordingTracePoint>,
    cancelled: &AtomicBool,
) {
    let scale = (metadata.width as f64 / 1920.0).min(metadata.height as f64 / 1080.0);
    let crop_width = (450.0 * scale) as usize;
    let crop_height = (20.0 * scale) as usize;
    let crop_x = ((metadata.width as f64 - 1920.0 * scale) / 2.0) as usize;
    let crop_y = ((metadata.height as f64 - 1080.0 * scale) / 2.0 + 490.0 * scale) as usize;
    for candidate in candidates.iter_mut().filter(|c| c.kind.is_some()) {
        if cancelled.load(Ordering::Acquire) {
            return;
        }
        let Ok(end) = candidate.source_end.nanoseconds() else {
            continue;
        };
        let start = end.saturating_sub(180_000_000);
        let result = video_command("ffmpeg")
            .args([
                "-hide_banner",
                "-nostats",
                "-loglevel",
                "info",
                "-copyts",
                "-threads",
                "2",
                "-ss",
                &format!("{:.9}", start as f64 / 1e9),
                "-i",
            ])
            .arg(path)
            .args([
                "-to",
                &format!("{:.9}", (end + 300_000_000) as f64 / 1e9),
                "-map",
                "0:v:0",
                "-vf",
                &format!(
                    "trim=start={:.9}:end={:.9},scale={}:{},format=bgra,crop={crop_width}:{crop_height}:{crop_x}:{crop_y}:exact=1,showinfo=checksum=0",
                    start as f64 / 1e9,
                    (end + 60_000_000) as f64 / 1e9,
                    metadata.width,
                    metadata.height
                ),
                "-fps_mode",
                "passthrough",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "bgra",
                "-an",
                "-sn",
                "-dn",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .output();
        let Ok(output) = result else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let timestamps = String::from_utf8_lossy(&output.stderr)
            .lines()
            .filter_map(|line| {
                parse_frame_timestamp(line, metadata.time_base)
                    .ok()
                    .flatten()
            })
            .collect::<Vec<_>>();
        let size = crop_width * crop_height * 4;
        if output.stdout.len() != timestamps.len() * size {
            continue;
        }
        let mut last_panel = None;
        let mut boundary = None;
        let mut clear = 0;
        for (frame, timestamp) in output.stdout.chunks_exact(size).zip(timestamps) {
            if cropped_panel_visible(frame, crop_width, crop_height) {
                last_panel = Some(timestamp);
                boundary = None;
                clear = 0;
            } else if let Some(before) = &last_panel {
                clear += 1;
                if boundary.is_none() {
                    boundary = Some((before.clone(), timestamp));
                }
                if clear >= 2 {
                    break;
                }
            }
        }
        let Some((before, after)) = boundary.filter(|_| clear >= 2) else {
            continue;
        };
        let (Ok(before_ns), Ok(after_ns)) = (before.nanoseconds(), after.nanoseconds()) else {
            continue;
        };
        if after_ns.abs_diff(end) > 150_000_000 {
            continue;
        }
        let Some(base) = trace
            .iter()
            .rev()
            .find(|p| p.source_timestamp_ns <= after_ns as f64)
            .cloned()
        else {
            continue;
        };
        let Some(next) = trace
            .iter()
            .find(|p| p.source_timestamp_ns >= after_ns as f64)
        else {
            continue;
        };
        let ratio = if next.source_timestamp_ns > base.source_timestamp_ns {
            (after_ns as f64 - base.source_timestamp_ns)
                / (next.source_timestamp_ns - base.source_timestamp_ns)
        } else {
            0.0
        };
        let frame = base.game_frame
            + ((next.game_frame.saturating_sub(base.game_frame)) as f64 * ratio).round() as u32;
        // 插值只改估计落点，不抹掉累计时钟不确定性。
        candidate.game_frame_range.start = base.game_frame_min.min(frame);
        candidate.game_frame_range.end = next.game_frame_max.max(frame);
        candidate.source_end = after;
        trace.push(RecordingTracePoint {
            source_timestamp_ns: after_ns as f64,
            game_frame: frame,
            game_frame_min: candidate.game_frame_range.start,
            game_frame_max: candidate.game_frame_range.end,
            ..base
        });
        trace.sort_by(|a, b| a.source_timestamp_ns.total_cmp(&b.source_timestamp_ns));
        crate::diagnostics::debug(
            "recording.refine",
            &format!(
                "candidate={} response_ns={before_ns}..{after_ns} game_frame={frame} mapping={}..{}",
                candidate.id, candidate.game_frame_range.start, candidate.game_frame_range.end
            ),
        );
    }
}

fn cropped_panel_visible(data: &[u8], width: usize, height: usize) -> bool {
    let mut cyan = 0;
    for x in (0..450).step_by(4) {
        for y in (0..20).step_by(4) {
            let i = ((y * height / 20) * width + x * width / 450) * 4;
            cyan += usize::from(data[i] > 150 && data[i + 1] > 90 && data[i + 2] < 70);
        }
    }
    cyan > 15
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cropped_response_uses_the_same_panel_evidence() {
        let mut full = vec![0; 960 * 540 * 4];
        let mut crop = vec![0; 225 * 10 * 4];
        for x in 0..40 {
            for y in 0..10 {
                let i = (y * 225 + x) * 4;
                crop[i..i + 4].copy_from_slice(&[200, 150, 20, 255]);
                let j = ((245 + y) * 960 + x) * 4;
                full[j..j + 4].copy_from_slice(&crop[i..i + 4]);
            }
        }
        assert!(cropped_panel_visible(&crop, 225, 10));
        assert_eq!(
            panel_visible(&full, 960, 540),
            cropped_panel_visible(&crop, 225, 10)
        );
        assert!(!cropped_panel_visible(&vec![0; 225 * 10 * 4], 225, 10));
    }
}
