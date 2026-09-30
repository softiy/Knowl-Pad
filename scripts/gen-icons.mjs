import { writeFile } from 'node:fs/promises';
import { deflateSync } from 'node:zlib';

const SIZE = 32;
function crc32(buf) {
  let c = ~0;
  for (const byte of buf) {
    c ^= byte;
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return (~c) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
  const t = Buffer.from(type, 'ascii');
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(Buffer.concat([t, data])));
  return Buffer.concat([len, t, data, crc]);
}
const raw = [];
for (let y = 0; y < SIZE; y++) {
  raw.push(Buffer.from([0]));
  for (let x = 0; x < SIZE; x++) {
    const edge = x < 2 || y < 2 || x >= SIZE - 2 || y >= SIZE - 2;
    raw.push(Buffer.from(edge ? [0x1f, 0x6f, 0xeb, 255] : [0x0b, 0x12, 0x20, 255]));
  }
}
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0); ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk('IHDR', ihdr),
  chunk('IDAT', deflateSync(Buffer.concat(raw))),
  chunk('IEND', Buffer.alloc(0)),
]);
await writeFile('src-tauri/icons/icon.png', png);

const dir = Buffer.alloc(6);
dir.writeUInt16LE(0, 0); dir.writeUInt16LE(1, 2); dir.writeUInt16LE(1, 4);
const entry = Buffer.alloc(16);
entry[0] = SIZE; entry[1] = SIZE; entry[2] = 0; entry[3] = 0;
entry.writeUInt16LE(1, 4); entry.writeUInt16LE(32, 6);
entry.writeUInt32LE(png.length, 8); entry.writeUInt32LE(22, 12);
await writeFile('src-tauri/icons/icon.ico', Buffer.concat([dir, entry, png]));
console.log('icons written: icon.png ' + png.length + ' bytes, icon.ico ' + (22 + png.length) + ' bytes');
