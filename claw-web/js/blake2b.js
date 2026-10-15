// BLAKE2b (RFC 7693), sans clé, longueur de sortie au choix (1 à 64 octets).
// Sert à dériver le nonce d'une « sealed box » libsodium, le format que GitHub
// exige pour les secrets d'Actions. Entiers de 64 bits en BigInt : lent, mais
// les entrées font quelques dizaines d'octets.

const MASK = (1n << 64n) - 1n;

const IV = [
  0x6a09e667f3bcc908n, 0xbb67ae8584caa73bn, 0x3c6ef372fe94f82bn, 0xa54ff53a5f1d36f1n,
  0x510e527fade682d1n, 0x9b05688c2b3e6c1fn, 0x1f83d9abfb41bd6bn, 0x5be0cd19137e2179n,
];

const SIGMA = [
  [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
  [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
  [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
  [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
  [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
  [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
  [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
  [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
  [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
  [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

const rotr = (value, bits) => ((value >> BigInt(bits)) | (value << BigInt(64 - bits))) & MASK;

function compress(h, block, counter, last) {
  const m = [];
  for (let i = 0; i < 16; i += 1) {
    let word = 0n;
    for (let j = 7; j >= 0; j -= 1) word = (word << 8n) | BigInt(block[i * 8 + j]);
    m.push(word);
  }
  const v = [...h, ...IV];
  v[12] ^= counter & MASK;
  v[13] ^= (counter >> 64n) & MASK;
  if (last) v[14] ^= MASK;
  const mix = (a, b, c, d, x, y) => {
    v[a] = (v[a] + v[b] + x) & MASK;
    v[d] = rotr(v[d] ^ v[a], 32);
    v[c] = (v[c] + v[d]) & MASK;
    v[b] = rotr(v[b] ^ v[c], 24);
    v[a] = (v[a] + v[b] + y) & MASK;
    v[d] = rotr(v[d] ^ v[a], 16);
    v[c] = (v[c] + v[d]) & MASK;
    v[b] = rotr(v[b] ^ v[c], 63);
  };
  for (let round = 0; round < 12; round += 1) {
    const s = SIGMA[round % 10];
    mix(0, 4, 8, 12, m[s[0]], m[s[1]]);
    mix(1, 5, 9, 13, m[s[2]], m[s[3]]);
    mix(2, 6, 10, 14, m[s[4]], m[s[5]]);
    mix(3, 7, 11, 15, m[s[6]], m[s[7]]);
    mix(0, 5, 10, 15, m[s[8]], m[s[9]]);
    mix(1, 6, 11, 12, m[s[10]], m[s[11]]);
    mix(2, 7, 8, 13, m[s[12]], m[s[13]]);
    mix(3, 4, 9, 14, m[s[14]], m[s[15]]);
  }
  for (let i = 0; i < 8; i += 1) h[i] ^= v[i] ^ v[i + 8];
}

/** Empreinte BLAKE2b de `input` (Uint8Array), sur `length` octets. */
export function blake2b(input, length = 64) {
  if (length < 1 || length > 64) throw new RangeError("longueur de BLAKE2b hors de 1 à 64");
  const h = [...IV];
  h[0] ^= 0x01010000n ^ BigInt(length);
  const block = new Uint8Array(128);
  let counter = 0n;
  let offset = 0;
  // Tous les blocs pleins sauf le dernier (le dernier, même plein, est compressé à part).
  while (input.length - offset > 128) {
    block.set(input.subarray(offset, offset + 128));
    counter += 128n;
    compress(h, block, counter, false);
    offset += 128;
  }
  block.fill(0);
  block.set(input.subarray(offset));
  counter += BigInt(input.length - offset);
  compress(h, block, counter, true);
  const out = new Uint8Array(length);
  for (let i = 0; i < length; i += 1) out[i] = Number((h[i >> 3] >> BigInt(8 * (i & 7))) & 0xffn);
  return out;
}
