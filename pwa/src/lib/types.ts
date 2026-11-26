// Contrat de messages partagé avec le daemon (section 6.4 du cahier des charges).

export type FsmState = 'PAUSED' | 'IDLE' | 'LISTENING' | 'TRANSCRIBING' | 'INJECTING' | 'ERROR';
export type SourceKind = 'local_mic' | 'remote_audio' | 'remote_text';
export type ErrorCode =
  | 'BUSY'
  | 'TEXT_TOO_LARGE'
  | 'OLLAMA_UNREACHABLE'
  | 'ADMIN_WINDOW_FALLBACK'
  | 'EMPTY_AUDIO'
  | 'TIMEOUT'
  | 'PAUSED'
  | 'DIRECT_TEXT_DISABLED'
  | 'UNAUTHORIZED'
  | 'BAD_REQUEST'
  | 'ENGINE_ERROR'
  | 'NO_MICROPHONE'
  | 'INJECTION_FAILED';

export interface InjectionResult {
  type: 'injection_result';
  id: string;
  source: SourceKind;
  text: string;
  injected: boolean;
  injection_note: string | null;
  target_window: string | null;
  latency_ms: number;
}

export interface ErrorEvent {
  type: 'error';
  id: string | null;
  code: ErrorCode;
  message: string;
}

export type ServerEvent =
  | { type: 'auth_ok'; pc_name: string; state: FsmState; device_id: string | null }
  | {
      type: 'state_changed';
      state: FsmState;
      source: SourceKind | null;
      session_id: string | null;
      target_window: string | null;
      error_code?: ErrorCode;
      detail?: string;
    }
  | { type: 'audio_level'; rms: number }
  | InjectionResult
  | ErrorEvent
  | { type: 'pong'; ts: number }
  | { type: 'notice'; level: string; code: string; message: string }
  | {
      type: 'download_progress';
      model: string;
      downloaded: number;
      total: number | null;
      done: boolean;
      error: string | null;
    }
  | { type: 'hotkey_captured'; keys: string[]; valid: boolean; message: string | null }
  | { type: 'config_changed' }
  | { type: 'devices_changed' };

export interface InjectTextMessage {
  type: 'inject_text';
  message_id: string;
  text: string;
  press_enter_after: boolean;
  refine_with_ollama: boolean;
}

export type QuickAction = 'enter' | 'undo_last' | 'backspace';

export interface Status {
  pc_name: string;
  version: string;
  enabled: boolean;
  state: FsmState;
  source: SourceKind | null;
  target_window: string | null;
  error_code: ErrorCode | null;
  onboarding_done: boolean;
  engine: {
    active: 'whisper' | 'ollama';
    whisper: {
      compiled: boolean;
      model_file: string;
      available: boolean;
      installed: string[];
      catalog: { profile: string; file: string; size_mb: number }[];
    };
    ollama: { reachable: boolean; version: string | null; base_url: string; model: string };
  };
  microphone: { current: string | null; devices: string[] | null };
  vad: string;
  wake_word: { enabled: boolean; compiled: boolean; words: { name: string; builtin: boolean; installed: boolean }[] };
  mobile: {
    enabled: boolean;
    running: boolean;
    lan_url: string | null;
    lan_ips: string[];
    port: number;
    mdns: string;
    connected_devices: string[];
    paired_count: number;
  };
  gpu: string | null;
  hotkey: string[];
  client: { scope: 'local' | 'lan'; device_id: string | null };
}

export interface PairedDevice {
  id: string;
  name: string;
  paired_at: string;
  connected: boolean;
}

export interface HistoryEntry {
  id: string;
  source: SourceKind;
  text: string;
  injected: boolean;
  target_window: string | null;
  at: string;
}

// Configuration (schéma 6.6) — typage volontairement souple.
export interface Config {
  version: number;
  enabled: boolean;
  launch_at_startup: boolean;
  onboarding_done: boolean;
  language: string;
  triggers: {
    hotkey: { enabled: boolean; keys: string[]; mode: 'push_to_talk' | 'toggle'; toggle_auto_stop: boolean };
    wake_word: {
      enabled: boolean;
      model: string;
      sensitivity: number;
      silence_timeout_ms: number;
      pause_during_calls: boolean;
    };
  };
  remote_mobile: {
    enabled: boolean;
    allow_direct_text: boolean;
    max_text_length: number;
    lan_port: number;
    mdns_name: string;
    paired_devices: { id: string; name: string; paired_at: string }[];
  };
  engine: {
    active: 'whisper' | 'ollama';
    post_process_with_ollama: boolean;
    whisper: { profile: 'fast' | 'balanced' | 'accurate'; model_file: string; use_gpu: boolean };
    ollama: {
      base_url: string;
      model: string;
      text_model: string;
      api_style: 'native' | 'openai';
      keep_alive: string;
      clean_dictation: boolean;
      custom_system_prompt: string | null;
      fallback_to_whisper: boolean;
    };
  };
  audio: { pc_device_id: string; duck_other_apps: boolean; duck_level: number; sound_feedback: boolean };
  injection: {
    method: 'clipboard_paste' | 'send_input';
    restore_clipboard: boolean;
    paste_delay_ms: number;
    check_window_focus: boolean;
    custom_dictionary: Record<string, string>;
  };
  ui: { overlay_position: string; overlay_enabled: boolean; keep_history_count: number };
  server: { local_port: number };
}

export type DeepPartial<T> = { [K in keyof T]?: T[K] extends object ? DeepPartial<T[K]> : T[K] };
