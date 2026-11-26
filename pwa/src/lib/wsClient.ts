// Client WebSocket : reconnexion automatique (backoff exponentiel), heartbeat 5 s,
// suivi des ACK (`injection_result` / `error` par identifiant).
import type { InjectionResult, ServerEvent, ErrorEvent } from './types';

export type ConnState = 'connecting' | 'open' | 'closed' | 'unauthorized';

export interface WsOptions {
  url?: string;
  /** Jeton d'appareil (mobile) ; vide pour la PWA Desktop (cookie HttpOnly). */
  token?: () => string | null;
  clientKind: 'mobile' | 'desktop';
  WebSocketImpl?: typeof WebSocket;
  heartbeatMs?: number;
  ackTimeoutMs?: number;
}

interface Pending {
  resolve: (r: InjectionResult) => void;
  reject: (e: AckError) => void;
  timer: ReturnType<typeof setTimeout>;
}

export class AckError extends Error {
  code: string;
  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

export function defaultWsUrl(): string {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${proto}//${location.host}/ws`;
}

export class WsClient {
  state: ConnState = 'closed';
  pcName = '';
  private ws: WebSocket | null = null;
  private listeners = new Set<(e: ServerEvent) => void>();
  private stateListeners = new Set<(s: ConnState) => void>();
  private pending = new Map<string, Pending>();
  private backoff = 500;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private heartbeat: ReturnType<typeof setInterval> | null = null;
  private lastMessage = 0;
  private stopped = false;
  private opts: Required<Omit<WsOptions, 'token' | 'url'>> & Pick<WsOptions, 'token' | 'url'>;

  constructor(opts: WsOptions) {
    this.opts = {
      heartbeatMs: 5000,
      ackTimeoutMs: 10000,
      WebSocketImpl: opts.WebSocketImpl ?? WebSocket,
      ...opts,
    };
  }

  onEvent(fn: (e: ServerEvent) => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  onState(fn: (s: ConnState) => void): () => void {
    this.stateListeners.add(fn);
    fn(this.state);
    return () => this.stateListeners.delete(fn);
  }

  private setState(s: ConnState) {
    this.state = s;
    for (const f of this.stateListeners) f(s);
  }

  get isOpen(): boolean {
    return this.state === 'open';
  }

  connect() {
    this.stopped = false;
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.ws && (this.ws.readyState === 0 || this.ws.readyState === 1)) return;
    this.setState('connecting');
    const ws = new this.opts.WebSocketImpl(this.opts.url ?? defaultWsUrl());
    ws.binaryType = 'arraybuffer';
    this.ws = ws;
    ws.onopen = () => {
      this.lastMessage = Date.now();
      const token = this.opts.token?.() ?? '';
      ws.send(JSON.stringify({ type: 'auth', token, client_kind: this.opts.clientKind }));
      this.startHeartbeat();
    };
    ws.onmessage = (m: MessageEvent) => {
      this.lastMessage = Date.now();
      if (typeof m.data !== 'string') return;
      let ev: ServerEvent;
      try {
        ev = JSON.parse(m.data);
      } catch {
        return;
      }
      this.handle(ev);
    };
    ws.onclose = () => {
      this.stopHeartbeat();
      if (this.ws === ws) this.ws = null;
      if (this.state !== 'unauthorized') this.setState('closed');
      this.scheduleReconnect();
    };
    ws.onerror = () => {
      try {
        ws.close();
      } catch {
        /* déjà fermé */
      }
    };
  }

  private handle(ev: ServerEvent) {
    if (ev.type === 'auth_ok') {
      this.pcName = ev.pc_name;
      this.backoff = 500;
      this.setState('open');
    } else if (ev.type === 'injection_result') {
      const p = this.pending.get(ev.id);
      if (p) {
        clearTimeout(p.timer);
        this.pending.delete(ev.id);
        p.resolve(ev);
      }
    } else if (ev.type === 'error') {
      if (ev.code === 'UNAUTHORIZED' && this.opts.clientKind === 'mobile') {
        this.setState('unauthorized');
        this.stopped = true;
      }
      if (ev.id) {
        const p = this.pending.get(ev.id);
        if (p) {
          clearTimeout(p.timer);
          this.pending.delete(ev.id);
          p.reject(new AckError(ev.code, ev.message));
        }
      }
    }
    for (const f of this.listeners) f(ev);
  }

  private startHeartbeat() {
    this.stopHeartbeat();
    this.heartbeat = setInterval(() => {
      if (!this.ws) return;
      // Aucun message depuis 2,5 périodes : connexion morte (Wi-Fi coupé).
      if (Date.now() - this.lastMessage > this.opts.heartbeatMs * 2.5) {
        try {
          this.ws.close();
        } catch {
          /* ignore */
        }
        return;
      }
      this.send({ type: 'ping', ts: Date.now() });
    }, this.opts.heartbeatMs);
  }

  private stopHeartbeat() {
    if (this.heartbeat) clearInterval(this.heartbeat);
    this.heartbeat = null;
  }

  private scheduleReconnect() {
    if (this.stopped || this.reconnectTimer) return;
    const delay = Math.min(this.backoff, 10000) * (0.8 + Math.random() * 0.4);
    this.backoff = Math.min(this.backoff * 2, 10000);
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, delay);
  }

  /** Reconnexion immédiate (bouton « Reconnecter », retour au premier plan). */
  reconnectNow() {
    this.backoff = 500;
    if (this.state === 'unauthorized') return;
    if (this.ws && this.ws.readyState === 1) return;
    if (this.ws) {
      try {
        this.ws.close();
      } catch {
        /* ignore */
      }
      this.ws = null;
    }
    this.connect();
  }

  close() {
    this.stopped = true;
    this.stopHeartbeat();
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
    this.ws?.close();
    this.ws = null;
    this.setState('closed');
  }

  send(obj: unknown): boolean {
    if (!this.ws || this.ws.readyState !== 1) return false;
    this.ws.send(JSON.stringify(obj));
    return true;
  }

  sendBinary(buf: ArrayBuffer): boolean {
    if (!this.ws || this.ws.readyState !== 1 || this.state !== 'open') return false;
    this.ws.send(buf);
    return true;
  }

  /**
   * Envoie un message identifié et attend l'ACK (`injection_result`) ou l'erreur
   * correspondante. Rejette si la connexion n'est pas ouverte ou après expiration.
   */
  request(id: string, msg: object): Promise<InjectionResult> {
    return new Promise((resolve, reject) => {
      if (this.state !== 'open' || !this.send(msg)) {
        reject(new AckError('OFFLINE', 'Non connecté au PC'));
        return;
      }
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new AckError('TIMEOUT', 'Pas de réponse du PC'));
      }, this.opts.ackTimeoutMs);
      this.pending.set(id, { resolve, reject, timer });
    });
  }
}

export function isErrorEvent(e: ServerEvent): e is ErrorEvent {
  return e.type === 'error';
}
