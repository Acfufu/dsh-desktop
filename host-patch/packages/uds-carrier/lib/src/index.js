import { Service } from '@deepseek-ai/cordis';
import { createServer as httpCreateServer } from 'node:http';
import { connect as netConnect } from 'node:net';
import * as fs from 'node:fs';
import * as os from 'node:os';
import { selectSocketPath } from './socket-path.js';
import { bridge, DEFAULT_MAX_REQUEST_BODY_BYTES } from '../vendor/http-bridge.js';
import { WebSocketDownlinks } from '../vendor/websocket-downlink.js';
import { toFetchHandler } from '@deepseek-ai/dsh-host-apiproxy';
export const inject = ['apiProxy'];
const MUX_PATH = '/api/events.mux';
const HOST_PATH = '/api/events.host';
export class UdsCarrierService extends Service {
    static inject = ['apiProxy'];
    server;
    downlinks;
    socketPath;
    config = {}; // R2 修正：显式字段
    constructor(ctx, config = {}) {
        super(ctx, 'udsCarrier');
        this.config = config; // R2 修正：存 config，否则 this.config?.maxBodyBytes 恒为 undefined
        this.socketPath =
            config.udsPath ??
                selectSocketPath(process.env.DSH_HOME, process.getuid?.() ?? 0, os.tmpdir());
    }
    getSocketPath() {
        return this.socketPath;
    }
    async start() {
        const apiProxy = this.ctx.get('apiProxy'); // inject 保证存在（api-gateway 行提供）
        const maxBody = this.config?.maxBodyBytes ?? DEFAULT_MAX_REQUEST_BODY_BYTES;
        const socketPath = this.socketPath;
        // 目录 0700（防其他 uid 替换 socket 文件 → bind 劫持）
        // DeepSec L3：recursive mkdir 跟随预置 symlink、无 owner 校验——攻击者可预置共享路径为
        // 指向己方目录的 symlink，使 socket 落在攻击者目录（chmod 只改 mode 不改 owner）→ 其他 uid MITM。
        // 修正：mkdir 后 lstat（不跟随）验证非 symlink 且 st_uid == 当前 uid。
        const dir = socketPath.slice(0, socketPath.lastIndexOf('/'));
        fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
        const st = fs.lstatSync(dir); // lstat 不跟随 symlink
        if (st.isSymbolicLink() || st.uid !== process.getuid?.()) {
            throw new Error(`socket dir unsafe (symlink or wrong owner): ${dir}`);
        }
        fs.chmodSync(dir, 0o700);
        // 残留 socket 清理：listen 前 connect 探测，无活服务则 unlink（spec §4.1）
        if (fs.existsSync(socketPath)) {
            const alive = await this.probeAlive(socketPath);
            if (!alive)
                fs.unlinkSync(socketPath);
        }
        const fetchHandler = toFetchHandler(apiProxy);
        this.downlinks = new WebSocketDownlinks(apiProxy);
        this.server = httpCreateServer((req, res) => {
            void bridge(req, res, fetchHandler, maxBody);
        });
        this.server.on('upgrade', (req, socket, head) => {
            // DeepSec L3：new URL 对畸形 req.url 抛异常会崩溃 carrier——try/catch 后直接拒绝
            let pathname;
            try {
                pathname = new URL(req.url ?? '/', 'http://dsh').pathname;
            }
            catch {
                socket.destroy();
                return;
            }
            if (pathname === MUX_PATH) {
                this.downlinks.handleMux(req, socket, head);
            }
            else if (pathname === HOST_PATH) {
                this.downlinks.handleHost(req, socket, head);
            }
            else {
                socket.destroy();
            }
        });
        await new Promise((resolve, reject) => {
            this.server.once('error', reject);
            this.server.listen(socketPath, () => resolve());
        });
        fs.chmodSync(socketPath, 0o600);
        this.ctx.logger.info(`uds-carrier listening on ${socketPath} (0600)`);
    }
    probeAlive(socketPath) {
        return new Promise((resolve) => {
            const c = netConnect(socketPath);
            c.once('connect', () => { c.destroy(); resolve(true); });
            c.once('error', () => resolve(false));
            c.setTimeout(1000, () => { c.destroy(); resolve(false); });
        });
    }
    async stop() {
        this.downlinks?.close();
        this.server?.closeAllConnections();
        await new Promise((resolve) => this.server?.close(() => resolve()) ?? resolve());
        try {
            fs.unlinkSync(this.socketPath);
        }
        catch { /* 幂等 */ }
        this.ctx.logger.info('uds-carrier stopped, socket cleaned');
    }
}
export function apply(ctx, config = {}) {
    const svc = new UdsCarrierService(ctx, config);
    ctx.provide('udsCarrier', svc);
    ctx.on('dispose', () => { void svc.stop(); });
    // R2 修正：捕获 start() 拒绝，避免 unhandled rejection
    void svc.start().catch((e) => ctx.logger.error(`uds-carrier start failed: ${e}`));
    return svc;
}
