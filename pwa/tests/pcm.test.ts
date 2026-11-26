import { describe, expect, it } from 'vitest';
import { Pcm16Resampler } from '../public/pcm-processor.js';

describe('Pcm16Resampler (AudioWorklet)', () => {
  it('48 kHz → 16 kHz : paquets de 640 échantillons (1280 octets)', () => {
    const r = new Pcm16Resampler(48000);
    const packets: { buffer: ArrayBuffer; rms: number }[] = [];
    // 1 seconde de sinus en blocs de 128 (taille de rendu AudioWorklet)
    for (let b = 0; b < 375; b++) {
      const block = new Float32Array(128);
      for (let i = 0; i < 128; i++) block[i] = 0.5 * Math.sin(((b * 128 + i) * 2 * Math.PI * 440) / 48000);
      packets.push(...r.push(block));
    }
    // 16000 échantillons ≈ 25 paquets/s
    expect(packets.length).toBe(25);
    for (const p of packets) expect(p.buffer.byteLength).toBe(1280);
    // RMS d'un sinus d'amplitude 0,5 ≈ 0,354
    expect(packets[5].rms).toBeGreaterThan(0.3);
    expect(packets[5].rms).toBeLessThan(0.4);
  });

  it('44,1 kHz : nombre d’échantillons cohérent', () => {
    const r = new Pcm16Resampler(44100, 16000, 160);
    let n = 0;
    for (let b = 0; b < 100; b++) n += r.push(new Float32Array(441)).length * 160;
    expect(Math.abs(n - 16000)).toBeLessThanOrEqual(160);
  });

  it('Int16 Little-Endian et saturation', () => {
    const r = new Pcm16Resampler(16000, 16000, 4);
    const out = r.push(new Float32Array([1.5, -2, 0.5, 0, 0]));
    expect(out.length).toBe(1);
    const dv = new DataView(out[0].buffer);
    expect(dv.getInt16(0, true)).toBe(32767);
    expect(dv.getInt16(2, true)).toBe(-32767);
    expect(dv.getInt16(4, true)).toBe(16384);
  });
});
