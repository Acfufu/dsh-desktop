import { describe, it, expect } from 'vitest';
import { selectSocketPath } from './socket-path.js';

describe('selectSocketPath', () => {
  it('prefers $DSH_HOME/run/dsh.sock when short enough', () => {
    expect(selectSocketPath('/Users/me/.dsh', 501, '/var/folders/x')).toBe('/Users/me/.dsh/run/dsh.sock');
  });

  it('falls back to os.tmpdir when $DSH_HOME path exceeds 100 bytes', () => {
    const longHome = '/Users/' + 'x'.repeat(120) + '/.deepseek-harness';
    // longHome/run/dsh.sock > 100 bytes → next candidate
    const tmp = selectSocketPath(longHome, 501, '/var/folders/ab/xyz');
    expect(tmp).toBe('/var/folders/ab/xyz/dsh-501/dsh.sock');
  });

  it('falls back to /tmp/dsh-<uid> when os.tmpdir path also exceeds 100 bytes', () => {
    const longTmp = '/tmp/' + 'y'.repeat(150);
    expect(selectSocketPath(undefined, 501, longTmp)).toBe('/tmp/dsh-501/dsh.sock');
  });

  it('always ends with dsh.sock', () => {
    expect(selectSocketPath(undefined, 501, '/tmp')).toBe('/tmp/dsh-501/dsh.sock');
  });
});
