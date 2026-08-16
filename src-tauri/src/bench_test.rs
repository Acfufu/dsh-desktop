#[cfg(test)]
mod bench {
    use super::*;
    use crate::http_command::{dsh_http_impl, UDS_PATH};
    use crate::AppState;
    use std::process::{Child, Command};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    struct Sidecar(Child);
    impl Sidecar {
        fn start(sock: &str) -> Sidecar {
            let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/bench-big-body.mjs");
            let child = Command::new("node").arg(script)
                .env("DSH_SOCKET", sock)
                .spawn().expect("spawn bench sidecar");
            std::thread::sleep(Duration::from_millis(800));
            Sidecar(child)
        }
    }
    impl Drop for Sidecar { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }

    #[tokio::test]
    async fn bench_150mib_through_pipe() {
        let sock = format!("/tmp/dsh-uds-test/bench-{}.sock", std::process::id());
        let _sc = Sidecar::start(&sock);
        let client = reqwest::ClientBuilder::new().unix_socket(sock.as_str()).build().unwrap();
        let state = crate::AppState {
            http_client: client,
            uds_path: sock,
            registry: Arc::new(Mutex::new(crate::streams::StreamRegistry::new())),
        };
        let t0 = Instant::now();
        let resp = dsh_http_impl(state, "GET".into(), "/api/big".into(), None).await.expect("fetch 150MiB");
        let elapsed = t0.elapsed();
        assert_eq!(resp.body.len(), 150 * 1024 * 1024);
        println!("bench: 150 MiB through dsh_http_impl in {elapsed:?}");
    }
}
