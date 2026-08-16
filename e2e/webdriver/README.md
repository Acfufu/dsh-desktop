# Optional WebDriver layer (nightly / manual)

tauri-driver (`cargo install tauri-driver`) provides WebDriver for Tauri v2,
but macOS WKWebView WebDriver support is weak and version-sensitive (spec §7:
labeled nightly/manual, NOT in blocking CI).

```bash
cargo install tauri-driver
# then run the app and drive it with any WebDriver client against the
# tauri-driver endpoint (default port 4444)
```

The blocking CI layer is `scripts/e2e-smoke.sh` (zero WebDriver).
