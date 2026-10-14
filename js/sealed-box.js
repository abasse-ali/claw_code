// Chiffrement d'un secret d'Actions pour GitHub : « sealed box » de libsodium
// (crypto_box_seal), faite avec la boîte NaCl de tweetnacl (vendor/nacl-fast.min.js,
// chargée avant l'appli) et le nonce BLAKE2b(clé éphémère ‖ clé du destinataire).
// Le secret ne quitte le navigateur que chiffré, pour la clé publique du dépôt.

import { blake2b } from "./blake2b.js";

const nacl = () => {
  if (!globalThis.nacl || !globalThis.nacl.box) throw new Error("bibliothèque de chiffrement absente");
  return globalThis.nacl;
};

export function fromBase64(text) {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

export function toBase64(bytes) {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

function nonceFor(ephemeral, recipient) {
  const input = new Uint8Array(64);
  input.set(ephemeral, 0);
  input.set(recipient, 32);
  return blake2b(input, 24);
}

/** `message` (texte) chiffré pour la clé publique `publicKey` (base 64), en base 64. */
export function sealedBox(message, publicKey) {
  const box = nacl();
  const recipient = fromBase64(publicKey);
  if (recipient.length !== 32) throw new Error("clé publique du dépôt invalide");
  const ephemeral = box.box.keyPair();
  const boxed = box.box(new TextEncoder().encode(message), nonceFor(ephemeral.publicKey, recipient),
    recipient, ephemeral.secretKey);
  ephemeral.secretKey.fill(0);
  const sealed = new Uint8Array(32 + boxed.length);
  sealed.set(ephemeral.publicKey, 0);
  sealed.set(boxed, 32);
  return toBase64(sealed);
}

/** Ouverture (essais seulement : GitHub, lui, a la clé secrète). */
export function openSealedBox(sealedBase64, publicKey, secretKey) {
  const box = nacl();
  const sealed = fromBase64(sealedBase64);
  const ephemeral = sealed.subarray(0, 32);
  const opened = box.box.open(sealed.subarray(32), nonceFor(ephemeral, publicKey), ephemeral, secretKey);
  return opened ? new TextDecoder().decode(opened) : null;
}
