import { describe, it, expect, vi, beforeEach } from 'vitest';
import { TauriApiClient, TransportError } from './tauri-api-client';

describe('TransportError classification', () => {
  it('invoke reject with kind → transport error', () => {
    expect(new TransportError('transport', 'x').kind).toBe('transport');
  });

  it('plain error → transport error', () => {
    const e = new TransportError('transport', 'boom');
    expect(e.kind).toBe('transport');
    expect(e instanceof Error).toBe(true);
  });

  it('business kind carried on TransportError', () => {
    expect(new TransportError('business', 'x').kind).toBe('business');
  });
});
