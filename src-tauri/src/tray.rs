use tauri::{AppHandle, Manager, tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState}};

pub fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show_i = tauri::menu::MenuBuilder::new(app)
        .text("show", "显示窗口")
        .text("quit", "退出")
        .build()?;

    TrayIconBuilder::with_id("main-tray")
        // R6 修正：托盘图标显式取自上游 anywhere-labs template PNG（32px @2x 单图，
        // tray-icon crate 无 @2x 自动检测，NSImage 强制 18pt，32px 背板 retina 清晰）。
        // bundle.icon（icons/icon.icns）仍保留供 Dock 图标，与托盘无关。
        .icon(tauri::image::Image::from_bytes(include_bytes!("../icons/tray-iconTemplate@2x.png")).unwrap_or_else(|_| {
            // 兜底：1×1 透明占位（不崩启动）
            tauri::image::Image::new_owned(vec![0; 4], 1, 1)
        }))
        .icon_as_template(true)
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
