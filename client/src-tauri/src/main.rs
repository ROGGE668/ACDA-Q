#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_store::StoreExt;
use sha2::{Sha256, Digest};
use sysinfo::{System, RefreshKind, CpuRefreshKind};

const STORE_PATH: &str = "settings.json";
const DEVICE_STORE_PATH: &str = "device.json";
const DEFAULT_API_BASE: &str = "http://100.68.25.78:8000/api/v1";
const OLD_API_BASES: &[&str] = &[
    "http://124.220.70.210:8000/api/v1",
];

// 设置默认配置
fn init_default_settings(app: &AppHandle) {
    if let Ok(store) = app.store(STORE_PATH) {
        // 检查存储的 api_base 是否为旧 IP，自动迁移
        let stored_api_base = store.get("api_base");
        let needs_migration = stored_api_base.as_ref().and_then(|v| v.as_str()).is_some_and(|v| {
            OLD_API_BASES.iter().any(|old| v == *old || v.starts_with(old))
        });
        if needs_migration {
            let _ = store.set("api_base", DEFAULT_API_BASE);
        } else if store.get("api_base").is_none() {
            let _ = store.set("api_base", DEFAULT_API_BASE);
        }
        if store.get("theme").is_none() {
            let _ = store.set("theme", "dark");
        }
        let _ = store.save();
    }
}

// 生成设备指纹：基于硬件 + 系统信息，首次安装时固定
fn generate_device_fingerprint() -> String {
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_cpu(CpuRefreshKind::everything()),
    );
    sys.refresh_all();

    let cpu_brand = sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_default();
    let cpu_cores = sys.physical_core_count().unwrap_or(0);
    let total_memory = sys.total_memory();

    let os_type = std::env::consts::OS;
    let os_arch = std::env::consts::ARCH;

    let hostname = System::host_name().unwrap_or_default();

    let raw = format!(
        "{}|{}|{}|{}|{}|{}",
        cpu_brand, cpu_cores, total_memory, os_type, os_arch, hostname
    );

    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

// 初始化设备码：如果不存在则生成并保存
fn init_device_fingerprint(app: &AppHandle) -> String {
    if let Ok(store) = app.store(DEVICE_STORE_PATH) {
        if let Some(v) = store.get("device_fingerprint") {
            if let Some(s) = v.as_str() {
                return s.to_string();
            }
        }
        let fp = generate_device_fingerprint();
        let _ = store.set("device_fingerprint", fp.clone());
        let _ = store.save();
        return fp;
    }
    generate_device_fingerprint()
}

#[tauri::command]
fn get_device_fingerprint(app: AppHandle) -> String {
    init_device_fingerprint(&app)
}

// 创建 macOS 原生菜单
fn create_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let app_menu = Submenu::with_items(
        app,
        "ACDA-Quant",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("关于 ACDA-Quant"), None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some("隐藏 ACDA-Quant"))?,
            &PredefinedMenuItem::hide_others(app, Some("隐藏其他"))?,
            &PredefinedMenuItem::show_all(app, Some("显示全部"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some("退出 ACDA-Quant"))?,
        ],
    )?;

    let edit_menu = Submenu::with_items(
        app,
        "编辑",
        true,
        &[
            &PredefinedMenuItem::undo(app, Some("撤销"))?,
            &PredefinedMenuItem::redo(app, Some("重做"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, Some("剪切"))?,
            &PredefinedMenuItem::copy(app, Some("复制"))?,
            &PredefinedMenuItem::paste(app, Some("粘贴"))?,
            &PredefinedMenuItem::select_all(app, Some("全选"))?,
        ],
    )?;

    let view_menu = Submenu::with_items(
        app,
        "视图",
        true,
        &[
            &MenuItem::with_id(app, "reload", "重新加载", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "toggle_devtools", "开发者工具", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "settings", "设置...", true, Some("CmdOrCtrl+,"))?,
        ],
    )?;

    let window_menu = Submenu::with_items(
        app,
        "窗口",
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some("最小化"))?,
            &PredefinedMenuItem::maximize(app, Some("最大化"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, Some("关闭窗口"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::fullscreen(app, Some("全屏"))?,
        ],
    )?;

    Menu::with_items(
        app,
        &[
            &app_menu,
            &edit_menu,
            &view_menu,
            &window_menu,
        ],
    )
}

// 显示/隐藏窗口
fn toggle_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

// 创建系统托盘
fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let show_i = MenuItem::with_id(app, "show", "显示", true, None::<&str>)?;
    let hide_i = MenuItem::with_id(app, "hide", "隐藏", true, None::<&str>)?;
    let settings_i = MenuItem::with_id(app, "tray_settings", "设置", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let tray_menu = Menu::with_items(app, &[&show_i, &hide_i, &settings_i, &PredefinedMenuItem::separator(app)?, &quit_i])?;

    let tray = TrayIconBuilder::new()
        .tooltip("ACDA-Quant")
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&tray_menu)
        .show_menu_on_left_click(true)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button, button_state, .. } = event {
                if button == MouseButton::Left && button_state == MouseButtonState::Up {
                    let app = tray.app_handle();
                    toggle_window(app);
                }
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "hide" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            "tray_settings" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                    let _ = window.eval("window.location.href = '/settings'");
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    // 保持 tray 存活
    let _ = tray;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![get_device_fingerprint])
        .setup(|app| {
            init_default_settings(app.handle());
            let _fp = init_device_fingerprint(app.handle());
            println!("[Device] fingerprint initialized");

            let menu = create_menu(app.handle())?;
            app.set_menu(menu)?;

            // 菜单事件处理
            app.on_menu_event(|app, event| {
                if let Some(window) = app.get_webview_window("main") {
                    match event.id().as_ref() {
                        "reload" => {
                            let _ = window.eval("window.location.reload()");
                        }
                        "toggle_devtools" => {
                            #[cfg(debug_assertions)]
                            {
                                let _ = window.open_devtools();
                            }
                        }
                        "settings" => {
                            let _ = window.eval("window.location.href = '/settings'");
                        }
                        _ => {}
                    }
                }
            });

            create_tray(app.handle())?;

            // macOS: 关闭窗口时不退出，只隐藏
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Regular);
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    #[cfg(target_os = "macos")]
                    {
                        // macOS: 关闭按钮只隐藏窗口
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
