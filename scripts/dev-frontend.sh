#!/usr/bin/env bash
# tauri beforeDevCommand 载体：cwd 在 CLI 间漂移（src-tauri / resources/dsh）——绝对路径自定位
cd "$(dirname "$0")/../frontend" && exec pnpm dev
