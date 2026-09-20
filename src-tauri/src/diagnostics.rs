use serde::Serialize;
use specta::Type;
use std::{
    collections::VecDeque,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LINES: usize = 4000;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
#[derive(Default)]
struct LogBuffer {
    enabled: bool,
    lines: VecDeque<String>,
    dropped: usize,
    directory: Option<PathBuf>,
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
pub fn initialize(directory: PathBuf) -> io::Result<()> {
    fs::create_dir_all(&directory)?;
    buffer()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .directory = Some(directory);
    info("application", "应用启动");
    Ok(())
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
    info(
        "diagnostics",
        if enabled {
            "调试模式已开启"
        } else {
            "调试模式已关闭"
        },
    );
    status()
}
pub fn info(target: &str, message: &str) {
    record("INFO", target, message);
}
pub fn debug(target: &str, message: &str) {
    record("DEBUG", target, message);
}
pub fn error(target: &str, message: &str) {
    record("ERROR", target, message);
}
fn append(directory: &Path, name: &str, line: &str) -> io::Result<()> {
    let path = directory.join(name);
    if fs::metadata(&path).is_ok_and(|metadata| metadata.len() >= MAX_FILE_BYTES) {
        let previous = directory.join(format!("{name}.previous"));
        if previous.exists() {
            fs::remove_file(&previous)?;
        }
        fs::rename(&path, previous)?;
    }
    writeln!(
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?,
        "{line}"
    )
}
fn record(level: &str, target: &str, message: &str) {
    record_to_buffer(
        &mut buffer().lock().unwrap_or_else(|error| error.into_inner()),
        level,
        target,
        message,
    );
}
fn record_to_buffer(log: &mut LogBuffer, level: &str, target: &str, message: &str) {
    if level == "DEBUG" && !log.enabled {
        return;
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let line = format!(
        "{timestamp} {level} {target}: {}",
        message.replace(['\r', '\n'], " ")
    );
    if let Some(directory) = &log.directory {
        let _ = append(directory, "history.log", &line);
        if level == "ERROR" {
            let _ = append(directory, "errors.log", &line);
        }
    }
    if log.lines.len() == MAX_LINES {
        log.lines.pop_front();
        log.dropped += 1;
    }
    log.lines.push_back(line);
}
fn read_locked(log: &LogBuffer, errors_only: bool) -> io::Result<String> {
    if let Some(directory) = &log.directory {
        let name = if errors_only {
            "errors.log"
        } else {
            "history.log"
        };
        let mut text = String::new();
        for file in [format!("{name}.previous"), name.to_string()] {
            match fs::read_to_string(directory.join(file)) {
                Ok(contents) => text.push_str(&contents),
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(error),
            }
        }
        Ok(text)
    } else {
        Ok(log
            .lines
            .iter()
            .filter(|line| !errors_only || line.contains(" ERROR "))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n"))
    }
}
pub fn read(errors_only: bool) -> io::Result<String> {
    read_locked(
        &buffer().lock().unwrap_or_else(|error| error.into_inner()),
        errors_only,
    )
}
#[cfg(test)]
pub fn export(path: &str) -> io::Result<()> {
    fs::write(path, read(false)?)
}
pub fn export_zip(path: &str) -> io::Result<()> {
    let (history, errors) = {
        let log = buffer().lock().unwrap_or_else(|error| error.into_inner());
        (read_locked(&log, false)?, read_locked(&log, true)?)
    };
    let directory = std::env::temp_dir().join(format!(
        "console-logs-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::create_dir(&directory)?;
    let result = (|| {
        fs::write(directory.join("history.log"), history)?;
        fs::write(directory.join("errors.log"), errors)?;
        let destination = std::path::absolute(path)?;
        let mut command = std::process::Command::new("tar");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let result = command
            .arg("--format=zip")
            .arg("-cf")
            .arg(destination)
            .arg("-C")
            .arg(&directory)
            .args(["history.log", "errors.log"])
            .output()?;
        if !result.status.success() {
            return Err(io::Error::other("日志 ZIP 打包失败"));
        }
        Ok(())
    })();
    let _ = fs::remove_file(directory.join("history.log"));
    let _ = fs::remove_file(directory.join("errors.log"));
    let _ = fs::remove_dir(directory);
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn debug_switch_keeps_info_and_errors() {
        let mut log = LogBuffer {
            enabled: true,
            ..Default::default()
        };
        for _ in 0..MAX_LINES + 2 {
            record_to_buffer(&mut log, "DEBUG", "test", "entry");
        }
        assert_eq!(log.lines.len(), MAX_LINES);
        assert_eq!(log.dropped, 2);
        log.enabled = false;
        record_to_buffer(&mut log, "DEBUG", "test", "ignored");
        assert_eq!(log.dropped, 2);
        record_to_buffer(&mut log, "INFO", "test", "normal-entry");
        record_to_buffer(&mut log, "ERROR", "test", "error-entry");
        assert!(read_locked(&log, false).unwrap().contains("normal-entry"));
        assert!(read_locked(&log, true).unwrap().contains("error-entry"));
        assert!(!read_locked(&log, true).unwrap().contains("normal-entry"));
    }
    #[test]
    #[cfg(windows)]
    fn exports_both_logs_in_a_readable_zip() {
        let path =
            std::env::temp_dir().join(format!("console-zip-test-{}.zip", std::process::id()));
        export_zip(path.to_str().unwrap()).unwrap();
        assert!(fs::read(&path).unwrap().starts_with(b"PK"));
        let output = std::process::Command::new("tar")
            .arg("-tf")
            .arg(&path)
            .output()
            .unwrap();
        assert!(output.status.success());
        let listing = String::from_utf8(output.stdout).unwrap();
        assert!(listing.contains("history.log"));
        assert!(listing.contains("errors.log"));
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn disk_history_survives_buffer_rotation() {
        let directory =
            std::env::temp_dir().join(format!("console-log-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        append(&directory, "history.log", "old session").unwrap();
        let log = LogBuffer {
            directory: Some(directory.clone()),
            ..Default::default()
        };
        assert!(read_locked(&log, false).unwrap().contains("old session"));
        assert!(read_locked(&log, true).unwrap().is_empty());
        fs::remove_file(directory.join("history.log")).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
