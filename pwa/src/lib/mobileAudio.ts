// Capture micro mobile (getUserMedia → AudioWorklet 16 kHz S16LE), Screen Wake Lock, haptique.

export type ChunkHandler = (pcm: ArrayBuffer, rms: number) => void;

export class MobileAudio {
  private ctx: AudioContext | null = null;
  private node: AudioWorkletNode | null = null;
  private source: MediaStreamAudioSourceNode | null = null;
  private stream: MediaStream | null = null;
  private ready: Promise<void> | null = null;
  private active = false;
  onChunk: ChunkHandler = () => {};
  onLevel: (rms: number) => void = () => {};

  get supported(): boolean {
    return typeof navigator.mediaDevices?.getUserMedia === 'function' && window.isSecureContext && 'AudioWorkletNode' in window;
  }

  /**
   * À appeler STRICTEMENT dans le gestionnaire `pointerdown`/`click` (iOS Safari
   * suspend un AudioContext créé ou repris hors geste utilisateur).
   */
  prime(): Promise<void> {
    if (!this.ctx) {
      const AC: typeof AudioContext = (window as any).AudioContext || (window as any).webkitAudioContext;
      this.ctx = new AC();
    }
    // Reprise synchrone dans le geste.
    const resume = this.ctx.state === 'suspended' ? this.ctx.resume() : Promise.resolve();
    if (!this.ready) {
      const ctx = this.ctx;
      this.ready = (async () => {
        await resume;
        await ctx.audioWorklet.addModule('/pcm-processor.js');
        this.stream = await navigator.mediaDevices.getUserMedia({
          audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
          video: false,
        });
        this.source = ctx.createMediaStreamSource(this.stream);
        this.node = new AudioWorkletNode(ctx, 'pcm-processor', { numberOfInputs: 1, numberOfOutputs: 0 });
        this.node.port.onmessage = (e: MessageEvent<{ buffer: ArrayBuffer; rms: number }>) => {
          this.onLevel(e.data.rms);
          if (this.active) this.onChunk(e.data.buffer, e.data.rms);
        };
        this.source.connect(this.node);
      })().catch((e) => {
        this.ready = null;
        throw e;
      });
    } else {
      this.ready = this.ready.then(() => resume);
    }
    return this.ready;
  }

  start() {
    this.node?.port.postMessage('reset');
    this.active = true;
  }

  stop() {
    this.active = false;
  }

  /** Libère le micro (page en arrière-plan) : l'indicateur micro du téléphone s'éteint. */
  async release() {
    this.active = false;
    this.stream?.getTracks().forEach((t) => t.stop());
    this.source?.disconnect();
    this.node?.disconnect();
    this.stream = null;
    this.source = null;
    this.node = null;
    this.ready = null;
    if (this.ctx) {
      try {
        await this.ctx.close();
      } catch {
        /* ignore */
      }
    }
    this.ctx = null;
  }
}

export function haptic(pattern: number | number[]) {
  try {
    navigator.vibrate?.(pattern);
  } catch {
    /* non supporté (iOS) */
  }
}

/** Garde l'écran allumé tant que la page est au premier plan. */
export class ScreenWakeLock {
  private lock: any = null;
  private wanted = false;
  private onVis = () => {
    if (document.visibilityState === 'visible' && this.wanted) this.acquire();
  };

  async enable() {
    this.wanted = true;
    document.addEventListener('visibilitychange', this.onVis);
    await this.acquire();
  }

  private async acquire() {
    try {
      const wl = (navigator as any).wakeLock;
      if (!wl || this.lock) return;
      this.lock = await wl.request('screen');
      this.lock.addEventListener?.('release', () => {
        this.lock = null;
      });
    } catch {
      this.lock = null;
    }
  }

  disable() {
    this.wanted = false;
    document.removeEventListener('visibilitychange', this.onVis);
    this.lock?.release?.();
    this.lock = null;
  }
}
