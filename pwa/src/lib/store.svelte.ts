// État applicatif réactif partagé (Svelte 5 runes).
import { api, getDeviceToken } from './api';
import { WsClient, type ConnState } from './wsClient';
import type { Config, DeepPartial, InjectionResult, ServerEvent, Status } from './types';

export interface Toast {
  id: number;
  text: string;
  level: 'info' | 'warn' | 'error' | 'success';
}

export interface DownloadState {
  downloaded: number;
  total: number | null;
  done: boolean;
  error: string | null;
}

const ERROR_TEXT: Record<string, string> = {
  BUSY: 'Une dictée est déjà en cours sur le PC.',
  PAUSED: 'DumonVoice est en pause sur le PC.',
  OLLAMA_UNREACHABLE: 'Ollama est injoignable.',
  TIMEOUT: 'Délai dépassé.',
};

export function errorText(code: string, fallback?: string): string {
  return ERROR_TEXT[code] ?? fallback ?? code;
}

export const isLan = typeof location !== 'undefined' && location.protocol === 'https:';

class AppStore {
  status = $state<Status | null>(null);
  config = $state<Config | null>(null);
  conn = $state<ConnState>('closed');
  pcName = $state('');
  level = $state(0);
  lastResult = $state<InjectionResult | null>(null);
  toasts = $state<Toast[]>([]);
  downloads = $state<Record<string, DownloadState>>({});
  hotkeyCaptured = $state<{ keys: string[]; valid: boolean; message: string | null } | null>(null);
  modelLoading = $state(false);
  devicesVersion = $state(0);
  ws: WsClient | null = null;
  private toastId = 0;
  private statusTimer: ReturnType<typeof setTimeout> | null = null;

  init(kind: 'desktop' | 'mobile') {
    if (this.ws) return;
    this.ws = new WsClient({ clientKind: kind, token: () => (kind === 'mobile' ? getDeviceToken() : '') });
    this.ws.onState((s) => {
      this.conn = s;
      if (s === 'open') this.pcName = this.ws!.pcName;
    });
    this.ws.onEvent((e) => this.onEvent(e));
    this.ws.connect();
  }

  private onEvent(e: ServerEvent) {
    switch (e.type) {
      case 'state_changed':
        if (this.status) {
          this.status.state = e.state;
          this.status.source = e.source;
          this.status.target_window = e.target_window;
          this.status.enabled = e.state !== 'PAUSED';
        }
        if (e.state !== 'TRANSCRIBING') this.modelLoading = false;
        break;
      case 'audio_level':
        this.level = e.rms;
        break;
      case 'injection_result':
        this.lastResult = e;
        if (!e.injected && e.injection_note) {
          this.toast(
            e.injection_note === 'ADMIN_WINDOW_CLIPBOARD_ONLY'
              ? 'Fenêtre Admin détectée : texte copié, faites Ctrl+V'
              : 'Fenêtre changée : texte copié dans le presse-papiers du PC',
            'warn',
          );
        }
        break;
      case 'notice':
        if (e.code === 'model_loading') this.modelLoading = true;
        else this.toast(e.message, e.level === 'error' ? 'error' : e.level === 'warn' ? 'warn' : 'info');
        break;
      case 'download_progress':
        this.downloads[e.model] = { downloaded: e.downloaded, total: e.total, done: e.done, error: e.error };
        if (e.done) {
          if (e.error) this.toast(`Téléchargement échoué : ${e.error}`, 'error');
          this.scheduleStatus();
        }
        break;
      case 'hotkey_captured':
        this.hotkeyCaptured = { keys: e.keys, valid: e.valid, message: e.message };
        break;
      case 'config_changed':
        this.refreshConfig();
        this.scheduleStatus();
        break;
      case 'devices_changed':
        this.devicesVersion++;
        this.scheduleStatus();
        break;
    }
  }

  private scheduleStatus() {
    if (this.statusTimer) return;
    this.statusTimer = setTimeout(() => {
      this.statusTimer = null;
      this.refreshStatus();
    }, 300);
  }

  async refreshStatus(mics = false) {
    try {
      const s = await api.status(mics);
      if (!mics && this.status?.microphone.devices) s.microphone.devices = this.status.microphone.devices;
      this.status = s;
      this.pcName = s.pc_name;
    } catch {
      /* hors ligne */
    }
  }

  async refreshConfig() {
    try {
      this.config = await api.config();
    } catch {
      /* hors ligne */
    }
  }

  async refresh() {
    await Promise.all([this.refreshStatus(), this.refreshConfig()]);
  }

  /** Mise à jour partielle de la configuration (rechargée à chaud par le daemon). */
  async patch(p: DeepPartial<Config> | Record<string, unknown>): Promise<boolean> {
    try {
      this.config = await api.patchConfig(p);
      this.scheduleStatus();
      return true;
    } catch (e: any) {
      this.toast(e?.message ?? 'Réglage refusé', 'error');
      await this.refreshConfig();
      return false;
    }
  }

  toast(text: string, level: Toast['level'] = 'info') {
    const id = ++this.toastId;
    this.toasts.push({ id, text, level });
    setTimeout(() => {
      this.toasts = this.toasts.filter((t) => t.id !== id);
    }, 4000);
  }
}

export const store = new AppStore();
