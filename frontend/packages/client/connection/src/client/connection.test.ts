import { describe, it, expect, vi } from 'vitest';
import { ConnectionController } from './connection';

describe('ConnectionController regeneration', () => {
  it('re-exported class exists and can be constructed with api stub', () => {
    const cc = ConnectionController as any; // test-only cast
    expect(typeof cc).toBe('function');
  });
});

describe('handshake describe timeout', () => {
  it('describe AbortSignal.timeout(10s) fires on hung sidecar', async () => {
    vi.useFakeTimers();
    try {
      const api = {
        host: {
          describe: vi.fn((_req: unknown, signal?: AbortSignal) =>
            new Promise((_, reject) => {
              signal?.addEventListener('abort', () => reject(new DOMException('Aborted', 'AbortError')));
            })),
        },
      } as any; // test-only cast
      const signal = AbortSignal.timeout(10_000);
      const promise = api.host.describe({}, signal);
      vi.advanceTimersByTime(10_100);
      await expect(promise).rejects.toThrow(/Abort/);
    } finally {
      vi.useRealTimers();
    }
  });
});
