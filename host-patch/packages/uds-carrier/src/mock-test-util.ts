import { EventEmitter } from 'node:events';

// 最小 ApiProxy 桩：满足 bridge/toFetchHandler 消费面的形状
export function makeMockApiProxy(handler: (method: string, args: unknown) => Promise<unknown>) {
  return {
    call: async (method: string, args: unknown) => handler(method, args),
  };
}

// 构造一个可消费的 IncomingMessage 桩（http.request 层测试用）
export function makeMockServer(handler: (req: unknown, res: unknown) => void) {
  const server = new EventEmitter() as any;
  server.listen = (path: string) => {
    server.listeningPath = path;
    server.emit('listening');
    return server;
  };
  server.close = (cb?: () => void) => { cb?.(); return server; };
  server.closeAllConnections = () => {};
  server.address = () => ({ path: server.listeningPath });
  return server;
}
