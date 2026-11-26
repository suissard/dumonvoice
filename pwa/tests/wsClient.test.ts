import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AckError, WsClient } from '../src/lib/wsClient';

class FakeWs {
  static last: FakeWs;
  readyState = 0;
  binaryType = '';
  sent: unknown[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((m: { data: unknown }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(public url: string) {
    FakeWs.last = this;
  }
  send(d: unknown) {
    this.sent.push(typeof d === 'string' ? JSON.parse(d) : d);
  }
  close() {
    this.readyState = 3;
    this.onclose?.();
  }
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
  recv(obj: unknown) {
    this.onmessage?.({ data: JSON.stringify(obj) });
  }
}

function client(kind: 'mobile' | 'desktop' = 'mobile') {
  return new WsClient({
    url: 'wss://pc/ws',
    clientKind: kind,
    token: () => 'TOKEN',
    WebSocketImpl: FakeWs as unknown as typeof WebSocket,
    heartbeatMs: 1000,
    ackTimeoutMs: 2000,
  });
}

describe('WsClient', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('authentifie avec le jeton puis passe en état open', () => {
    const c = client();
    c.connect();
    const ws = FakeWs.last;
    ws.open();
    expect(ws.sent[0]).toEqual({ type: 'auth', token: 'TOKEN', client_kind: 'mobile' });
    expect(c.state).toBe('connecting');
    ws.recv({ type: 'auth_ok', pc_name: 'PC-BUREAU', state: 'IDLE', device_id: 'dev_1' });
    expect(c.state).toBe('open');
    expect(c.pcName).toBe('PC-BUREAU');
  });

  it('résout la requête sur injection_result (ACK)', async () => {
    const c = client();
    c.connect();
    FakeWs.last.open();
    FakeWs.last.recv({ type: 'auth_ok', pc_name: 'PC', state: 'IDLE', device_id: null });
    const p = c.request('m1', { type: 'inject_text', message_id: 'm1', text: 'x' });
    FakeWs.last.recv({ type: 'injection_result', id: 'm1', injected: true, text: 'x', source: 'remote_text', latency_ms: 12 });
    await expect(p).resolves.toMatchObject({ id: 'm1', injected: true });
  });

  it('rejette sur erreur BUSY avec le même id (brouillon conservé côté UI)', async () => {
    const c = client();
    c.connect();
    FakeWs.last.open();
    FakeWs.last.recv({ type: 'auth_ok', pc_name: 'PC', state: 'IDLE', device_id: null });
    const p = c.request('m2', { type: 'inject_text', message_id: 'm2' });
    FakeWs.last.recv({ type: 'error', id: 'm2', code: 'BUSY', message: 'occupé' });
    await expect(p).rejects.toMatchObject({ code: 'BUSY' });
  });

  it('rejette immédiatement hors connexion, et après expiration', async () => {
    const c = client();
    await expect(c.request('m3', {})).rejects.toBeInstanceOf(AckError);
    c.connect();
    FakeWs.last.open();
    FakeWs.last.recv({ type: 'auth_ok', pc_name: 'PC', state: 'IDLE', device_id: null });
    const p = c.request('m4', {});
    vi.advanceTimersByTime(2100);
    await expect(p).rejects.toMatchObject({ code: 'TIMEOUT' });
  });

  it('reconnecte avec backoff après une coupure', () => {
    const c = client();
    c.connect();
    const first = FakeWs.last;
    first.open();
    first.recv({ type: 'auth_ok', pc_name: 'PC', state: 'IDLE', device_id: null });
    first.close();
    expect(c.state).toBe('closed');
    vi.advanceTimersByTime(700);
    expect(FakeWs.last).not.toBe(first);
    expect(c.state).toBe('connecting');
  });

  it('heartbeat : ping périodique puis fermeture si plus aucun message', () => {
    const c = client();
    c.connect();
    const ws = FakeWs.last;
    ws.open();
    ws.recv({ type: 'auth_ok', pc_name: 'PC', state: 'IDLE', device_id: null });
    vi.advanceTimersByTime(1000);
    expect(ws.sent.some((m: any) => m.type === 'ping')).toBe(true);
    vi.advanceTimersByTime(3000);
    expect(ws.readyState).toBe(3);
  });

  it('cesse de reconnecter si l’appareil est révoqué', () => {
    const c = client();
    c.connect();
    const ws = FakeWs.last;
    ws.open();
    ws.recv({ type: 'error', id: null, code: 'UNAUTHORIZED', message: 'non' });
    expect(c.state).toBe('unauthorized');
    ws.close();
    vi.advanceTimersByTime(20000);
    expect(FakeWs.last).toBe(ws);
  });

  it('n’envoie pas d’audio binaire avant authentification', () => {
    const c = client();
    c.connect();
    FakeWs.last.open();
    expect(c.sendBinary(new ArrayBuffer(4))).toBe(false);
    FakeWs.last.recv({ type: 'auth_ok', pc_name: 'PC', state: 'IDLE', device_id: null });
    expect(c.sendBinary(new ArrayBuffer(4))).toBe(true);
  });
});
