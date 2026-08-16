// 通知（spec §4.3）：订阅 agent 完成/提问事件。
// 事件源 = fork 内 TauriApiClient 的 onEnvelope tap（facts §2 实证逐帧调用）。
import type { ServerRequest } from '@deepseek-ai/dsh-host-apiproxy/api';

export interface EnvelopeSource {
  subscribe(fn: (req: ServerRequest) => void): () => void;
}

export interface NotificationEvent {
  title: string;
  body: string;
}

// 纯函数：从 ServerRequest 判断是否触发通知（可测）
export function notificationFromRequest(req: ServerRequest): NotificationEvent | null {
  if (req.type !== 'server-request') return null;
  if (req.method === 'turn/end') {
    return { title: 'Agent 完成', body: '回合已结束' };
  }
  if (req.method === 'session-status' && (req.payload as { status?: string })?.status === 'question') {
    return { title: '需要输入', body: 'Agent 正在等你回应' };
  }
  return null;
}

export async function startNotifications(source: EnvelopeSource): Promise<() => void> {
  const { isPermissionGranted, requestPermission, sendNotification } = await import('@tauri-apps/plugin-notification');
  let granted = await isPermissionGranted();
  if (!granted) {
    granted = (await requestPermission()) === 'granted';
  }
  if (!granted) return () => {};

  return source.subscribe((req) => {
    const n = notificationFromRequest(req);
    if (n) sendNotification({ title: n.title, body: n.body });
  });
}
