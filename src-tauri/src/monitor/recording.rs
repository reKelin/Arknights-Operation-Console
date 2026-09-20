use std::{
    fs::File,
    io::{BufRead, BufReader, ErrorKind, Read},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crate::stage::{StageCatalog, StageMatchStatus, StageRecognition};
use serde::Deserialize;

use super::{
    ClockTransition, MonitorEvent, MonitorEventQueue, ObservationClock, RecordingSegment,
    RecordingTracePoint, VisionConfig, analyze_bgra,
    ocr::{OcrImage, StageOcrAccumulator, StageOcrRecognizer, crop_region, crop_title},
};

pub mod analysis;

use analysis::{
    CandidateObservation, GameFrameRange, SourceTimeBase, SourceTimestamp,
    extract_operation_candidates,
};

const MIN_GAP_THRESHOLD_NS: u64 = 250_000_000;

pub struct RecordingSession {
    cancelled: Arc<AtomicBool>,
}

impl RecordingSession {
    pub fn start(
        path: &str,
        config: VisionConfig,
        catalog: Arc<StageCatalog>,
        events: Arc<Mutex<MonitorEventQueue>>,
    ) -> Result<(Self, String, u16), String> {
        let path = Path::new(path);
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
                let started = Instant::now();
                crate::diagnostics::info("recording", "开始读取视频信息和原始 PTS");
                let result = validate_recording_path(&path)
                    .and_then(|_| probe(&path, &task_cancelled))
                    .and_then(|metadata| {
                        crate::diagnostics::info(
                            "recording",
                            &format!(
                                "probe_ms={} width={} height={} sample_stride={}",
                                started.elapsed().as_millis(),
                                metadata.width,
                                metadata.height,
                                metadata.stride
                            ),
                        );
                        if task_cancelled.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        analyze_file(&path, metadata, config, catalog, &task_cancelled, &events)
                    });
                if let Err(message) = result
                    && !task_cancelled.load(Ordering::Acquire)
                {
                    let safe_message = message.replace(path.to_string_lossy().as_ref(), "[视频]");
                    crate::diagnostics::error("recording", &safe_message);
                    publish(&events, MonitorEvent::Error(message));
                }
                crate::diagnostics::info(
                    "recording",
                    &format!(
                        "total_ms={} cancelled={}",
                        started.elapsed().as_millis(),
                        task_cancelled.load(Ordering::Acquire)
                    ),
                );
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
    time_base: String,
}

#[derive(Deserialize)]
struct ProbeFormat {
    duration: String,
}

#[derive(Clone, Debug)]
struct RecordingMetadata {
    width: u32,
    height: u32,
    gap_threshold_ns: u64,
    time_base: SourceTimeBase,
    duration_ns: u64,
    stride: usize,
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

// 管道必须并行读取，避免子进程错误输出填满后解码互相等待。
fn drain_pipe(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<Result<Vec<u8>, String>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        Ok(bytes)
    })
}
struct VideoProcess(Child);
impl Drop for VideoProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn video_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn probe(path: &Path, cancelled: &AtomicBool) -> Result<RecordingMetadata, String> {
    let mut child = video_command("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,avg_frame_rate,time_base:format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("未找到 ffprobe，无法分析录屏：{error}"))?;
    let stdout = drain_pipe(child.stdout.take().unwrap());
    let stderr = drain_pipe(child.stderr.take().unwrap());
    let mut child = VideoProcess(child);
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("录屏分析已取消".to_string());
        }
        if child
            .0
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let status = child.0.wait().map_err(|error| error.to_string())?;
    let stdout = stdout.join().map_err(|_| "读取视频信息失败")??;
    let stderr = stderr.join().map_err(|_| "读取视频错误信息失败")??;
    if !status.success() {
        return Err(format!(
            "ffprobe 无法读取录屏：{}",
            String::from_utf8_lossy(&stderr).trim()
        ));
    }
    parse_probe_output(&stdout)
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
    let time_base = SourceTimeBase::parse(&stream.time_base).map_err(|_| "录屏时间基无效")?;
    let stride = (frame_rate / 15.0).round().max(1.0) as usize;
    let duration_seconds = probe
        .format
        .duration
        .parse::<f64>()
        .map_err(|_| "录屏时长无效".to_string())?;
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return Err("录屏时长必须大于 0".to_string());
    }
    let expected_frame_interval_ns = (1_000_000_000.0 / frame_rate).ceil() as u64;
    Ok(RecordingMetadata {
        width: stream.width,
        height: stream.height,
        gap_threshold_ns: MIN_GAP_THRESHOLD_NS.max(
            expected_frame_interval_ns
                .saturating_mul(stride as u64)
                .saturating_mul(4),
        ),
        time_base,
        duration_ns: (duration_seconds * 1_000_000_000.0) as u64,
        stride,
    })
}

fn parse_frame_timestamp(
    line: &str,
    time_base: SourceTimeBase,
) -> Result<Option<SourceTimestamp>, String> {
    if let Some(value) = line.split("config in time_base: ").nth(1) {
        let actual = value.split(',').next().unwrap_or_default();
        if SourceTimeBase::parse(actual).ok() != Some(time_base) {
            return Err("解码时间基与视频流不一致".into());
        }
    }
    if !line.contains(" n:") {
        return Ok(None);
    }
    let pts = line
        .split(" pts:")
        .nth(1)
        .and_then(|value| value.split_whitespace().next())
        .ok_or("解码帧缺少 PTS")?;
    SourceTimestamp::parse(pts, time_base)
        .map(Some)
        .map_err(|_| "解码 PTS 无效".into())
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
    catalog: Arc<StageCatalog>,
    cancelled: &AtomicBool,
    events: &Mutex<MonitorEventQueue>,
) -> Result<(), String> {
    let started = Instant::now();
    let config = VisionConfig {
        recording_analysis: true,
        ..config
    };
    let mut metadata = metadata;
    if metadata.width > 960 && metadata.height > 540 {
        let scale = (960.0 / f64::from(metadata.width)).max(540.0 / f64::from(metadata.height));
        metadata.width = (f64::from(metadata.width) * scale).round() as u32;
        metadata.height = (f64::from(metadata.height) * scale).round() as u32;
    }
    let mut state_counts = std::collections::BTreeMap::<String, usize>::new();
    let mut trusted_frames = 0usize;
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
    let mut child = video_command("ffmpeg")
        .args([
            "-hide_banner",
            "-nostats",
            "-loglevel",
            "info",
            "-copyts",
            "-i",
        ])
        .arg(path)
        .args([
            "-vf",
            &format!(
                r"select=not(mod(n\,{stride})),scale={}:{},showinfo=checksum=0",
                metadata.width,
                metadata.height,
                stride = metadata.stride
            ),
        ])
        .args([
            "-map",
            "0:v:0",
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
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("未找到 ffmpeg，无法解码录屏：{error}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ffmpeg 没有提供视频输出".to_string())?;
    let (timestamp_tx, timestamp_rx) = std::sync::mpsc::channel();
    let pipe = child.stderr.take().unwrap();
    let time_base = metadata.time_base;
    let stderr = thread::spawn(move || -> Result<String, String> {
        let mut tail = std::collections::VecDeque::new();
        for line in BufReader::new(pipe).lines() {
            let line = line.map_err(|error| error.to_string())?;
            if line.contains("Parsed_showinfo") {
                if let Some(timestamp) = parse_frame_timestamp(&line, time_base)?
                    && timestamp_tx.send(timestamp).is_err()
                {
                    break;
                }
            } else {
                if tail.len() == 12 {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
        }
        Ok(tail.into_iter().collect::<Vec<_>>().join("\n"))
    });
    let mut child = VideoProcess(child);
    let mut buffer = vec![0_u8; frame_size];
    let mut source_frame = 0_usize;
    let mut previous_timestamp_ns = None;
    let mut clock = ObservationClock::default();
    let mut trace = Vec::new();
    let mut segments = Vec::new();
    let mut candidate_observations = Vec::new();
    let mut ocr_accumulator = StageOcrAccumulator::new(Arc::clone(&catalog));
    let recognizer = StageOcrRecognizer::new(catalog);
    let mut stage_recognition = match &recognizer {
        Ok(_) => StageRecognition::default(),
        Err(warning) => StageRecognition {
            warning: Some(warning.clone()),
            ..StageRecognition::default()
        },
    };
    let mut active_segment: Option<(u32, u32, u32, u32, StageRecognition)> = None;
    let mut last_progress = u8::MAX;
    let mut last_cost_image = None;
    let mut pending: Option<PendingInteraction> = None;
    let mut completions = Vec::new();
    let operators: OperatorNames = serde_json::from_str(include_str!("../../data/operators.json"))
        .map_err(|error| error.to_string())?;

    loop {
        if cancelled.load(Ordering::Acquire) {
            let _ = child.0.kill();
            let _ = child.0.wait();
            return Ok(());
        }
        match stdout.read_exact(&mut buffer) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => break,
            Err(error) => {
                let _ = child.0.kill();
                let _ = child.0.wait();
                return Err(format!("读取 ffmpeg 解码帧失败：{error}"));
            }
        }
        let source_timestamp = timestamp_rx
            .recv()
            .map_err(|_| "解码帧缺少对应的原始 PTS")?;
        let timestamp_ns = source_timestamp
            .nanoseconds()
            .map_err(|_| format!("录屏第 {source_frame} 帧的原始展示时间戳无法换算"))?;
        if previous_timestamp_ns.is_some_and(|previous| timestamp_ns <= previous) {
            return Err("录屏原始 PTS 倒退或重复".into());
        }
        let discontinuity_before = previous_timestamp_ns.is_some_and(|previous| {
            timestamp_ns.saturating_sub(previous) > metadata.gap_threshold_ns
        });
        if discontinuity_before {
            clock.mark_observation_gap(1);
        }
        previous_timestamp_ns = Some(timestamp_ns);
        let mut observation = analyze_bgra(
            &buffer,
            metadata.width,
            metadata.height,
            metadata.width * 4,
            timestamp_ns,
            config,
        )?;
        if observation.title_candidate
            && stage_recognition.status != StageMatchStatus::Matched
            && source_frame.is_multiple_of(30)
            && let (Ok(recognizer), Ok(image)) = (
                recognizer.as_ref(),
                crop_title(&buffer, metadata.width, metadata.height, metadata.width * 4),
            )
        {
            let candidate = recognizer
                .recognize_text(image)
                .map(|text| ocr_accumulator.push(&text))
                .unwrap_or_else(|warning| StageRecognition {
                    warning: Some(warning),
                    ..StageRecognition::default()
                });
            if recognition_rank(candidate.status) >= recognition_rank(stage_recognition.status) {
                stage_recognition = candidate;
            }
        }
        if stage_recognition.status != StageMatchStatus::Unavailable
            || stage_recognition.warning.is_some()
        {
            observation.stage_recognition = Some(stage_recognition.clone());
        }
        *state_counts
            .entry(format!("{:?}", observation.battle_state))
            .or_default() += 1;
        observe_interaction(
            &buffer,
            metadata.width,
            metadata.height,
            &observation,
            recognizer.as_ref().ok(),
            &mut last_cost_image,
            &mut pending,
            &mut completions,
            &operators,
        );
        // 离线视频 UI 缩放尚未校准，费用条不能冒充有效时间锚点。
        observation.cost_phase = None;
        let update = clock.observe(&observation);
        trusted_frames += usize::from(update.quality == super::ClockQuality::Trusted);
        match update.transition {
            ClockTransition::Started => {
                let source = source_frame.min(u32::MAX as usize) as u32;
                let index = segments.len().min(u32::MAX as usize) as u32;
                active_segment = Some((index, source, 0, source, stage_recognition.clone()));
            }
            ClockTransition::Exited => {
                if let Some((_, start, duration, last_inside, recognition)) = active_segment.take()
                {
                    push_segment(&mut segments, start, last_inside, duration, recognition);
                }
                stage_recognition = StageRecognition::default();
                ocr_accumulator.reset();
            }
            _ => {
                if let Some((_, _, duration, last_inside, recognition)) = &mut active_segment {
                    *duration = (*duration).max(update.frame);
                    if recognition_rank(stage_recognition.status)
                        >= recognition_rank(recognition.status)
                    {
                        *recognition = stage_recognition.clone();
                    }
                    if observation.battle_state.is_in_battle() {
                        *last_inside = source_frame.min(u32::MAX as usize) as u32;
                    }
                }
            }
        }
        if let Some((segment_index, ..)) = active_segment.as_ref() {
            candidate_observations.push(CandidateObservation {
                source_timestamp,
                segment_index: *segment_index,
                game_frame_range: GameFrameRange {
                    start: update.frame.saturating_sub(update.uncertainty_frames),
                    end: update.frame.saturating_add(update.uncertainty_frames),
                },
                battle_state: observation.battle_state,
                observation_confidence: observation.confidence,
                mapping_trusted: update.quality == super::ClockQuality::Trusted,
                discontinuity_before,
            });
        }
        let point = RecordingTracePoint {
            source_frame: source_frame.min(u32::MAX as usize) as u32,
            source_timestamp_ns: timestamp_ns as f64,
            game_frame: update.frame,
            game_frame_min: update.frame.saturating_sub(update.uncertainty_frames),
            game_frame_max: update.frame.saturating_add(update.uncertainty_frames),
            clock_quality: update.quality,
            battle_state: observation.battle_state,
            cost_phase: observation.cost_phase,
        };
        trace.push(point);
        let progress = ((timestamp_ns.saturating_mul(100) / metadata.duration_ns).min(99)) as u8;
        if progress != last_progress {
            last_progress = progress;
            crate::diagnostics::debug(
                "recording",
                &format!(
                    "progress={progress} frame={source_frame} elapsed_ms={} state={:?} clock={:?} trusted_frames={trusted_frames}",
                    started.elapsed().as_millis(),
                    observation.battle_state,
                    update.quality
                ),
            );
            publish(
                events,
                MonitorEvent::RecordingProgress {
                    progress,
                    observation,
                },
            );
        }
        source_frame = source_frame.saturating_add(metadata.stride);
    }

    drop(stdout);
    let status = child
        .0
        .wait()
        .map_err(|error| format!("等待 ffmpeg 结束失败：{error}"))?;
    let stderr = stderr.join().map_err(|_| "读取解码错误信息失败")??;
    if !status.success() {
        return Err(format!("ffmpeg 解码失败：{}", stderr.trim()));
    }
    if source_frame == 0 || timestamp_rx.try_recv().is_ok() {
        return Err("解码画面与来源时间戳数量不一致".into());
    }
    if let Some((_, start, duration, last_inside, recognition)) = active_segment {
        push_segment(&mut segments, start, last_inside, duration, recognition);
    }
    let mut candidates = extract_operation_candidates(&candidate_observations);
    for candidate in &mut candidates {
        // 手势本身只证明出现部署预览；没有完成证据时仍需人工校对。
        if candidate.kind == Some(analysis::CandidateActionKind::Deploy) {
            candidate.kind = None;
            candidate
                .unconfirmed_fields
                .push(analysis::UnconfirmedField::ActionKind);
        }
        let end = candidate.source_end.nanoseconds().unwrap_or_default();
        let Some(completion) = completions
            .iter()
            .find(|event| end >= event.start_ns && end <= event.end_ns)
        else {
            continue;
        };
        let deployed = (completion.deployment && completion.cooldown_started)
            || completion
                .before
                .slots
                .zip(completion.after.slots)
                .is_some_and(|(before, after)| before == after + 1)
            || completion
                .before
                .cost
                .zip(completion.after.cost)
                .is_some_and(|(before, after)| after < before);
        if deployed {
            candidate.kind = Some(analysis::CandidateActionKind::Deploy);
            candidate.confidence = 85;
            candidate
                .unconfirmed_fields
                .retain(|field| *field != analysis::UnconfirmedField::ActionKind);
            if let Some(operator) = &completion.operator {
                candidate.operator = Some(operator.clone());
                candidate
                    .unconfirmed_fields
                    .retain(|field| *field != analysis::UnconfirmedField::Operator);
            } else if !candidate
                .unconfirmed_fields
                .contains(&analysis::UnconfirmedField::Operator)
            {
                candidate
                    .unconfirmed_fields
                    .push(analysis::UnconfirmedField::Operator);
            }
            if !candidate
                .unconfirmed_fields
                .contains(&analysis::UnconfirmedField::Direction)
            {
                candidate
                    .unconfirmed_fields
                    .push(analysis::UnconfirmedField::Direction);
            }
        } else if !completion.deployment
            && completion
                .before
                .slots
                .zip(completion.after.slots)
                .is_some_and(|(before, after)| after == before + 1)
        {
            candidate.kind = Some(analysis::CandidateActionKind::Retreat);
            candidate.confidence = 80;
            candidate
                .unconfirmed_fields
                .retain(|field| *field != analysis::UnconfirmedField::ActionKind);
        }
    }
    crate::diagnostics::info(
        "recording",
        &format!(
            "analysis_ms={} sampled_frames={} trusted_frames={trusted_frames} segments={} candidates={} states={state_counts:?}",
            started.elapsed().as_millis(),
            source_frame / metadata.stride,
            segments.len(),
            candidates.len()
        ),
    );
    let duration_frames = trace
        .iter()
        .map(|point| point.game_frame)
        .max()
        .unwrap_or(0);
    publish(
        events,
        MonitorEvent::RecordingReady {
            trace,
            segments,
            candidates,
            duration_frames,
        },
    );
    Ok(())
}

#[derive(Deserialize)]
struct OperatorNames {
    operators: Vec<OperatorName>,
}
#[derive(Deserialize)]
struct OperatorName {
    id: String,
    name: String,
}
struct PendingInteraction {
    start_ns: u64,
    before: HudReading,
    before_cooldown: f64,
    deployment: bool,
    operator: Option<String>,
    name_read: bool,
    running_frames: u8,
}
struct CompletedInteraction {
    start_ns: u64,
    end_ns: u64,
    before: HudReading,
    after: HudReading,
    cooldown_started: bool,
    deployment: bool,
    operator: Option<String>,
}
#[derive(Clone, Copy, Debug, Default)]
struct HudReading {
    cost: Option<u8>,
    slots: Option<u8>,
}
fn read_cost(recognizer: Option<&StageOcrRecognizer>, image: Option<OcrImage>) -> HudReading {
    let Some(text) = recognizer
        .zip(image)
        .and_then(|(recognizer, image)| recognizer.recognize_text(image).ok())
    else {
        return HudReading::default();
    };
    parse_hud(&text)
}
fn parse_hud(text: &str) -> HudReading {
    let normalized = text.replace(char::is_whitespace, "");
    let (cost_text, slots_text) = normalized.split_once('剩').unwrap_or((&normalized, ""));
    let number = |value: &str| {
        value
            .split(|ch: char| !ch.is_ascii_digit())
            .rfind(|part| !part.is_empty())
            .and_then(|part| part.parse::<u8>().ok())
    };
    // OCR 常把费用图标识别成 0，保留原始分词，只取图标后的最后一组数字。
    let raw_cost = text.split('剩').next().unwrap_or_default();
    HudReading {
        cost: if cost_text.is_empty() {
            None
        } else {
            number(raw_cost).filter(|value| *value <= 99)
        },
        slots: number(slots_text).filter(|value| *value <= 12),
    }
}
#[allow(clippy::too_many_arguments)]
fn observe_interaction(
    data: &[u8],
    width: u32,
    height: u32,
    observation: &super::VisualObservation,
    recognizer: Option<&StageOcrRecognizer>,
    last_cost: &mut Option<(OcrImage, f64)>,
    pending: &mut Option<PendingInteraction>,
    completions: &mut Vec<CompletedInteraction>,
    operators: &OperatorNames,
) {
    use super::ObservedBattleState::*;
    let timestamp = observation.capture_timestamp_ns;
    let running = matches!(observation.battle_state, OneXRunning | TwoXRunning);
    let interacting = matches!(
        observation.battle_state,
        DeployingOperator | AdjustingOperatorFacing | PointTwoXRunning
    );
    if interacting && pending.is_none() {
        let previous = last_cost.take();
        let before_cooldown = previous.as_ref().map_or(0.0, |(_, cooldown)| *cooldown);
        *pending = Some(PendingInteraction {
            start_ns: timestamp,
            before: read_cost(recognizer, previous.map(|(image, _)| image)),
            before_cooldown,
            deployment: false,
            operator: None,
            name_read: false,
            running_frames: 0,
        });
    }
    if let Some(event) = pending.as_mut() {
        event.deployment |= observation.battle_state == DeployingOperator;
        if interacting
            && !event.name_read
            && timestamp.saturating_sub(event.start_ns) >= 200_000_000
        {
            event.name_read = true;
            if let Some(recognizer) = recognizer
                && let Ok(image) = crop_region(data, width, height, width * 4, [0, 180, 450, 610])
                && let Ok(text) = recognizer.recognize_text(image)
            {
                let normalized = text.replace(char::is_whitespace, "");
                let matched = operators
                    .operators
                    .iter()
                    .filter(|operator| normalized.contains(&operator.name))
                    .collect::<Vec<_>>();
                if matched.len() == 1 {
                    event.operator = Some(matched[0].id.clone());
                }
            }
        }
        event.running_frames = if running {
            event.running_frames.saturating_add(1)
        } else {
            0
        };
        if event.running_frames >= 3 {
            let event = pending.take().unwrap();
            let after = read_cost(
                recognizer,
                crop_region(data, width, height, width * 4, [1640, 745, 1919, 900]).ok(),
            );
            crate::diagnostics::debug(
                "recording",
                &format!(
                    "interaction_start_ns={} end_ns={timestamp} cost_before={:?} cost_after={after:?} deployment={} operator={:?}",
                    event.start_ns, event.before, event.deployment, event.operator
                ),
            );
            completions.push(CompletedInteraction {
                start_ns: event.start_ns,
                end_ns: timestamp,
                before: event.before,
                after,
                cooldown_started: cooldown_ratio(data, width, height) - event.before_cooldown
                    > 0.01,
                deployment: event.deployment,
                operator: event.operator,
            });
        }
    }
    if running && pending.is_none() {
        *last_cost = crop_region(data, width, height, width * 4, [1640, 745, 1919, 900])
            .ok()
            .map(|image| (image, cooldown_ratio(data, width, height)));
    }
    if matches!(observation.battle_state, NotInBattle | BattleBegin) {
        *pending = None;
        *last_cost = None;
    }
}

fn cooldown_ratio(data: &[u8], width: u32, height: u32) -> f64 {
    let scale = (f64::from(width) / 1920.0).min(f64::from(height) / 1080.0);
    let offset_x = (f64::from(width) - 1920.0 * scale) / 2.0;
    let offset_y = (f64::from(height) - 1080.0 * scale) / 2.0;
    let mut red = 0;
    let mut total = 0;
    for y in (900..1070).step_by(4) {
        for x in (300..1910).step_by(4) {
            let x = (offset_x + f64::from(x) * scale) as usize;
            let y = (offset_y + f64::from(y) * scale) as usize;
            let offset = (y * width as usize + x) * 4;
            if let Some(pixel) = data.get(offset..offset + 4) {
                red += usize::from(
                    pixel[2] > 60
                        && f64::from(pixel[2]) > f64::from(pixel[1]) * 1.5
                        && f64::from(pixel[2]) > f64::from(pixel[0]) * 1.4,
                );
                total += 1;
            }
        }
    }
    red as f64 / total.max(1) as f64
}

fn push_segment(
    segments: &mut Vec<RecordingSegment>,
    start: u32,
    end: u32,
    game_duration_frames: u32,
    stage_recognition: StageRecognition,
) {
    segments.push(RecordingSegment {
        index: segments.len().min(u32::MAX as usize) as u32,
        source_start_frame: start,
        source_end_frame: end,
        game_duration_frames,
        stage_recognition,
    });
}

fn recognition_rank(status: StageMatchStatus) -> u8 {
    match status {
        StageMatchStatus::Unavailable => 0,
        StageMatchStatus::Partial => 1,
        StageMatchStatus::Ambiguous => 2,
        StageMatchStatus::Matched => 3,
    }
}

fn publish(events: &Mutex<MonitorEventQueue>, event: MonitorEvent) {
    if let Ok(mut events) = events.lock() {
        events.publish(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "需要本地录屏，设置 CONSOLE_RECORDING_PATH 与 CONSOLE_RECORDING_REPORT"]
    fn analyze_local_recording() {
        let path = std::env::var("CONSOLE_RECORDING_PATH").expect("CONSOLE_RECORDING_PATH");
        let report = std::env::var("CONSOLE_RECORDING_REPORT").expect("CONSOLE_RECORDING_REPORT");
        crate::diagnostics::initialize(std::path::PathBuf::from(format!("{report}.logs"))).unwrap();
        crate::diagnostics::set_enabled(true);
        let events = Mutex::new(MonitorEventQueue::default());
        let catalog = Arc::new(StageCatalog::embedded().unwrap());
        let start = Instant::now();
        let (session, _, _) = RecordingSession::start(
            &path,
            VisionConfig::default(),
            Arc::clone(&catalog),
            Arc::new(Mutex::new(MonitorEventQueue::default())),
        )
        .unwrap();
        let start_ms = start.elapsed().as_millis();
        session.stop();
        assert!(start_ms < 1000, "启动不应同步扫描录屏：{start_ms}ms");
        crate::diagnostics::info("recording", &format!("start_return_ms={start_ms}"));
        let probe_start = Instant::now();
        let metadata = probe(Path::new(&path), &AtomicBool::new(false)).unwrap();
        crate::diagnostics::info(
            "recording",
            &format!("probe_ms={}", probe_start.elapsed().as_millis()),
        );
        analyze_file(
            Path::new(&path),
            metadata,
            VisionConfig::default(),
            Arc::new(StageCatalog::embedded().unwrap()),
            &AtomicBool::new(false),
            &events,
        )
        .unwrap();
        let mut queue = events.lock().unwrap();
        let mut ready = false;
        while let Some(envelope) = queue.pop() {
            if let MonitorEvent::RecordingReady {
                trace,
                segments,
                candidates,
                duration_frames,
            } = envelope.event
            {
                let mut runner = crate::runner::RunnerState::new(Instant::now());
                runner.set_monitor_snapshot(super::super::MonitorSnapshot {
                    source_kind: super::super::MonitorSourceKind::Recording,
                    connection_state: super::super::MonitorConnectionState::Ready,
                    recording_analysis_id: Some("local-check".into()),
                    trace_points: trace.clone(),
                    recording_segments: segments.clone(),
                    recording_candidates: candidates.clone(),
                    trace_duration_frames: Some(duration_frames),
                    ..Default::default()
                });
                let axis = runner.snapshot().axis;
                let output = serde_json::json!({"trace": trace, "segments": segments, "candidates": candidates, "durationFrames": duration_frames, "axis": axis});
                std::fs::write(&report, serde_json::to_vec_pretty(&output).unwrap()).unwrap();
                if std::env::var_os("CONSOLE_RECORDING_CHECK_SR8").is_some() {
                    let reference: serde_json::Value = serde_json::from_str(include_str!(
                        "../../tests/fixtures/monitor/sr8-reference.json"
                    ))
                    .unwrap();
                    assert!(
                        trace.last().unwrap().source_timestamp_ns > 194_000_000_000.0,
                        "必须分析完整视频"
                    );
                    assert!(
                        axis.events.iter().all(|event| event.frame > 0),
                        "该视频没有 0 帧操作"
                    );
                    assert!(
                        axis.events.last().unwrap().frame > 180 * 30,
                        "轴不能在 10 秒结束"
                    );
                    for checkpoint in reference["checkpoints"].as_array().unwrap() {
                        let start = checkpoint["sourceStartSeconds"].as_f64().unwrap();
                        let end = checkpoint["sourceEndSeconds"].as_f64().unwrap();
                        let event = axis
                            .events
                            .iter()
                            .find(|event| {
                                event
                                    .source_timestamp_ns
                                    .is_some_and(|ns| ns / 1e9 >= start && ns / 1e9 <= end)
                            })
                            .unwrap_or_else(|| panic!("漏识别检查点：{checkpoint}"));
                        assert_eq!(
                            serde_json::to_value(event.kind).unwrap(),
                            checkpoint["kind"],
                            "{checkpoint}"
                        );
                        if let Some(expected) = checkpoint["gameSeconds"].as_f64() {
                            assert!(
                                (f64::from(event.frame) / 30.0 - expected).abs()
                                    <= checkpoint["toleranceSeconds"].as_f64().unwrap(),
                                "时间偏离检查点：{checkpoint} actual={}",
                                event.frame
                            );
                        }
                    }
                    assert!(
                        !axis.events.iter().any(|event| event.kind
                            == crate::axis::DraftKind::Deploy
                            && event
                                .source_timestamp_ns
                                .is_some_and(|ns| (12.0..16.0).contains(&(ns / 1e9)))),
                        "取消棋子预览不能算部署"
                    );
                }
                ready = true;
            }
        }
        assert!(ready);
        crate::diagnostics::export(&format!("{report}.log")).unwrap();
    }

    #[test]
    fn hud_numbers_do_not_confuse_cost_icon_and_slots() {
        let reading = parse_hud("0 16 剩 余 可 放 置 角 色 ： 8");
        assert_eq!(reading.cost, Some(16));
        assert_eq!(reading.slots, Some(8));
        let reading = parse_hud("剩 余 可 放 置 角 色 ： 7");
        assert_eq!(reading.cost, None);
        assert_eq!(reading.slots, Some(7));
    }

    #[test]
    fn selected_frames_keep_variable_rate_pts() {
        let time_base = SourceTimeBase::parse("1/90000").unwrap();
        for pts in [0, 3003, 7507, 12012] {
            let timestamp = parse_frame_timestamp(
                &format!("[Parsed_showinfo_2] n: 1 pts: {pts} pts_time: 0.1"),
                time_base,
            )
            .unwrap()
            .unwrap();
            assert_eq!(timestamp.raw_pts, pts.to_string());
            assert_eq!(timestamp.time_base, time_base);
        }
        assert!(parse_frame_timestamp(" n: 0 pts: NOPTS", time_base).is_err());
        assert!(parse_frame_timestamp(" n: 0 pts_time: 0.0", time_base).is_err());
        assert!(
            parse_frame_timestamp("config in time_base: 1/1000, frame_rate: 60/1", time_base)
                .is_err()
        );
    }

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
                    "avg_frame_rate": "60000/1001",
                    "time_base": "1/90000"
                }],
                "frames": [
                    { "best_effort_timestamp": "0" },
                    { "best_effort_timestamp": "1502" }
                ],
                "format": { "duration": "153.749" }
            }"#,
        )
        .unwrap();

        assert_eq!(metadata.width, 1920);
        assert_eq!(metadata.height, 1080);
        assert_eq!(metadata.stride, 4);
    }

    #[test]
    fn rejects_missing_video_stream() {
        let error = parse_probe_output(br#"{"streams":[],"format":{"duration":"1"}}"#).unwrap_err();
        assert_eq!(error, "录屏中没有视频流");
    }

    #[test]
    fn appends_indexed_recording_segment() {
        let mut segments = Vec::new();
        push_segment(&mut segments, 30, 300, 450, StageRecognition::default());
        push_segment(&mut segments, 600, 900, 300, StageRecognition::default());

        assert_eq!(segments[1].index, 1);
        assert_eq!(segments[1].source_start_frame, 600);
        assert_eq!(segments[1].game_duration_frames, 300);
    }
}
