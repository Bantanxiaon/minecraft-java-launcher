//! 启动窗口状态机：唯一的窗口交接 owner。
//!
//! 硬性 invariant（NextGen P0）：
//! 1. `main` window object exists != main visible；
//! 2. `show()` 返回成功 != main visible（SHOW_ACK_BUT_NOT_VISIBLE 是显式失败态）；
//! 3. 只有经过真实可见性确认后才允许关闭 splash；
//! 4. splash 关闭后的 grace 期若 main 无用户操作地变 hidden，必须自动恢复；
//! 5. bootstrap 阶段所有 show/hide/close 调用带 reason code 的结构化日志。

use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

static UI_READY: OnceLock<Notify> = OnceLock::new();
fn ui_ready() -> &'static Notify {
    UI_READY.get_or_init(Notify::new)
}

const UI_READY_TIMEOUT: Duration = Duration::from_secs(15);
const CONFIRM_WINDOW: Duration = Duration::from_secs(8);
const STABLE_PERIOD: Duration = Duration::from_millis(400);
const GRACE_MONITOR: Duration = Duration::from_secs(10);
const PROBE_INTERVAL: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupState {
    Boot,
    UiReady,
    MainShowRequested,
    MainVisibleProbing,
    MainVisibleConfirmed,
    SplashClosed,
    PostHandoffMonitoring,
    Ready,
    ShowAckButNotVisible,
    Failed,
}

fn log_state(app: &AppHandle, state: StartupState, reason: &str) {
    log::info!("startup-window state={state:?} reason={reason}");
    let _ = app.emit(
        "startup-window-state",
        serde_json::json!({
            "state": format!("{state:?}"),
            "reason": reason,
        }),
    );
}

/// 前端在 bootstrap 完成后调用，通知 Rust 可以开始主窗口交接。
#[tauri::command]
pub(crate) fn startup_ready() {
    ui_ready().notify_one();
}

/// 启动窗口协调器：唯一负责“显示主窗口 → 确认可见 → 关闭小窗 → grace 监控”。
pub(crate) fn init(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        log_state(&app, StartupState::Boot, "process_start");

        // 1. 等待前端 bootstrap（bounded，超时也继续，避免永远卡在小窗）。
        let wait = ui_ready().notified();
        tokio::pin!(wait);
        let _ = tokio::time::timeout(UI_READY_TIMEOUT, &mut wait).await;
        log_state(&app, StartupState::UiReady, "bootstrap_ready_or_timeout");

        let Some(main_window) = app.get_webview_window("main") else {
            log::error!("startup-window FAILED reason=main_window_missing");
            return;
        };
        let splash_window = app.get_webview_window("splash");

        // 2. 请求显示主窗口。
        log_state(&app, StartupState::MainShowRequested, "coordinator_show");
        let _ = main_window.show();
        log_state(&app, StartupState::MainVisibleProbing, "probing_started");

        // 3. 真实可见性确认：连续多次可见 + 非最小化 + 非离屏 + 原生 HWND 可见。
        if !confirm_visible(&main_window, CONFIRM_WINDOW).await {
            // SHOW_ACK_BUT_NOT_VISIBLE：记录诊断，尝试 restore + 原生兜底。
            log_state(
                &app,
                StartupState::ShowAckButNotVisible,
                "show_ack_but_not_visible",
            );
            log::warn!("startup-window SHOW_ACK_BUT_NOT_VISIBLE reason=is_visible_false");
            let _ = main_window.unminimize();
            let _ = main_window.show();
            native_show_window(&main_window);
            if !confirm_visible(&main_window, Duration::from_secs(5)).await {
                log_state(&app, StartupState::Failed, "main_visible_confirm_failed");
                // 主窗口确认失败：保留 splash，不关闭任何窗口，并让 splash 展示错误。
                let _ = app.emit(
                    "startup-window-error",
                    serde_json::json!({
                        "classification": "LAUNCHER_WINDOW_INVISIBLE",
                        "message": "主窗口启动失败，可点击“重试”或查看诊断。",
                        "action": "retry_or_diagnostics",
                    }),
                );
                return;
            }
        }
        log_state(
            &app,
            StartupState::MainVisibleConfirmed,
            "main_visible_confirmed",
        );

        // 4. 稳定期后关闭 splash。
        tokio::time::sleep(STABLE_PERIOD).await;
        if !confirm_visible(&main_window, Duration::from_millis(600)).await {
            log_state(&app, StartupState::Failed, "stability_check_failed");
            return;
        }
        if let Some(splash) = splash_window {
            let _ = splash.close();
        }
        log_state(
            &app,
            StartupState::SplashClosed,
            "splash_closed_after_visible_confirmed",
        );

        // 5. Grace 监控：splash 关闭后 10 秒内，无用户操作的 hide 必须恢复。
        log_state(
            &app,
            StartupState::PostHandoffMonitoring,
            "grace_monitor_started",
        );
        let deadline = Instant::now() + GRACE_MONITOR;
        let mut last_visible = main_window.is_visible().unwrap_or(false);
        while Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let visible = main_window.is_visible().unwrap_or(false);
            if !visible && last_visible {
                log::warn!("startup-window UNEXPECTED_POST_HANDOFF_HIDE reason=no_user_action");
                let _ = main_window.unminimize();
                let _ = main_window.show();
                native_show_window(&main_window);
                let _ = app.emit("startup-window-recovered", serde_json::json!({}));
            }
            last_visible = visible;
        }
        log_state(&app, StartupState::Ready, "startup_ready");
    });
}

/// 轮询确认主窗口真实可见：连续 2 次通过（间隔 PROBE_INTERVAL），
/// 且在超时时间内。判定条件：is_visible + 非最小化 + 非离屏 + 原生 HWND 可见。
async fn confirm_visible(main_window: &tauri::WebviewWindow, timeout: Duration) -> bool {
    confirm_visible_with(
        || window_really_visible(main_window),
        timeout,
        PROBE_INTERVAL,
    )
    .await
}

async fn confirm_visible_with<F>(mut check: F, timeout: Duration, interval: Duration) -> bool
where
    F: FnMut() -> bool,
{
    let deadline = Instant::now() + timeout;
    let mut consecutive = 0u32;
    while Instant::now() < deadline {
        if check() {
            consecutive += 1;
            if consecutive >= 2 {
                return true;
            }
        } else {
            consecutive = 0;
        }
        tokio::time::sleep(interval).await;
    }
    false
}

fn window_really_visible(main_window: &tauri::WebviewWindow) -> bool {
    if !main_window.is_visible().unwrap_or(false) {
        return false;
    }
    if main_window.is_minimized().unwrap_or(false) {
        return false;
    }
    if !native_window_visible(main_window) {
        return false;
    }
    if let (Ok(position), Ok(size), Ok(monitors)) = (
        main_window.outer_position(),
        main_window.outer_size(),
        main_window.available_monitors(),
    ) {
        let center = (
            position.x + (size.width as i32) / 2,
            position.y + (size.height as i32) / 2,
        );
        let rects: Vec<(i32, i32, i32, i32)> = monitors
            .iter()
            .map(|monitor| {
                let position = monitor.position();
                let size = monitor.size();
                (
                    position.x,
                    position.y,
                    position.x + size.width as i32,
                    position.y + size.height as i32,
                )
            })
            .collect();
        if !center_within_any(&rects, center.0, center.1) {
            return false;
        }
    }
    true
}

fn center_within_any(rects: &[(i32, i32, i32, i32)], x: i32, y: i32) -> bool {
    rects
        .iter()
        .any(|(left, top, right, bottom)| x >= *left && x < *right && y >= *top && y < *bottom)
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    #[link(name = "user32")]
    extern "system" {
        fn IsWindowVisible(hWnd: *const c_void) -> i32;
        fn ShowWindow(hWnd: *const c_void, nCmdShow: i32) -> i32;
    }

    pub const SW_RESTORE: i32 = 9;
    pub const SW_SHOWNORMAL: i32 = 1;

    pub fn is_visible(hwnd: *const c_void) -> bool {
        if hwnd.is_null() {
            return false;
        }
        unsafe { IsWindowVisible(hwnd) != 0 }
    }

    pub fn show(hwnd: *const c_void) {
        if !hwnd.is_null() {
            unsafe {
                ShowWindow(hwnd, SW_SHOWNORMAL);
            }
        }
    }

    pub fn restore(hwnd: *const c_void) {
        if !hwnd.is_null() {
            unsafe {
                ShowWindow(hwnd, SW_RESTORE);
            }
        }
    }
}

#[cfg(windows)]
fn native_window_visible(main_window: &tauri::WebviewWindow) -> bool {
    use raw_window_handle::HasWindowHandle;
    match main_window.window_handle() {
        Ok(handle) => match handle.as_raw() {
            raw_window_handle::RawWindowHandle::Win32(win32) => {
                win::is_visible(win32.hwnd.get() as *const std::ffi::c_void)
            }
            _ => true,
        },
        Err(_) => true,
    }
}

#[cfg(windows)]
fn native_show_window(main_window: &tauri::WebviewWindow) {
    use raw_window_handle::HasWindowHandle;
    if let Ok(handle) = main_window.window_handle() {
        if let raw_window_handle::RawWindowHandle::Win32(win32) = handle.as_raw() {
            let hwnd = win32.hwnd.get() as *const std::ffi::c_void;
            win::show(hwnd);
            win::restore(hwnd);
        }
    }
}

#[cfg(not(windows))]
fn native_window_visible(_main_window: &tauri::WebviewWindow) -> bool {
    true
}

#[cfg(not(windows))]
fn native_show_window(_main_window: &tauri::WebviewWindow) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn confirm_requires_two_consecutive_checks() {
        let mut calls = 0u32;
        let ok = confirm_visible_with(
            || {
                calls += 1;
                calls >= 3 // false, false, true, true
            },
            Duration::from_millis(2000),
            Duration::from_millis(10),
        )
        .await;
        assert!(ok);
        assert!(calls >= 4);
    }

    #[tokio::test]
    async fn confirm_times_out_when_never_visible() {
        let ok = confirm_visible_with(
            || false,
            Duration::from_millis(120),
            Duration::from_millis(20),
        )
        .await;
        assert!(!ok);
    }

    #[test]
    fn center_within_any_detects_offscreen() {
        let monitors = [(0, 0, 1920, 1080)];
        assert!(center_within_any(&monitors, 960, 540));
        assert!(!center_within_any(&monitors, 2000, 500));
        assert!(!center_within_any(&monitors, -10, 500));
    }

    #[test]
    fn center_within_any_supports_multi_monitor() {
        let monitors = [(-1920, 0, 0, 1080), (0, 0, 1920, 1080)];
        assert!(center_within_any(&monitors, -960, 500));
        assert!(center_within_any(&monitors, 960, 500));
        assert!(!center_within_any(&monitors, 2000, 500));
    }
}
