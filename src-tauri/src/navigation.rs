use url::Url;

/// 导航白名单（spec §4.2）：仅放行 tauri://localhost 与 http://ipc.localhost；
/// dev 构建额外放行 vite dev server（cfg!(debug_assertions) 门控）。
/// DeepSec L3：前缀匹配 → URL 解析后比较 scheme+host（+dev port），拒绝 host 后缀/端口混淆。
pub fn allowed_navigation(url: &str, debug: bool) -> bool {
    let Ok(parsed) = Url::parse(url) else { return false; };
    let host = parsed.host_str().unwrap_or("");

    if (parsed.scheme() == "tauri" && host == "localhost") {
        return true;
    }
    if parsed.scheme() == "http" && host == "ipc.localhost" {
        return true;
    }
    // dev：仅 localhost/127.0.0.1 且端口恰为 1420（防 14200 等混淆）
    if debug
        && parsed.scheme() == "http"
        && (host == "localhost" || host == "127.0.0.1")
        && parsed.port() == Some(1420)
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_tauri_localhost() {
        assert!(allowed_navigation("tauri://localhost/", false));
        assert!(allowed_navigation("tauri://localhost/index.html", false));
    }

    #[test]
    fn allows_ipc_localhost() {
        assert!(allowed_navigation("http://ipc.localhost", false));
    }

    #[test]
    fn rejects_external_links() {
        assert!(!allowed_navigation("https://example.com", false));
        assert!(!allowed_navigation("https://evil.com/phish", false));
    }

    #[test]
    fn rejects_host_suffix_spoofing() {
        // DeepSec L3：前缀匹配绕过向量
        assert!(!allowed_navigation("tauri://localhost.evil.com/", false));
        assert!(!allowed_navigation("http://ipc.localhost.evil.com", false));
        assert!(!allowed_navigation("tauri://localhost@evil.com/", false)); // userinfo
    }

    #[test]
    fn dev_url_only_in_debug_exact_port() {
        assert!(allowed_navigation("http://localhost:1420/", true));
        assert!(!allowed_navigation("http://localhost:1420/", false));
        // DeepSec L3：端口混淆
        assert!(!allowed_navigation("http://127.0.0.1:14200/", true));
        assert!(!allowed_navigation("http://localhost:1420.evil.com/", true));
    }

    #[test]
    fn rejects_file_scheme() {
        assert!(!allowed_navigation("file:///etc/passwd", false));
    }
}
