// Les codes QR : en montrer un (un lien, un IBAN, un billet, le partage d'une liste) et en
// lire un avec l'appareil photo ou depuis une photo. Les bibliothèques (qrcode, jsQR) ne sont
// téléchargées qu'au premier usage.

import { Camera, Download, ImageUp, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Icon } from "./icons";

/** Un code QR pour `value` (un lien, un texte), net à toute taille, avec « Enregistrer
 * l'image ». Noir sur blanc même en mode sombre : les lecteurs le lisent mieux. */
export function QRCode({ value, label, size = 200, download = true }: { value: string; label: string; size?: number; download?: boolean }) {
  const [svg, setSvg] = useState("");
  const [error, setError] = useState("");
  useEffect(() => {
    let alive = true;
    if (!value) { setSvg(""); return; }
    import("qrcode")
      .then((qr) => qr.toString(value, { type: "svg", margin: 2, errorCorrectionLevel: "M", color: { dark: "#000000", light: "#ffffff" } }))
      .then((text) => { if (alive) { setSvg(text); setError(""); } })
      .catch(() => { if (alive) setError("Ce texte est trop long pour un code QR."); });
    return () => { alive = false; };
  }, [value]);
  const save = async () => {
    const qr = await import("qrcode");
    const url = await qr.toDataURL(value, { width: 1024, margin: 2 });
    const link = document.createElement("a");
    link.href = url;
    link.download = `${label.replace(/[^\p{L}\p{N}]+/gu, "-").toLowerCase() || "code"}-qr.png`;
    link.click();
  };
  if (error) return <p className="qr__error" role="alert">{error}</p>;
  return (
    <figure className="qr">
      <div className="qr__code" style={{ width: size, height: size }} role="img" aria-label={`Code QR : ${label}`}
        dangerouslySetInnerHTML={svg ? { __html: svg } : undefined} />
      <figcaption>{label}</figcaption>
      {download && svg && <button className="btn btn--small" type="button" onClick={save}><Icon icon={Download} size={16} />Enregistrer l'image</button>}
    </figure>
  );
}

/** Lit l'image d'un code QR (un canvas ou une image) ; rend son texte, ou null. */
async function decode(image: CanvasImageSource, width: number, height: number): Promise<string | null> {
  const canvas = document.createElement("canvas");
  const scale = Math.min(1, 1024 / Math.max(width, height));
  canvas.width = Math.round(width * scale);
  canvas.height = Math.round(height * scale);
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context) return null;
  context.drawImage(image, 0, 0, canvas.width, canvas.height);
  const pixels = context.getImageData(0, 0, canvas.width, canvas.height);
  const { default: jsQR } = await import("jsqr");
  return jsQR(pixels.data, pixels.width, pixels.height, { inversionAttempts: "attemptBoth" })?.data ?? null;
}

/** Lire un code QR : « Scanner » ouvre l'appareil photo (dos du téléphone) et rend le texte
 * lu à `onScan` ; « Depuis une photo » lit une image déjà prise (utile quand l'appareil photo
 * est refusé). */
export function QRScanner({ onScan, label = "Scanner un code QR" }: { onScan: (text: string) => void; label?: string }) {
  const video = useRef<HTMLVideoElement>(null);
  const stream = useRef<MediaStream | null>(null);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState("");
  const stop = () => { stream.current?.getTracks().forEach((track) => track.stop()); stream.current = null; setOpen(false); };
  useEffect(() => () => stream.current?.getTracks().forEach((track) => track.stop()), []);

  useEffect(() => {
    if (!open) return;
    // La vidéo existe une fois l'écran de l'appareil photo affiché : le flux s'y branche ici.
    if (video.current && stream.current && video.current.srcObject !== stream.current) {
      video.current.srcObject = stream.current;
      video.current.play().catch(() => {});
    }
    let frame = 0;
    let busy = false;
    let alive = true;
    const look = async () => {
      if (!alive) return;
      const player = video.current;
      if (player && player.readyState >= 2 && !busy) {
        busy = true;
        const text = await decode(player, player.videoWidth, player.videoHeight).catch(() => null);
        busy = false;
        if (text && alive) { stop(); onScan(text); return; }
      }
      frame = requestAnimationFrame(look);
    };
    frame = requestAnimationFrame(look);
    return () => { alive = false; cancelAnimationFrame(frame); };
    // onScan peut changer à chaque rendu : la lecture continue sans redémarrer.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const start = async () => {
    setError("");
    if (!navigator.mediaDevices?.getUserMedia) { setError("L'appareil photo n'est pas disponible ici : utilisez « Depuis une photo »."); return; }
    try {
      stream.current = await navigator.mediaDevices.getUserMedia({ video: { facingMode: "environment" }, audio: false });
      setOpen(true);
    } catch (failure) {
      setError(failure instanceof DOMException && failure.name === "NotAllowedError"
        ? "L'appareil photo est refusé : autorisez-le dans les réglages, ou utilisez « Depuis une photo »."
        : "L'appareil photo ne s'ouvre pas : utilisez « Depuis une photo ».");
    }
  };

  const fromPhoto = async (file: File | undefined) => {
    if (!file) return;
    setError("");
    try {
      const bitmap = await createImageBitmap(file);
      const text = await decode(bitmap, bitmap.width, bitmap.height);
      if (text) onScan(text); else setError("Aucun code QR lisible sur cette photo.");
    } catch {
      setError("Cette image ne s'ouvre pas.");
    }
  };

  return (
    <div className="qr-scan">
      {open ? (
        <div className="qr-scan__view">
          <video ref={video} className="qr-scan__video" playsInline muted aria-label="Image de l'appareil photo" />
          <span className="qr-scan__frame" aria-hidden="true" />
          <p className="qr-scan__hint" role="status">Placez le code QR dans le cadre</p>
          <button className="icon-btn qr-scan__close" type="button" aria-label="Fermer l'appareil photo" onClick={stop}><Icon icon={X} /></button>
        </div>
      ) : (
        <div className="btn-row">
          <button className="btn btn--primary" type="button" onClick={start}><Icon icon={Camera} size={18} />{label}</button>
          <label className="btn qr-scan__file">
            <Icon icon={ImageUp} size={18} />Depuis une photo
            <input type="file" accept="image/*" className="visually-hidden" onChange={(event) => { fromPhoto(event.target.files?.[0]); event.target.value = ""; }} />
          </label>
        </div>
      )}
      {error && <p className="qr-scan__error" role="alert">{error}</p>}
    </div>
  );
}
