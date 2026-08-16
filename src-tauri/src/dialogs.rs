use tauri::{AppHandle, Manager};

/// 错误对话框（spec §6）：sidecar 启动失败 stderr tail 20 / frontendDist 缺失 / socket 权限异常。
/// R2 修正：§4.5 排除 dialog 插件 → 用主窗口原生 alert（eval）
pub fn show_error_dialog(app: &AppHandle, title: &str, body: &str) {
    if let Some(win) = app.get_webview_window("main") {
        let js = format!("alert({:?})", format!("{title}\n\n{body}"));
        let _ = win.eval(&js);
    }
}
