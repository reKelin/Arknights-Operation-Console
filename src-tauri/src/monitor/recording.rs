use std::{
    fs::File,
    io::{ErrorKind, Read},
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use serde::Deserialize;

use super::{
    ClockTransition, MonitorEvent, ObservationClock, RecordingSegment, RecordingTracePoint,
    VisionConfig, analyze_bgra,
};

const OUTPUT_FPS: u64 = 30;

pub struct RecordingSession {
    cancelled: Arc<AtomicBool>,
}

impl RecordingSession {
    pub fn start(
        path: &str,
        config: VisionConfig,
        latest: Arc<Mutex<Option<MonitorEvent>>>,
    ) -> Result<(Self, String, u16), String> {
        let path = Path::new(path);
        validate_recording_path(path)?;
        let metadata = probe(path)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("游戏录屏")
            .to_string();
        let path = path.to_path_buf();
        let cancelled = Arc::new(AtomicBool::new(false));
        let task_cancelled = Arc::clone(&cancelled);
        thread::Builder::new()
            .name("recording-analysis".to_string())
            .spawn(move || {
                if let Err(message) =
                    analyze_file(&path, metadata, config, &task_cancelled, &latest)
                {
                    publish(&latest, MonitorEvent::Error(message));
                }
            })
            .map_err(|error| format!("启动录屏分析线程失败：{error}"))?;
        Ok((Self { cancelled }, name, config.frames_per_cost))
    }

    pub fn stop(self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

#[derive(Deserialize)]
struct ProbeOutput {
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}

#[derive(Deserialize)]
struct ProbeStream {
    width: u32,
    height: u32,
    avg_frame_rate: String,
}

#[derive(Deserialize)]
struct ProbeFormat {
    duration: String,
}

#[derive(Clone, Copy, Debug)]
struct RecordingMetadata {
    width: u32,
    height: u32,
    duration_seconds: f64,
}

fn validate_recording_path(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("所选录屏文件不存在".to_string());
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    if !extension.eq_ignore_ascii_case("mkv") && !extension.eq_ignore_ascii_case("mp4") {
        return Err("录屏格式必须是 MKV 或 MP4".to_string());
    }
    File::open(path).map_err(|error| format!("无法读取录屏文件：{error}"))?;
    Ok(())
}

fn probe(path: &Path) -> Result<RecordingMetadata, String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,avg_frame_rate:format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("未找到 ffprobe，无法分析录屏：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ffprobe 无法读取录屏：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_probe_output(&output.stdout)
}

fn parse_probe_output(bytes: &[u8]) -> Result<RecordingMetadata, String> {
    let probe: ProbeOutput =
        serde_json::from_slice(bytes).map_err(|error| format!("解析录屏信息失败：{error}"))?;
    let stream = probe
        .streams
        .first()
        .ok_or_else(|| "录屏中没有视频流".to_string())?;
    if stream.width < 640 || stream.height < 360 {
        return Err(format!(
            "录屏画面尺寸过小：{}×{}",
            stream.width, stream.height
        ));
    }
    let frame_rate = parse_rate(&stream.avg_frame_rate)?;
    if !(1.0..=240.0).contains(&frame_rate) {
        return Err(format!("录屏帧率无效：{frame_rate}"));
    }
    let duration_seconds = probe
        .format
        .duration
        .parse::<f64>()
        .map_err(|_| "录屏时长无效".to_string())?;
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return Err("录屏时长必须大于 0".to_string());
    }
    Ok(RecordingMetadata {
        width: stream.width,
        height: stream.height,
        duration_seconds,
    })
}

fn parse_rate(value: &str) -> Result<f64, String> {
    let (numerator, denominator) = value
        .split_once('/')
        .ok_or_else(|| format!("无法解析录屏帧率：{value}"))?;
    let numerator = numerator
        .parse::<f64>()
        .map_err(|_| format!("无法解析录屏帧率：{value}"))?;
    let denominator = denominator
        .parse::<f64>()
        .map_err(|_| format!("无法解析录屏帧率：{value}"))?;
    if denominator == 0.0 {
        return Err(format!("录屏帧率分母为 0：{value}"));
    }
    Ok(numerator / denominator)
}

fn analyze_file(
    path: &Path,
    metadata: RecordingMetadata,
    config: VisionConfig,
    cancelled: &AtomicBool,
    latest: &Mutex<Option<MonitorEvent>>,
) -> Result<(), String> {
    let frame_size = usize::try_from(metadata.width)
        .ok()
        .and_then(|width| {
            usize::try_from(metadata.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|size| *size <= 256 * 1024 * 1024)
        .ok_or_else(|| "录屏画面尺寸过大".to_string())?;
    let mut child = Command::new("ffmpeg")
        .args(["-loglevel", "error", "-i"])
        .arg(path)
        .args([
            "-vf", "fps=30", "-f", "rawvideo", "-pix_fmt", "bgra", "-an", "-sn", "-dn", "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("未找到 ffmpeg，无法解码录屏：{error}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ffmpeg 没有提供视频输出".to_string())?;
    let expected_frames = (metadata.duration_seconds * OUTPUT_FPS as f64)
        .ceil()
        .max(1.0) as u64;
    let mut buffer = vec![0_u8; frame_size];
    let mut source_frame = 0_u64;
    let mut clock = ObservationClock::default();
    let mut trace = Vec::new();
    let mut segments = Vec::new();
    let mut active_segment: Option<(u32, u32, u32)> = None;
    let mut last_progress = u8::MAX;

    loop {
        if cancelled.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        match stdout.read_exact(&mut buffer) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => break,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("读取 ffmpeg 解码帧失败：{error}"));
            }
        }
        let timestamp_ns = source_frame.saturating_mul(1_000_000_000) / OUTPUT_FPS;
        let observation = analyze_bgra(
            &buffer,
            metadata.width,
            metadata.height,
            metadata.width * 4,
            timestamp_ns,
            config,
        )?;
        let update = clock.observe(&observation);
        match update.transition {
            ClockTransition::Started => {
                let source = source_frame.min(u64::from(u32::MAX)) as u32;
                active_segment = Some((source, 0, source));
            }
            ClockTransition::Exited => {
                if let Some((start, duration, last_inside)) = active_segment.take() {
                    push_segment(&mut segments, start, last_inside, duration);
                }
            }
            _ => {
                if let Some((_, duration, last_inside)) = &mut active_segment {
                    *duration = (*duration).max(update.frame);
                    if observation.battle_state.is_in_battle() {
                        *last_inside = source_frame.min(u64::from(u32::MAX)) as u32;
                    }
                }
            }
        }
        let point = RecordingTracePoint {
            source_frame: source_frame.min(u64::from(u32::MAX)) as u32,
            game_frame: update.frame,
            battle_state: observation.battle_state,
            cost_phase: observation.cost_phase,
        };
        if trace.last().is_none_or(|previous: &RecordingTracePoint| {
            previous.game_frame != point.game_frame
                || previous.battle_state != point.battle_state
                || previous.cost_phase != point.cost_phase
        }) {
            trace.push(point);
        }
        let progress = ((source_frame.saturating_mul(100) / expected_frames).min(99)) as u8;
        if progress != last_progress {
            last_progress = progress;
            publish(
                latest,
                MonitorEvent::RecordingProgress {
                    progress,
                    observation,
                },
            );
        }
        source_frame = source_frame.saturating_add(1);
    }

    drop(stdout);
    let output = child
        .wait_with_output()
        .map_err(|error| format!("等待 ffmpeg 结束失败：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ffmpeg 解码失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if let Some((start, duration, last_inside)) = active_segment {
        push_segment(&mut segments, start, last_inside, duration);
    }
    let duration_frames = trace
        .iter()
        .map(|point| point.game_frame)
        .max()
        .unwrap_or(0);
    publish(
        latest,
        MonitorEvent::RecordingReady {
            trace,
            segments,
            duration_frames,
        },
    );
    Ok(())
}

fn push_segment(
    segments: &mut Vec<RecordingSegment>,
    start: u32,
    end: u32,
    game_duration_frames: u32,
) {
    segments.push(RecordingSegment {
        index: segments.len().min(u32::MAX as usize) as u32,
        source_start_frame: start,
        source_end_frame: end,
        game_duration_frames,
    });
}

fn publish(latest: &Mutex<Option<MonitorEvent>>, event: MonitorEvent) {
    if let Ok(mut latest) = latest.lock() {
        *latest = Some(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fractional_frame_rate() {
        assert!((parse_rate("60000/1001").unwrap() - 59.940_059).abs() < 0.000_1);
    }

    #[test]
    fn rejects_zero_frame_rate_denominator() {
        assert!(parse_rate("60/0").is_err());
    }

    #[test]
    fn parses_ffprobe_metadata() {
        let metadata = parse_probe_output(
            br#"{
                "streams": [{
                    "width": 1920,
                    "height": 1080,
                    "avg_frame_rate": "60000/1001"
                }],
                "format": { "duration": "153.749" }
            }"#,
        )
        .unwrap();

        assert_eq!(metadata.width, 1920);
        assert_eq!(metadata.height, 1080);
        assert_eq!(metadata.duration_seconds, 153.749);
    }

    #[test]
    fn rejects_missing_video_stream() {
        let error = parse_probe_output(br#"{"streams":[],"format":{"duration":"1"}}"#).unwrap_err();
        assert_eq!(error, "录屏中没有视频流");
    }

    #[test]
    fn appends_indexed_recording_segment() {
        let mut segments = Vec::new();
        push_segment(&mut segments, 30, 300, 450);
        push_segment(&mut segments, 600, 900, 300);

        assert_eq!(segments[1].index, 1);
        assert_eq!(segments[1].source_start_frame, 600);
        assert_eq!(segments[1].game_duration_frames, 300);
    }
}
