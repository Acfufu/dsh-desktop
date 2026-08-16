use tauri::{AppHandle, Manager, tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState}};

pub fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show_i = tauri::menu::MenuBuilder::new(app)
        .text("show", "显示窗口")
        .text("quit", "退出")
        .build()?;

    TrayIconBuilder::with_id("main-tray")
        // R3 修正：conf 加 bundle.icon 后 default_window_icon() 非 None；仍用 if let Some 防 panic
        .icon(app.default_window_icon().cloned().unwrap_or_else(|| {
            // 兜底：1×1 透明占位（不崩启动）
            tauri::image::Image::new_owned(vec![0; 4], 1, 1)
        }))
        .menu(&show_i)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => { show_main_window(app); }
            "quit" => { app.exit(0); }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}
