import { Context, Service } from '@deepseek-ai/cordis';
import * as os from 'node:os';
import { selectSocketPath } from './socket-path';

declare module '@deepseek-ai/cordis' {
  interface Context {
    apiProxy: any; // ApiProxy 由 api-gateway 行提供（@deepseek-ai/dsh-host-apiproxy）
  }
}

export const inject = ['apiProxy'];

export class UdsCarrierService extends Service {
  static inject = ['apiProxy'];
  private socketPath: string;

  constructor(ctx: Context, config: { udsPath?: string }) {
    super(ctx, 'udsCarrier');
    this.socketPath =
      config.udsPath ??
      selectSocketPath(process.env.DSH_HOME, process.getuid?.() ?? 0, os.tmpdir());
    ctx.logger.info(`uds-carrier socket path: ${this.socketPath}`);
  }

  getSocketPath(): string {
    return this.socketPath;
  }
}

export function apply(ctx: Context, config: { udsPath?: string } = {}) {
  const svc = new UdsCarrierService(ctx, config);
  ctx.provide('udsCarrier', svc);
  return svc;
}
