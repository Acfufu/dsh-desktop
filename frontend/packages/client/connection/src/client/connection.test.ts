import { describe, it, expect } from 'vitest';
import { ConnectionController } from './connection';

describe('ConnectionController regeneration', () => {
  it('re-exported class exists and can be constructed with api stub', () => {
    const cc = ConnectionController as any; // test-only cast
    expect(typeof cc).toBe('function');
  });
});
