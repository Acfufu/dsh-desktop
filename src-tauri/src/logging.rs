use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const MAX_LOG_BYTES: u64 = 1024 * 1024; // 1 MiB
const ROTATIONS: usize = 3;

/// sidecar stdout/stderr → ~/Library/Logs/dsh-desktop/sidecar.log（1 MiB × 3 轮转）
/// DeepSec L3：日志文件 0600（OpenOptions create+append 默认 0644，其他用户可读 sidecar 输出）
pub fn init_sidecar_log(logs_dir: &Path) -> Result<File, String> {
    std::fs::create_dir_all(logs_dir).map_err(|e| e.to_string())?;
    let path = logs_dir.join("sidecar.log");
    rotate_if_needed(&path);
    let f = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| e.to_string())?;
    Ok(f)
}

fn rotate_if_needed(path: &Path) {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() >= MAX_LOG_BYTES {
            for i in (1..ROTATIONS).rev() {
                let _ = std::fs::rename(
                    path.with_file_name(format!("sidecar.log.{i}")),
                    path.with_file_name(format!("sidecar.log.{}", i + 1)),
                );
            }
            let _ = std::fs::rename(path, path.with_file_name("sidecar.log.1"));
        }
    }
}

/// panic hook：写 ~/Library/Logs/dsh-desktop/dsh-desktop-panic.log
pub fn setup_panic_hook(logs_dir: &Path) {
    let dir = logs_dir.to_path_buf();
    std::panic::set_hook(Box::new(move |info| {
        let msg = format!("panic: {info}");
        let _ = std::fs::write(dir.join("dsh-desktop-panic.log"), &msg);
        eprintln!("{msg}");
    }));
}

/// 读日志尾部 N 行（诊断对话框；from_utf8_lossy 解码，spec §4.2）
pub fn tail_lines(path: &Path, n: usize) -> String {
    match std::fs::read(path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            text.lines().rev().take(n).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
        }
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_log_file() {
        let dir = std::env::temp_dir().join(format!("dsh-log-{}", std::process::id()));
        let f = init_sidecar_log(&dir).expect("init");
        assert!(dir.join("sidecar.log").exists());
        drop(f);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tail_reads_last_lines_lossy() {
        let dir = std::env::temp_dir().join(format!("dsh-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("sidecar.log");
        std::fs::write(&p, "line1\nline2\nline3\n").unwrap();
        assert_eq!(tail_lines(&p, 2), "line2\nline3");
        assert_eq!(tail_lines(&p, 0), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotation_renames() {
        let dir = std::env::temp_dir().join(format!("dsh-rot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("sidecar.log");
        // 写满 1 MiB 触发轮转
        std::fs::write(&p, vec![b'x'; (MAX_LOG_BYTES + 10) as usize]).unwrap();
        let f = init_sidecar_log(&dir).expect("init after full");
        assert!(dir.join("sidecar.log.1").exists());
        assert!(dir.join("sidecar.log").exists());
        drop(f);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
