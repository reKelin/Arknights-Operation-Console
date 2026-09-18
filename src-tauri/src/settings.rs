use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use specta::Type;

const SETTINGS_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AppTheme {
    Dark,
    Light,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppSettings {
    pub version: u8,
    pub theme: AppTheme,
    pub frames_per_cost: u16,
    pub game_ui_scale: u8,
    #[serde(default = "default_pause_key")]
    pub pause_key: String,
    #[serde(default = "default_skill_key")]
    pub skill_key: String,
    #[serde(default = "default_retreat_key")]
    pub retreat_key: String,
    #[serde(default)]
    pub bindings_confirmed: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            theme: AppTheme::Dark,
            frames_per_cost: 30,
            game_ui_scale: 100,
            pause_key: default_pause_key(),
            skill_key: default_skill_key(),
            retreat_key: default_retreat_key(),
            bindings_confirmed: false,
        }
    }
}

impl AppSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != SETTINGS_VERSION {
            return Err(format!("不支持设置版本 {}", self.version));
        }
        if !(15..=150).contains(&self.frames_per_cost) {
            return Err("费用逻辑秒分母必须在 15–150 之间".to_string());
        }
        if self.game_ui_scale > 100 {
            return Err("游戏 UI 比例必须在 0–100 之间".to_string());
        }
        crate::executor::validate_key_name(&self.pause_key)?;
        crate::executor::validate_key_name(&self.skill_key)?;
        crate::executor::validate_key_name(&self.retreat_key)?;
        if self.pause_key.eq_ignore_ascii_case(&self.skill_key)
            || self.pause_key.eq_ignore_ascii_case(&self.retreat_key)
            || self.skill_key.eq_ignore_ascii_case(&self.retreat_key)
        {
            return Err("暂停、技能和撤退必须使用不同键位".to_string());
        }
        Ok(())
    }

    pub fn load(path: &Path) -> (Self, Option<String>) {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return (Self::default(), None);
            }
            Err(error) => {
                return (
                    Self::default(),
                    Some(format!("读取设置失败，已使用默认值：{error}")),
                );
            }
        };
        match serde_json::from_str::<Self>(&text)
            .map_err(|error| format!("设置文件格式无效：{error}"))
            .and_then(|settings| {
                settings.validate()?;
                Ok(settings)
            }) {
            Ok(settings) => (settings, None),
            Err(message) => (Self::default(), Some(format!("{message}，已使用默认值"))),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let temporary = temporary_path(path);
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        fs::write(&temporary, format!("{text}\n"))?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(temporary, path)
    }
}

fn default_pause_key() -> String {
    "Escape".to_string()
}

fn default_skill_key() -> String {
    "D".to_string()
}

fn default_retreat_key() -> String {
    "A".to_string()
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "settings.json".into());
    name.push(".tmp");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_reject_invalid_denominator() {
        let settings = AppSettings {
            frames_per_cost: 14,
            ..AppSettings::default()
        };

        assert!(settings.validate().is_err());
    }
}
