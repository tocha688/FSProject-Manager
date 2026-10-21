// 生成应用图标：纯 Node 实现（zlib 打包 PNG + ico 容器），无外部依赖。
import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "src-tauri", "icons");
mkdirSync(outDir, { recursive: true });

const CRC_TABLE = (() => {
  const t = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body), 0);
  return Buffer.concat([len, body, crc]);
}

function renderPng(size) {
  const raw = Buffer.alloc(size * (1 + size * 4));
  const cx = size * 0.42, cy = size * 0.42, r = size * 0.22, ring = size * 0.055;
  const hx0 = size * 0.585, hy0 = size * 0.585, hx1 = size * 0.8, hy1 = size * 0.8, hw = size * 0.055;
  const distSeg = (px, py, ax, ay, bx, by) => {
    const dx = bx - ax, dy = by - ay;
    const t = Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)));
    const sx = ax + t * dx, sy = ay + t * dy;
    return Math.hypot(px - sx, py - sy);
  };
  let p = 0;
  for (let y = 0; y < size; y++) {
    raw[p++] = 0; // filter: none
    for (let x = 0; x < size; x++) {
      const t = (x + y) / (2 * size);
      let r8 = Math.round(59 + (139 - 59) * t);   // #3b82f6 -> #8b5cf6
      let g8 = Math.round(130 + (92 - 130) * t);
      let b8 = Math.round(246 + (245 - 246) * t);
      const d = Math.hypot(x - cx, y - cy);
      const isRing = Math.abs(d - r) < ring;
      const isHandle = distSeg(x + 0.5, y + 0.5, hx0, hy0, hx1, hy1) < hw;
      if (isRing || isHandle) { r8 = 255; g8 = 255; b8 = 255; }
      raw[p++] = r8; raw[p++] = g8; raw[p++] = b8; raw[p++] = 255;
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8;  // bit depth
  ihdr[9] = 6;  // RGBA
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function wrapIco(png) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); header.writeUInt16LE(1, 2); header.writeUInt16LE(1, 4);
  const entry = Buffer.alloc(16);
  entry[0] = 0; entry[1] = 0; // 256 -> 0
  entry.writeUInt16LE(1, 4);  // planes
  entry.writeUInt16LE(32, 6); // bpp
  entry.writeUInt32LE(png.length, 8);
  entry.writeUInt32LE(22, 12);
  return Buffer.concat([header, entry, png]);
}

for (const [name, size] of [["32x32.png", 32], ["128x128.png", 128], ["128x128@2x.png", 256]]) {
  writeFileSync(join(outDir, name), renderPng(size));
}
writeFileSync(join(outDir, "icon.ico"), wrapIco(renderPng(256)));
console.log("icons written to", outDir);
