// UDS 路径选择（Rust 侧共享同一逻辑，spec §4.1）。
// sockaddr_un.sun_path 上限 104；保守阈值 100。目录一律 0700 且属当前 uid。

const MAX_PATH_BYTES = 100;

function usable(path: string): boolean {
  return Buffer.byteLength(path, 'utf8') <= MAX_PATH_BYTES;
}

export function selectSocketPath(
  dshHome: string | undefined,
  uid: number,
  osTmp: string,
): string {
  if (dshHome && usable(`${dshHome}/run/dsh.sock`)) {
    return `${dshHome}/run/dsh.sock`;
  }
  if (usable(`${osTmp}/dsh-${uid}/dsh.sock`)) {
    return `${osTmp}/dsh-${uid}/dsh.sock`;
  }
  return `/tmp/dsh-${uid}/dsh.sock`;
}
