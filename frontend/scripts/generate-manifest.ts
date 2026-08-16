// M3 Task 5 以真实实现替换（derive-composed-entries → manifest）。Task 1 先置桩保证 vite 配置可加载。
import type { Plugin } from 'vite';

export interface ManifestEntry {
  id: string;
  file: string;
  rev: string;
}

export function buildManifest(_entries: ManifestEntry[]): { rev: number; entries: ManifestEntry[] } {
  return { rev: 0, entries: [] };
}

export function deriveComposedEntries(): { id: string; file: string; rev: string }[] {
  return [];
}
