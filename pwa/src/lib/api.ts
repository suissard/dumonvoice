// Appels REST typés vers le daemon.
import type {
  Config,
  DeepPartial,
  HistoryEntry,
  InjectionResult,
  InjectTextMessage,
  PairedDevice,
  QuickAction,
  Status,
} from './types';

const TOKEN_KEY = 'dv_device_token';
const DEVICE_KEY = 'dv_device_id';

export function getDeviceToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

export function setDeviceToken(token: string | null, deviceId?: string) {
  try {
    if (token) {
      localStorage.setItem(TOKEN_KEY, token);
      if (deviceId) localStorage.setItem(DEVICE_KEY, deviceId);
    } else {
      localStorage.removeItem(TOKEN_KEY);
      localStorage.removeItem(DEVICE_KEY);
    }
  } catch {
    /* stockage indisponible */
  }
}

export class ApiError extends Error {
  status: number;
  code: string;
  constructor(status: number, code: string, message: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

async function req<T>(method: string, path: string, body?: unknown, raw?: BodyInit): Promise<T> {
  const headers: Record<string, string> = {};
  const token = getDeviceToken();
  if (token && location.protocol === 'https:') headers['Authorization'] = `Bearer ${token}`;
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const res = await fetch(path, {
    method,
    headers,
    credentials: 'same-origin',
    body: raw ?? (body !== undefined ? JSON.stringify(body) : undefined),
  });
  if (res.status === 204) return undefined as T;
  let data: any = null;
  try {
    data = await res.json();
  } catch {
    /* corps vide */
  }
  if (!res.ok) {
    throw new ApiError(res.status, data?.code ?? 'HTTP_' + res.status, data?.message ?? res.statusText);
  }
  return data as T;
}

export const api = {
  status: (mics = false) => req<Status>('GET', `/api/status${mics ? '?mics=1' : ''}`),
  config: () => req<Config>('GET', '/api/config'),
  patchConfig: (patch: DeepPartial<Config> | Record<string, unknown>) => req<Config>('PATCH', '/api/config', patch),
  pairingCreate: () =>
    req<{ pair_code: string; url: string; expires_in: number; qr_svg: string; alternatives: string[] }>(
      'POST',
      '/api/pairing/create',
    ),
  pairingExchange: (pair_code: string, device_name: string) =>
    req<{ device_id: string; device_token: string; pc_name: string }>('POST', '/api/pairing/exchange', {
      pair_code,
      device_name,
    }),
  devices: () => req<PairedDevice[]>('GET', '/api/pairing/devices'),
  revoke: (id: string) => req<void>('DELETE', `/api/pairing/devices/${encodeURIComponent(id)}`),
  injectText: (msg: Omit<InjectTextMessage, 'type'>) => req<InjectionResult>('POST', '/api/remote/inject-text', msg),
  action: (action: QuickAction) => req<void>('POST', '/api/remote/action', { action }),
  ollamaModels: () =>
    req<{ reachable: boolean; models: { name: string; size: number; audio_capable: boolean }[]; error?: string }>(
      'GET',
      '/api/engines/ollama/models',
    ),
  download: (kind: 'whisper' | 'ollama' | 'vad' | 'wakeword', model: string) =>
    req<{ started: boolean }>('POST', '/api/engines/download', { kind, model }),
  history: () => req<HistoryEntry[]>('GET', '/api/history'),
  hotkeyCapture: () => req<{ capturing: boolean }>('POST', '/api/hotkey/capture'),
  importWakeword: (name: string, file: Blob) =>
    req<{ name: string }>('POST', `/api/wakewords/import?name=${encodeURIComponent(name)}`, undefined, file),
};

/** Nom d'appareil lisible pour l'appairage. */
export function guessDeviceName(ua = navigator.userAgent): string {
  if (/iPhone/.test(ua)) return 'iPhone';
  if (/iPad/.test(ua)) return 'iPad';
  const m = ua.match(/Android [\d.]+; ([^;)]+)/);
  if (m) return m[1].replace(/Build.*/, '').trim() || 'Android';
  if (/Android/.test(ua)) return 'Android';
  return 'Navigateur';
}
