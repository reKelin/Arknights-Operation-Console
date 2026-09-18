use std::{thread, time::Duration};

#[cfg(windows)]
use windows::{
    Win32::{
        Foundation::{HWND, POINT, RECT},
        Graphics::Gdi::ClientToScreen,
        UI::{
            Input::Pointer::{
                InitializeTouchInjection, InjectTouchInput, POINTER_FLAG_CANCELED,
                POINTER_FLAG_DOWN, POINTER_FLAG_INCONTACT, POINTER_FLAG_INRANGE, POINTER_FLAG_UP,
                POINTER_FLAG_UPDATE, POINTER_FLAGS, POINTER_TOUCH_INFO, TOUCH_FEEDBACK_DEFAULT,
            },
            WindowsAndMessaging::{
                GetClientRect, GetForegroundWindow, IsWindow, PT_TOUCH, TOUCH_MASK_CONTACTAREA,
                TOUCH_MASK_ORIENTATION, TOUCH_MASK_PRESSURE,
            },
        },
    },
    core::Result as WindowsResult,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientSize {
    pub width: i32,
    pub height: i32,
}

#[cfg(windows)]
pub struct TouchInjector {
    active: bool,
    last: POINT,
}

#[cfg(windows)]
impl TouchInjector {
    pub fn new() -> Result<Self, String> {
        unsafe { InitializeTouchInjection(1, TOUCH_FEEDBACK_DEFAULT) }
            .map_err(|error| format!("初始化 Windows 触摸输入失败：{error}"))?;
        Ok(Self {
            active: false,
            last: POINT::default(),
        })
    }

    pub fn validate_window(window_id: &str) -> Result<(HWND, ClientSize), String> {
        let raw =
            usize::from_str_radix(window_id, 16).map_err(|_| "游戏窗口句柄无效".to_string())?;
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        if unsafe { !IsWindow(Some(hwnd)).as_bool() } {
            return Err("游戏窗口已失效".to_string());
        }
        if unsafe { GetForegroundWindow() } != hwnd {
            return Err("游戏窗口不在前台".to_string());
        }
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) }
            .map_err(|error| format!("读取游戏客户区失败：{error}"))?;
        let size = ClientSize {
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        };
        if size.width < 640 || size.height < 360 {
            return Err("游戏客户区尺寸过小".to_string());
        }
        Ok((hwnd, size))
    }

    pub fn tap(
        &mut self,
        hwnd: HWND,
        client: (i32, i32),
        allowed: impl Fn() -> bool,
    ) -> Result<(), String> {
        if !allowed() {
            return Err("代理执行已取消（尚未发送输入）".to_string());
        }
        let screen = client_to_screen(hwnd, client)?;
        self.inject(
            screen,
            POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT | POINTER_FLAG_DOWN,
        )?;
        self.active = true;
        thread::sleep(Duration::from_millis(16));
        if !allowed() {
            self.cancel();
            return Err("代理执行已取消（输入结果未知）".to_string());
        }
        self.inject(
            screen,
            POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT | POINTER_FLAG_UPDATE,
        )?;
        self.inject(screen, POINTER_FLAG_UP)?;
        self.active = false;
        Ok(())
    }

    pub fn drag(
        &mut self,
        hwnd: HWND,
        from: (i32, i32),
        to: (i32, i32),
        allowed: impl Fn() -> bool,
    ) -> Result<(), String> {
        if !allowed() {
            return Err("代理执行已取消（尚未发送输入）".to_string());
        }
        let from = client_to_screen(hwnd, from)?;
        let to = client_to_screen(hwnd, to)?;
        self.inject(
            from,
            POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT | POINTER_FLAG_DOWN,
        )?;
        self.active = true;
        for step in 1..=5 {
            thread::sleep(Duration::from_millis(16));
            if !allowed() {
                self.cancel();
                return Err("代理执行已取消（输入结果未知）".to_string());
            }
            let point = POINT {
                x: from.x + (to.x - from.x) * step / 5,
                y: from.y + (to.y - from.y) * step / 5,
            };
            self.inject(
                point,
                POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT | POINTER_FLAG_UPDATE,
            )?;
        }
        self.inject(to, POINTER_FLAG_UP)?;
        self.active = false;
        Ok(())
    }

    pub fn cancel(&mut self) {
        if self.active {
            let _ = self.inject(self.last, POINTER_FLAG_CANCELED | POINTER_FLAG_UP);
            self.active = false;
        }
    }

    fn inject(&mut self, point: POINT, flags: POINTER_FLAGS) -> Result<(), String> {
        self.last = point;
        let contact = POINTER_TOUCH_INFO {
            pointerInfo: windows::Win32::UI::Input::Pointer::POINTER_INFO {
                pointerType: PT_TOUCH,
                pointerId: 1,
                pointerFlags: flags,
                ptPixelLocation: point,
                ..Default::default()
            },
            touchMask: TOUCH_MASK_CONTACTAREA | TOUCH_MASK_ORIENTATION | TOUCH_MASK_PRESSURE,
            rcContact: RECT {
                left: point.x - 2,
                top: point.y - 2,
                right: point.x + 2,
                bottom: point.y + 2,
            },
            orientation: 90,
            pressure: 32_000,
            ..Default::default()
        };
        retry_not_ready(|| unsafe { InjectTouchInput(&[contact]) })
            .map_err(|error| format!("发送 Windows 触摸输入失败：{error}"))
    }
}

#[cfg(windows)]
impl Drop for TouchInjector {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(windows)]
fn retry_not_ready(action: impl Fn() -> WindowsResult<()>) -> WindowsResult<()> {
    let mut result = action();
    if result.is_err() {
        thread::sleep(Duration::from_millis(1));
        result = action();
    }
    result
}

#[cfg(windows)]
fn client_to_screen(hwnd: HWND, client: (i32, i32)) -> Result<POINT, String> {
    let mut point = POINT {
        x: client.0,
        y: client.1,
    };
    unsafe { ClientToScreen(hwnd, &mut point) }
        .ok()
        .map_err(|error| format!("转换游戏屏幕坐标失败：{error}"))?;
    Ok(point)
}

#[cfg(not(windows))]
pub struct TouchInjector;

#[cfg(not(windows))]
impl TouchInjector {
    pub fn new() -> Result<Self, String> {
        Err("代理执行仅支持 Windows 10/11".to_string())
    }
}
