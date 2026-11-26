// Génère les icônes PNG (192 / 512) de la PWA sans dépendance externe.
import { deflateSync } from 'node:zlib';
import { writeFileSync } from 'node:fs';

function crc32(buf) {
  let c, crc = 0xffffffff;
  for (let n = 0; n < buf.length; n++) {
    c = (crc ^ buf[n]) & 0xff;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    crc = (crc >>> 8) ^ c;
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}

function icon(size) {
  const s = size / 512;
  const raw = Buffer.alloc((size * 4 + 1) * size);
  const inRoundRect = (x, y, x0, y0, w, h, r) => {
    const cx = Math.min(Math.max(x, x0 + r), x0 + w - r);
    const cy = Math.min(Math.max(y, y0 + r), y0 + h - r);
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r && x >= x0 && x <= x0 + w && y >= y0 && y <= y0 + h;
  };
  for (let py = 0; py < size; py++) {
    raw[py * (size * 4 + 1)] = 0;
    for (let px = 0; px < size; px++) {
      const x = px / s, y = py / s;
      let rgba = [0, 0, 0, 0];
      if (inRoundRect(x, y, 0, 0, 512, 512, 112)) rgba = [37, 99, 235, 255];
      const capsule = inRoundRect(x, y, 206, 104, 100, 190, 50);
      const d = Math.hypot(x - 256, y - 250);
      const arc = y >= 250 && d >= 96 && d <= 124;
      const stem = Math.abs(x - 256) <= 14 && y >= 360 && y <= 410;
      const base = Math.abs(x - 256) <= 74 && Math.abs(y - 410) <= 14;
      if (rgba[3] && (capsule || arc || stem || base)) rgba = [255, 255, 255, 255];
      raw.set(rgba, py * (size * 4 + 1) + 1 + px * 4);
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

for (const n of [192, 512]) writeFileSync(new URL(`../public/icon-${n}.png`, import.meta.url), icon(n));
console.log('icônes générées');
