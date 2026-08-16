import { describe, it, expect } from 'vitest';
import { notificationFromRequest } from './notify';

describe('notificationFromRequest', () => {
  it('fires on turn/end', () => {
    const r = { type: 'server-request', rpcId: 'r1', method: 'turn/end', payload: {} } as any; // test-only cast
    expect(notificationFromRequest(r)?.title).toBe('Agent 完成');
  });

  it('fires on session-status question', () => {
    const r = { type: 'server-request', rpcId: 'r2', method: 'session-status', payload: { status: 'question' } } as any; // test-only cast
    expect(notificationFromRequest(r)?.title).toBe('需要输入');
  });

  it('ignores unrelated methods', () => {
    const r = { type: 'server-request', rpcId: 'r3', method: 'session/list', payload: {} } as any; // test-only cast
    expect(notificationFromRequest(r)).toBeNull();
  });
});
