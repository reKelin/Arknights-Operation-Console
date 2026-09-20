use serde::Serialize;
use specta::Type;
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LINES: usize = 4000;
#[derive(Default)]
struct LogBuffer {
    enabled: bool,
    lines: VecDeque<String>,
    dropped: usize,
}
static LOG: OnceLock<Mutex<LogBuffer>> = OnceLock::new();
fn buffer() -> &'static Mutex<LogBuffer> {
    LOG.get_or_init(|| Mutex::new(LogBuffer::default()))
}

#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LogStatus {
    pub enabled: bool,
    pub lines: u32,
    pub dropped: u32,
}
pub fn status() -> LogStatus {
    let log = buffer().lock().unwrap_or_else(|error| error.into_inner());
    LogStatus {
        enabled: log.enabled,
        lines: log.lines.len() as u32,
        dropped: log.dropped.min(u32::MAX as usize) as u32,
    }
}
pub fn set_enabled(enabled: bool) -> LogStatus {
    buffer()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .enabled = enabled;
    if enabled {
        info("diagnostics", "日志已开启（仅当前会话，最多保留 4000 条）");
    }
    status()
}
pub fn info(target: &str, message: &str) {
    record("INFO", target, message);
}
pub fn debug(target: &str, message: &str) {
    record("DEBUG", target, message);
}
fn record(level: &str, target: &str, message: &str) {
    let mut log = buffer().lock().unwrap_or_else(|error| error.into_inner());
    if !log.enabled {
        return;
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    if log.lines.len() == MAX_LINES {
        log.lines.pop_front();
        log.dropped += 1;
    }
    log.lines.push_back(format!(
        "{timestamp} {level} {target}: {}",
        message.replace(['\r', '\n'], " ")
    ));
}
pub fn export(path: &str) -> std::io::Result<()> {
    let text = {
        let log = buffer().lock().unwrap_or_else(|error| error.into_inner());
        format!(
            "Arknights Operation Console {} | timestamps: Unix milliseconds | dropped: {}\n{}\n",
            env!("CARGO_PKG_VERSION"),
            log.dropped,
            log.lines.iter().cloned().collect::<Vec<_>>().join("\n")
        )
    };
    std::fs::write(path, text)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_and_disabled_logs() {
        set_enabled(true);
        for _ in 0..MAX_LINES + 2 {
            debug("test", "entry");
        }
        assert_eq!(status().lines, MAX_LINES as u32);
        assert!(status().dropped >= 2);
        set_enabled(false);
        let before = status().dropped;
        info("test", "ignored");
        assert_eq!(status().dropped, before);
    }
}
