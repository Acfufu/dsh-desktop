import { describe, it, expect } from 'vitest';
import { buildManifest, ManifestEntry } from './generate-manifest';

const sample: ManifestEntry[] = [
  { id: 'connection', file: '/fake/lib/client.js', rev: 'abc123' },
  { id: 'locale', file: '/fake/lib/client.js', rev: 'def456' },
];

describe('buildManifest', () => {
  it('produces schema { rev, entries: [{id,url,rev}] }', () => {
    const m = buildManifest(sample);
    expect(typeof m.rev).toBe('string');
    expect(m.rev.length).toBe(12);
    expect(m.entries.length).toBe(2);
    expect(m.entries[0]).toEqual({ id: 'connection', url: '/plugins/connection/client.js?rev=abc123', rev: 'abc123' });
  });

  it('rev is stable for identical content (sha1-12)', () => {
    const a = buildManifest([{ id: 'x', file: '/fake/lib/client.js', rev: 'deadbeef00aa' }]);
    const b = buildManifest([{ id: 'x', file: '/fake/lib/client.js', rev: 'deadbeef00aa' }]);
    expect(a.rev).toBe(b.rev);
  });

  it('rev changes when entry revs change', () => {
    const a = buildManifest([{ id: 'x', file: '/f', rev: 'aaaa00000001' }]);
    const b = buildManifest([{ id: 'x', file: '/f', rev: 'aaaa00000002' }]);
    expect(a.rev).not.toBe(b.rev);
  });
});
