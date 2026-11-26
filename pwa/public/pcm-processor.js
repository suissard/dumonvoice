// AudioWorklet : rééchantillonnage linéaire vers 16 kHz, conversion Float32 → Int16 LE,
// paquets de 640 échantillons (40 ms = 1280 octets) + niveau RMS pour le vumètre.

export class Pcm16Resampler {
  constructor(inRate, outRate = 16000, frame = 640) {
    this.step = inRate / outRate;
    this.t = 0;
    this.prev = null;
    this.frame = frame;
    this.buf = new Int16Array(frame);
    this.len = 0;
    this.sumSq = 0;
  }

  /** Traite un bloc Float32 ; renvoie les paquets complets { buffer, rms }. */
  push(input) {
    const out = [];
    const hasPrev = this.prev !== null;
    const len = input.length + (hasPrev ? 1 : 0);
    const get = (i) => (hasPrev ? (i === 0 ? this.prev : input[i - 1]) : input[i]);
    if (len < 2) {
      if (input.length) this.prev = input[input.length - 1];
      return out;
    }
    let t = this.t;
    while (t + 1 < len) {
      const i = Math.floor(t);
      const frac = t - i;
      const a = get(i);
      const b = get(i + 1);
      let s = a + (b - a) * frac;
      if (s > 1) s = 1;
      else if (s < -1) s = -1;
      this.sumSq += s * s;
      this.buf[this.len++] = Math.round(s * 32767);
      if (this.len === this.frame) {
        const rms = Math.sqrt(this.sumSq / this.frame);
        // Copie Little-Endian explicite (indépendante de l'endianness de la plateforme).
        const ab = new ArrayBuffer(this.frame * 2);
        const dv = new DataView(ab);
        for (let k = 0; k < this.frame; k++) dv.setInt16(k * 2, this.buf[k], true);
        out.push({ buffer: ab, rms });
        this.len = 0;
        this.sumSq = 0;
      }
      t += this.step;
    }
    this.t = t - (len - 1);
    this.prev = input[input.length - 1];
    return out;
  }

  reset() {
    this.t = 0;
    this.prev = null;
    this.len = 0;
    this.sumSq = 0;
  }
}

if (typeof AudioWorkletProcessor !== 'undefined' && typeof registerProcessor === 'function') {
  class PcmProcessor extends AudioWorkletProcessor {
    constructor() {
      super();
      // `sampleRate` est une globale du scope AudioWorklet (souvent 48000 Hz sur iOS).
      this.resampler = new Pcm16Resampler(sampleRate);
      this.port.onmessage = (e) => {
        if (e.data === 'reset') this.resampler.reset();
      };
    }

    process(inputs) {
      const ch = inputs[0] && inputs[0][0];
      if (ch && ch.length) {
        for (const pkt of this.resampler.push(ch)) {
          this.port.postMessage(pkt, [pkt.buffer]);
        }
      }
      return true;
    }
  }
  registerProcessor('pcm-processor', PcmProcessor);
}
