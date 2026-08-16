// mock @tauri-apps/api/core：invoke + Channel
import { vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => {
  class MockChannel<T> {
    private handler: ((msg: T) => void) | null = null;
    set onmessage(fn: (msg: T) => void) { this.handler = fn; }
    get onmessage() { return this.handler!; }
    send(msg: T) { this.handler?.(msg); }
  }
  return {
    invoke: vi.fn(async () => { throw new Error('invoke not mocked in this test'); }),
    Channel: MockChannel,
  };
});
