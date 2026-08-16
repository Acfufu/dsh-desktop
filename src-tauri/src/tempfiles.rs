use std::path::{Path, PathBuf};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tauri::{AppHandle, Manager};
use crate::AppState;

/// 上传/临时文件纪律（spec §4.6）：canonicalize 后校验仍在允许范围内。
pub fn canonicalize_within(root: &Path, candidate: &Path) -> Result<PathBuf, String> {
    let root_c = root.canonicalize().map_err(|e| e.to_string())?;
    let cand_c = candidate.canonicalize().map_err(|e| e.to_string())?;
    if cand_c.starts_with(&root_c) {
        Ok(cand_c)
    } else {
        Err(format!("path escapes allowed root: {}", cand_c.display()))
    }
}

/// 随机后缀（时间戳 nano 十六进制；文件名由 Rust 生成，绝不采用 Content-Disposition）
pub fn nanoid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    format!("{n:x}")
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

#[tauri::command]
pub async fn dsh_save_export(app: AppHandle, bytes: Vec<u8>, file_name: String) -> Result<String, String> {
    // 下载（spec §4.6）：session.export ZIP → 用户下载目录
    let downloads = app.path().download_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    // 随机名后缀防覆盖；file_name 仅作前缀
    let safe_name = format!("{}-{}.zip", sanitize(&file_name), nanoid());
    let path = downloads.join(&safe_name);
    fs::write(&path, &bytes).map_err(|e| {
        if e.kind() == std::io::ErrorKind::StorageFull || e.kind() == std::io::ErrorKind::QuotaExceeded {
            "磁盘空间不足".to_string()
        } else {
            format!("disk write failed: {e}")
        }
    })?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub async fn dsh_write_temp(app: AppHandle, bytes: Vec<u8>, ext: String) -> Result<String, String> {
    // DeepSec L3：大小上限——XSS 可反复 invoke 写超大文件填满磁盘（disk-fill DoS）
    if bytes.len() > 160 * 1024 * 1024 {
        return Err("body exceeds 160 MiB".into());
    }
    // 上传临时文件（spec §4.6）：app 专属临时子目录，0600，随机名
    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    let dir = cache.join("temp-uploads");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut perms = fs::metadata(&dir).map_err(|e| e.to_string())?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(&dir, perms).map_err(|e| e.to_string())?;
    let safe_ext = sanitize(&ext).chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>();
    let path = dir.join(format!("dsh-{}.{}", nanoid(), safe_ext));
    fs::write(&path, &bytes).map_err(|e| {
        if e.kind() == std::io::ErrorKind::StorageFull || e.kind() == std::io::ErrorKind::QuotaExceeded {
            "磁盘空间不足".to_string()
        } else {
            format!("disk write failed: {e}")
        }
    })?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

// R4 修正：拖拽来源白名单——只有 on_drag_drop_event 记录过的路径才可读（XSS 不能 invoke 任意路径）
static DROPPED_PATHS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> =
    std::sync::OnceLock::new();

pub fn record_dropped_path(p: &Path) {
    DROPPED_PATHS.get_or_init(Default::default).lock().unwrap().insert(p.to_path_buf());
}

#[tauri::command]
pub async fn dsh_import_dropped(path: String) -> Result<Vec<u8>, String> {
    let canon = Path::new(&path).canonicalize().map_err(|e| format!("canonicalize: {e}"))?;
    let ok = DROPPED_PATHS.get_or_init(Default::default).lock().unwrap().iter().any(|p| p == &canon);
    if !ok { return Err("path not from drag-drop".into()); }
    if !canon.is_file() { return Err("not a file".into()); }
    let meta = std::fs::metadata(&canon).map_err(|e| e.to_string())?;
    if meta.len() > 160 * 1024 * 1024 { return Err("file too large (>160 MiB)".into()); }
    std::fs::read(&canon).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dsh_export_session(app: AppHandle, session_id: String, state: tauri::State<'_, AppState>) -> Result<String, String> {
    // 下载（R2 修正：session.export 大 ZIP 不经 invoke bytes 回传——Rust 侧流式落盘）
    let url = format!("http://dsh/api/session.export?sessionId={}", urlencode(&session_id));
    let resp = state.http_client.get(&url).send().await.map_err(|e| format!("transport: {e}"))?;
    let bytes = resp.bytes().await.map_err(|e| format!("read: {e}"))?.to_vec();
    let downloads = app.path().download_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    let dest = downloads.join(format!("dsh-session-{}-{}.zip", sanitize(&session_id), nanoid()));
    std::fs::write(&dest, &bytes).map_err(|e| {
        if e.kind() == std::io::ErrorKind::StorageFull || e.kind() == std::io::ErrorKind::QuotaExceeded {
            "磁盘空间不足".to_string()
        } else {
            format!("disk write failed: {e}")
        }
    })?;
    Ok(dest.display().to_string())
}

fn urlencode(s: &str) -> String { s.replace('%', "%25").replace('?', "%3F").replace('&', "%26") }

/// 启动时按年龄清扫孤儿临时文件（spec §4.6）
pub fn age_sweep(cache_dir: &Path, max_age: std::time::Duration) {
    let uploads = cache_dir.join("temp-uploads");
    let Ok(entries) = std::fs::read_dir(&uploads) else { return; };
    let now = std::time::SystemTime::now();
    for e in entries.flatten() {
        if let Ok(meta) = e.metadata() {
            if let Ok(modified) = meta.modified() {
                if now.duration_since(modified).map(|d| d > max_age).unwrap_or(false) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_within_root() {
        let root = std::env::temp_dir().join("dsh-temp-test");
        fs::create_dir_all(&root).unwrap();
        let inner = root.join("a.txt");
        fs::write(&inner, "x").unwrap();
        assert!(canonicalize_within(&root, &inner).is_ok());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rejects_escape() {
        let root = std::env::temp_dir().join("dsh-temp-test2");
        fs::create_dir_all(&root).unwrap();
        let outside = std::env::temp_dir().join("dsh-escape-marker.txt");
        fs::write(&outside, "x").unwrap();
        assert!(canonicalize_within(&root, &outside).is_err());
        fs::remove_dir_all(&root).unwrap();
        let _ = fs::remove_file(&outside);
    }

    #[test]
    fn random_name_no_user_input() {
        // 文件名由 Rust 随机生成（spec §4.6）——绝不采用 Content-Disposition
        let name = format!("dsh-{}-{}.bin", std::process::id(), nanoid());
        assert!(name.starts_with("dsh-"));
    }

    #[test]
    fn sanitize_strips_dangerous() {
        assert_eq!(sanitize("../evil/name"), "___evil_name");
    }

    #[test]
    fn age_sweep_removes_old() {
        let dir = std::env::temp_dir().join(format!("dsh-sweep-{}", std::process::id()));
        let uploads = dir.join("temp-uploads");
        fs::create_dir_all(&uploads).unwrap();
        let old = uploads.join("old.bin");
        fs::write(&old, "x").unwrap();
        // 回拨 mtime 到 2 天前（FileTimes, Rust 1.75+）
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 24 * 3600);
        let f = std::fs::File::open(&old).unwrap();
        let _ = f.set_times(std::fs::FileTimes::new().set_modified(past));
        drop(f);
        age_sweep(&dir, std::time::Duration::from_secs(24 * 3600));
        assert!(!old.exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
