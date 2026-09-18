use std::{collections::BTreeSet, thread, time::Duration};

#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY,
};

pub fn parse_virtual_key(name: &str) -> Result<u16, String> {
    let normalized = name.trim().to_ascii_uppercase();
    if normalized.len() == 1 {
        let value = normalized.as_bytes()[0];
        if value.is_ascii_uppercase() || value.is_ascii_digit() {
            return Ok(u16::from(value));
        }
    }
    let key = match normalized.as_str() {
        "SPACE" => 0x20,
        "ESC" | "ESCAPE" => 0x1B,
        "TAB" => 0x09,
        "ENTER" => 0x0D,
        "BACKSPACE" => 0x08,
        "LEFT" => 0x25,
        "UP" => 0x26,
        "RIGHT" => 0x27,
        "DOWN" => 0x28,
        "F1" => 0x70,
        "F2" => 0x71,
        "F3" => 0x72,
        "F4" => 0x73,
        "F5" => 0x74,
        "F6" => 0x75,
        "F7" => 0x76,
        "F8" => 0x77,
        "F9" => 0x78,
        "F10" => 0x79,
        "F11" => 0x7A,
        "F12" => 0x7B,
        _ => return Err(format!("不支持的游戏键位：{name}")),
    };
    Ok(key)
}

#[cfg(windows)]
pub struct KeyboardInjector {
    active: BTreeSet<u16>,
}

#[cfg(windows)]
impl KeyboardInjector {
    pub fn new() -> Self {
        Self {
            active: BTreeSet::new(),
        }
    }

    pub fn press(&mut self, key: u16, allowed: impl Fn() -> bool) -> Result<(), String> {
        if !allowed() {
            return Err("代理执行已取消（尚未发送输入）".to_string());
        }
        self.send(key, false)?;
        self.active.insert(key);
        thread::sleep(Duration::from_millis(16));
        if !allowed() {
            self.cancel();
            return Err("代理执行已取消（输入结果未知）".to_string());
        }
        self.send(key, true)?;
        self.active.remove(&key);
        Ok(())
    }

    pub fn cancel(&mut self) {
        for key in std::mem::take(&mut self.active) {
            let _ = self.send(key, true);
        }
    }

    fn send(&self, key: u16, released: bool) -> Result<(), String> {
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(key),
                    dwFlags: if released {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    ..Default::default()
                },
            },
        };
        let sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
        if sent == 1 {
            Ok(())
        } else {
            Err("发送 Windows 键盘输入失败".to_string())
        }
    }
}

#[cfg(windows)]
impl Drop for KeyboardInjector {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(not(windows))]
pub struct KeyboardInjector;

#[cfg(not(windows))]
impl KeyboardInjector {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_supported_key_names() {
        assert_eq!(parse_virtual_key("d"), Ok(u16::from(b'D')));
        assert_eq!(parse_virtual_key("F12"), Ok(0x7B));
        assert!(parse_virtual_key("Ctrl+D").is_err());
    }
}
