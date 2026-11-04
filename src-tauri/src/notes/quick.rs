//! 迷你快速记录窗口与全局快捷键。

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const LABEL: &str = "quick-capture";
static REGISTERED: AtomicBool = AtomicBool::new(false);

fn shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyN)
}

fn reveal(window: &tauri::WebviewWindow) {
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.center();
    let _ = window.set_focus();
}

/// 显示或聚焦迷你窗口；首次调用时创建
///
/// 窗口创建会阻塞等待主事件循环（tauri-runtime-wry 的 create_window 会
/// `rx.recv()`），而 IPC / 托盘 / 快捷键回调都跑在主线程上，直接 build 会
/// 把整个应用锁死（表现为窗口白屏、所有 invoke 无响应）。因此首次创建
/// 必须放到独立线程里。
pub fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        reveal(&window);
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        // 并发调用时可能已被另一个线程创建，二次检查避免重复建窗
        if let Some(window) = app.get_webview_window(LABEL) {
            reveal(&window);
            return;
        }
        match WebviewWindowBuilder::new(
            &app,
            LABEL,
            WebviewUrl::App("index.html#/quick-capture".into()),
        )
        .title("快速记录")
        .inner_size(520.0, 200.0)
        .min_inner_size(420.0, 160.0)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .center()
        .build()
        {
            Ok(window) => reveal(&window),
            Err(error) => log::warn!("创建快速记录窗口失败：{error}"),
        }
    });
}

/// 按设置注册 / 注销 Ctrl+Alt+N；注册失败（被其他软件占用）返回中文原因
pub fn sync_shortcut(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.global_shortcut();
    let registered = REGISTERED.load(Ordering::SeqCst);
    if enabled {
        if registered {
            return Ok(());
        }
        manager
            .on_shortcut(shortcut(), |app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    let app = app.clone();
                    let _ = app.clone().run_on_main_thread(move || show(&app));
                }
            })
            .map_err(|error| {
                format!("注册全局快捷键 Ctrl+Alt+N 失败，可能被其他软件占用：{error}")
            })?;
        REGISTERED.store(true, Ordering::SeqCst);
    } else if registered {
        let _ = manager.unregister(shortcut());
        REGISTERED.store(false, Ordering::SeqCst);
    }
    Ok(())
}
