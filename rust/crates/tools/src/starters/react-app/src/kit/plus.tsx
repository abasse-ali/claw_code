// Encore des widgets : panneau latéral, bulle d'options, clavier de montant, heure, tableau de
// tâches en colonnes, agenda, jauge, objectif d'épargne, tirer pour rafraîchir, « Voir plus »,
// pages, fil d'Ariane, visionneuse de photos, signature au doigt, bandeau hors ligne, écran de
// secours quand une partie plante, cloche à pastille ; et les branchements : une API (Express,
// FastAPI) avec useApi, le temps réel (Socket.IO) avec useLive, un module Rust compilé en
// WebAssembly avec useWasm.

import { AnimatePresence, motion, useMotionValue, useReducedMotion, useTransform, animate } from "framer-motion";
import { ArrowLeft, ArrowRight, Bell, ChevronDown, ChevronLeft, ChevronRight, ChevronUp, Delete, RefreshCw, WifiOff, X } from "lucide-react";
import { Component, useCallback, useEffect, useId, useRef, useState, type ErrorInfo, type ReactNode } from "react";
import { io, type Socket } from "socket.io-client";
import { money } from "./format";
import { Icon, type LucideIcon } from "./icons";

const EASE = [0.2, 0.9, 0.3, 1] as const;

// --- Fenêtres -------------------------------------------------------------------------------

/** Un panneau qui glisse depuis un bord (`side` : gauche par défaut) : un menu, des filtres.
 * Échap, la croix ou un toucher à côté le ferment. */
export function Drawer({ open, onClose, title, side = "left", children }: {
  open: boolean; onClose: () => void; title: ReactNode; side?: "left" | "right"; children: ReactNode;
}) {
  const id = useId();
  useEffect(() => {
    if (!open) return;
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [open, onClose]);
  return (
    <AnimatePresence>
      {open && (
        <div className="drawer" role="presentation">
          <motion.div className="drawer__backdrop" onClick={onClose} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} />
          <motion.aside className={`drawer__panel drawer__panel--${side}`} role="dialog" aria-modal="true" aria-labelledby={id}
            initial={{ x: side === "left" ? "-100%" : "100%" }} animate={{ x: 0 }} exit={{ x: side === "left" ? "-100%" : "100%" }}
            transition={{ duration: 0.28, ease: EASE }}>
            <div className="drawer__head">
              <h2 className="drawer__title" id={id}>{title}</h2>
              <button className="icon-btn" type="button" aria-label="Fermer" onClick={onClose}><Icon icon={X} /></button>
            </div>
            <div className="drawer__body">{children}</div>
          </motion.aside>
        </div>
      )}
    </AnimatePresence>
  );
}

/** Une bulle qui s'ouvre sous un bouton (`trigger`) : des options, un petit formulaire.
 * Un toucher ailleurs ou Échap la ferme. */
export function Popover({ trigger, children, label }: { trigger: ReactNode; children: ReactNode; label: string }) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (event: PointerEvent) => { if (box.current && !box.current.contains(event.target as Node)) setOpen(false); };
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") setOpen(false); };
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", key);
    return () => { document.removeEventListener("pointerdown", away); document.removeEventListener("keydown", key); };
  }, [open]);
  return (
    <div className="bubble" ref={box}>
      <button className="bubble__trigger" type="button" aria-expanded={open} aria-label={label} onClick={() => setOpen(!open)}>{trigger}</button>
      <AnimatePresence>
        {open && (
          <motion.div className="bubble__panel" role="dialog" aria-label={label}
            initial={{ opacity: 0, y: -6, scale: 0.97 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, y: -6, scale: 0.97 }} transition={{ duration: 0.16 }}>
            {children}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

// --- Saisie ---------------------------------------------------------------------------------

/** Le clavier d'un montant, comme une appli de paiement : de grandes touches, la virgule, ←.
 * `value` est le texte tapé (« 12,5 ») ; `max` chiffres avant la virgule. */
export function NumberPad({ value, onChange, max = 6, label = "Montant" }: { value: string; onChange: (value: string) => void; max?: number; label?: string }) {
  const press = (key: string) => {
    if (key === "back") return onChange(value.slice(0, -1));
    if (key === ",") return onChange(value.includes(",") ? value : `${value || "0"},`);
    const [whole, cents] = value.split(",");
    if (cents !== undefined && cents.length >= 2) return;
    if (cents === undefined && (whole ?? "").replace(/^0$/, "").length >= max) return;
    onChange(value === "0" ? key : value + key);
  };
  const keys = ["1", "2", "3", "4", "5", "6", "7", "8", "9", ",", "0", "back"];
  return (
    <div className="number-pad" role="group" aria-label={label}>
      <output className="number-pad__value" aria-live="polite">{value || "0"} €</output>
      <div className="number-pad__keys">
        {keys.map((key) => (
          <button key={key} type="button" className="number-pad__key" aria-label={key === "back" ? "Effacer" : key} onClick={() => press(key)}>
            {key === "back" ? <Icon icon={Delete} size={22} /> : key}
          </button>
        ))}
      </div>
    </div>
  );
}

/** Le montant tapé sur un NumberPad, en nombre (« 12,5 » → 12.5). */
export const padAmount = (value: string) => Number((value || "0").replace(",", ".")) || 0;

/** Une heure (« 08:30 ») à choisir en deux roues d'un toucher : heures, minutes par `step`. */
export function TimePicker({ label, value, onChange, step = 5 }: { label: string; value: string; onChange: (value: string) => void; step?: number }) {
  const [hours, minutes] = (value || "08:00").split(":").map(Number);
  const pad = (number: number) => String(number).padStart(2, "0");
  const set = (h: number, m: number) => onChange(`${pad((h + 24) % 24)}:${pad((m + 60) % 60)}`);
  return (
    <div className="time-picker" role="group" aria-label={label}>
      <span className="time-picker__label">{label}</span>
      <div className="time-picker__wheels">
        {[["Heures", hours, () => set(hours + 1, minutes), () => set(hours - 1, minutes)], ["Minutes", minutes, () => set(hours, minutes + step), () => set(hours, minutes - step)]].map(([name, number, up, down]) => (
          <div className="time-picker__wheel" key={name as string}>
            <button type="button" className="icon-btn" aria-label={`${name} +`} onClick={up as () => void}><Icon icon={ChevronUp} /></button>
            <output aria-label={name as string}>{pad(number as number)}</output>
            <button type="button" className="icon-btn" aria-label={`${name} −`} onClick={down as () => void}><Icon icon={ChevronDown} /></button>
          </div>
        ))}
      </div>
    </div>
  );
}

// --- Organiser ------------------------------------------------------------------------------

export type KanbanCard = { id: string; title: string; column: string; note?: string };

/** Un tableau de tâches en colonnes (« À faire », « En cours », « Fait ») qui défile de côté au
 * téléphone ; une carte change de colonne avec ses flèches. */
export function Kanban({ columns, cards, onMove, label }: {
  columns: { id: string; title: string }[]; cards: KanbanCard[]; onMove: (id: string, column: string) => void; label: string;
}) {
  return (
    <div className="kanban" role="region" aria-label={label}>
      {columns.map((column, index) => {
        const mine = cards.filter((card) => card.column === column.id);
        return (
          <section className="kanban__column" key={column.id} aria-label={column.title}>
            <h3 className="kanban__title">{column.title}<span className="kanban__count">{mine.length}</span></h3>
            <ul className="kanban__cards">
              <AnimatePresence initial={false}>
                {mine.map((card) => (
                  <motion.li layout key={card.id} className="kanban__card" initial={{ opacity: 0, scale: 0.96 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0, scale: 0.96 }}>
                    <b>{card.title}</b>
                    {card.note && <small>{card.note}</small>}
                    <span className="kanban__moves">
                      {index > 0 ? <button className="icon-btn" type="button" aria-label={`« ${card.title} » vers ${columns[index - 1].title}`} onClick={() => onMove(card.id, columns[index - 1].id)}><Icon icon={ArrowLeft} size={18} /></button> : <span />}
                      {index < columns.length - 1 ? <button className="icon-btn" type="button" aria-label={`« ${card.title} » vers ${columns[index + 1].title}`} onClick={() => onMove(card.id, columns[index + 1].id)}><Icon icon={ArrowRight} size={18} /></button> : <span />}
                    </span>
                  </motion.li>
                ))}
              </AnimatePresence>
              {!mine.length && <li className="kanban__empty">Rien ici</li>}
            </ul>
          </section>
        );
      })}
    </div>
  );
}

export type AgendaItem = { id: string; date: string; time?: string; title: string; note?: string; tone?: "accent" | "in" | "out" | "warn" };

/** Un agenda : les évènements groupés par jour (« lundi 2 novembre »), dans l'ordre. */
export function Agenda({ items, emptyText = "Rien de prévu." }: { items: AgendaItem[]; emptyText?: ReactNode }) {
  if (!items.length) return <p className="agenda__empty">{emptyText}</p>;
  const sorted = [...items].sort((a, b) => `${a.date} ${a.time ?? ""}`.localeCompare(`${b.date} ${b.time ?? ""}`));
  const days = [...new Set(sorted.map((item) => item.date))];
  const dayName = (date: string) => {
    const [year, month, day] = date.split("-").map(Number);
    const text = new Intl.DateTimeFormat("fr-FR", { weekday: "long", day: "numeric", month: "long" }).format(new Date(year, month - 1, day));
    return text.charAt(0).toUpperCase() + text.slice(1);
  };
  return (
    <div className="agenda">
      {days.map((date) => (
        <section key={date} className="agenda__day" aria-label={dayName(date)}>
          <h3 className="agenda__date">{dayName(date)}</h3>
          <ul className="agenda__items">
            {sorted.filter((item) => item.date === date).map((item) => (
              <li key={item.id} className={`agenda__item agenda__item--${item.tone ?? "accent"}`}>
                <span className="agenda__time">{item.time ?? "Journée"}</span>
                <span className="agenda__text"><b>{item.title}</b>{item.note && <small>{item.note}</small>}</span>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

// --- Suivre ---------------------------------------------------------------------------------

/** Une jauge en demi-cercle (0 à 1), la valeur au centre : un budget, une batterie, un score.
 * Elle passe à l'orange au-delà de `warnAt` et au rouge au-delà de 1. */
export function Gauge({ value, label, text, warnAt = 0.8 }: { value: number; label: string; text?: ReactNode; warnAt?: number }) {
  const still = useReducedMotion();
  const clamped = Math.min(1, Math.max(0, value || 0));
  const progress = useMotionValue(still ? clamped : 0);
  const offset = useTransform(progress, (current) => 100 - current * 100);
  useEffect(() => {
    if (still) { progress.set(clamped); return; }
    const controls = animate(progress, clamped, { duration: 0.8, ease: EASE });
    return () => controls.stop();
  }, [clamped, still, progress]);
  const tone = value > 1 ? "out" : value >= warnAt ? "warn" : "accent";
  return (
    <div className={`gauge gauge--${tone}`} role="img" aria-label={`${label} : ${Math.round((value || 0) * 100)} %`}>
      <svg viewBox="0 0 100 56" aria-hidden="true">
        <path className="gauge__track" d="M 8 50 A 42 42 0 0 1 92 50" pathLength={100} />
        <motion.path className="gauge__value" d="M 8 50 A 42 42 0 0 1 92 50" pathLength={100} strokeDasharray="100 100" style={{ strokeDashoffset: offset }} />
      </svg>
      <span className="gauge__text">{text ?? `${Math.round((value || 0) * 100)} %`}</span>
      <span className="gauge__label">{label}</span>
    </div>
  );
}

/** Un objectif d'épargne (une moto, des vacances) : ce qui est mis de côté, ce qui reste, et
 * combien par mois jusqu'à la date voulue. Montants en euros. */
export function GoalCard({ title, saved, target, until, icon }: { title: string; saved: number; target: number; until?: string; icon?: LucideIcon }) {
  const share = target > 0 ? Math.min(1, saved / target) : 0;
  const left = Math.max(0, target - saved);
  let perMonth = "";
  if (until && left > 0) {
    const [year, month] = until.split("-").map(Number);
    const now = new Date();
    const months = Math.max(1, (year - now.getFullYear()) * 12 + (month - 1 - now.getMonth()));
    perMonth = `${money(left / months)} par mois pendant ${months} mois`;
  }
  return (
    <div className="goal">
      <div className="goal__head">
        {icon && <span className="row__icon row__icon--accent"><Icon icon={icon} /></span>}
        <span className="goal__title"><b>{title}</b><small>{left > 0 ? `il reste ${money(left)}` : "objectif atteint"}</small></span>
        <span className="goal__amount">{money(saved)}<small>sur {money(target)}</small></span>
      </div>
      <div className="meter__track meter__track--in" role="progressbar" aria-label={title} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(share * 100)}>
        <i style={{ "--value": `${Math.round(share * 100)}%` } as React.CSSProperties} />
      </div>
      {perMonth && <p className="goal__plan">{perMonth}</p>}
    </div>
  );
}

// --- Listes ---------------------------------------------------------------------------------

/** Tirer la liste vers le bas pour rafraîchir (au téléphone) ; `onRefresh` rend une promesse. */
export function PullToRefresh({ onRefresh, children }: { onRefresh: () => Promise<unknown> | void; children: ReactNode }) {
  const y = useMotionValue(0);
  const [busy, setBusy] = useState(false);
  const turn = useTransform(y, [0, 80], [0, 270]);
  const shown = useTransform(y, [0, 40], [0, 1]);
  const release = async () => {
    if (y.get() > 70 && !busy) {
      setBusy(true);
      animate(y, 56);
      try { await onRefresh(); } finally { setBusy(false); animate(y, 0); }
    } else animate(y, 0);
  };
  return (
    <div className="pull">
      <motion.span className={`pull__spinner${busy ? " is-busy" : ""}`} style={{ opacity: shown, rotate: turn }} aria-hidden="true"><Icon icon={RefreshCw} /></motion.span>
      <motion.div style={{ y }} drag="y" dragConstraints={{ top: 0, bottom: 0 }} dragElastic={{ top: 0, bottom: 0.5 }} onDragEnd={release}
        onDrag={(_, info) => { if (window.scrollY > 0 || info.offset.y < 0) y.set(0); }}>
        {children}
      </motion.div>
      <span className="visually-hidden" aria-live="polite">{busy ? "Mise à jour…" : ""}</span>
    </div>
  );
}

/** Les `step` premiers éléments, puis « Voir plus » (une longue liste reste rapide). */
export function LoadMore<T>({ items, step = 20, render, more = "Voir plus" }: { items: T[]; step?: number; render: (items: T[]) => ReactNode; more?: string }) {
  const [count, setCount] = useState(step);
  return (
    <>
      {render(items.slice(0, count))}
      {count < items.length && (
        <button className="btn btn--small btn--block load-more" type="button" onClick={() => setCount(count + step)}>
          {more} ({items.length - count})
        </button>
      )}
    </>
  );
}

/** Des pages numérotées (1 2 3 … 9) avec précédente et suivante. */
export function Pagination({ page, pages, onPage }: { page: number; pages: number; onPage: (page: number) => void }) {
  if (pages <= 1) return null;
  const shown = [...new Set([1, page - 1, page, page + 1, pages].filter((number) => number >= 1 && number <= pages))].sort((a, b) => a - b);
  return (
    <nav className="pagination" aria-label="Pages">
      <button className="icon-btn" type="button" aria-label="Page précédente" disabled={page <= 1} onClick={() => onPage(page - 1)}><Icon icon={ChevronLeft} /></button>
      {shown.map((number, index) => (
        <span key={number} className="pagination__group">
          {index > 0 && number - shown[index - 1] > 1 && <span className="pagination__gap" aria-hidden="true">…</span>}
          <button type="button" className="pagination__page" aria-current={number === page ? "page" : undefined} onClick={() => onPage(number)}>{number}</button>
        </span>
      ))}
      <button className="icon-btn" type="button" aria-label="Page suivante" disabled={page >= pages} onClick={() => onPage(page + 1)}><Icon icon={ChevronRight} /></button>
    </nav>
  );
}

/** Le chemin jusqu'à l'écran ouvert (Réglages › Catégories › Courses). */
export function Breadcrumbs({ items }: { items: { label: string; href?: string; onClick?: () => void }[] }) {
  return (
    <nav className="crumbs" aria-label="Chemin">
      <ol>
        {items.map((item, index) => {
          const last = index === items.length - 1;
          return (
            <li key={index}>
              {last ? <span aria-current="page">{item.label}</span>
                : item.href ? <a href={item.href}>{item.label}</a>
                  : <button type="button" onClick={item.onClick}>{item.label}</button>}
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

// --- Images ---------------------------------------------------------------------------------

/** Des vignettes qui s'ouvrent en grand, une à la fois, avec précédente et suivante (et le
 * glissement du doigt). `photos` : des adresses d'images (ou des textes de PhotoField). */
export function Lightbox({ photos, label }: { photos: { src: string; alt: string }[]; label: string }) {
  const [open, setOpen] = useState<number | null>(null);
  const go = useCallback((step: number) => setOpen((index) => (index === null ? null : (index + step + photos.length) % photos.length)), [photos.length]);
  useEffect(() => {
    if (open === null) return;
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(null);
      if (event.key === "ArrowRight") go(1);
      if (event.key === "ArrowLeft") go(-1);
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [open, go]);
  return (
    <>
      <ul className="lightbox__thumbs" aria-label={label}>
        {photos.map((photo, index) => (
          <li key={photo.src + index}><button type="button" onClick={() => setOpen(index)} aria-label={`Agrandir : ${photo.alt}`}><img src={photo.src} alt="" loading="lazy" /></button></li>
        ))}
      </ul>
      <AnimatePresence>
        {open !== null && photos[open] && (
          <motion.div className="lightbox" role="dialog" aria-modal="true" aria-label={photos[open].alt} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}>
            <button className="icon-btn lightbox__close" type="button" aria-label="Fermer" onClick={() => setOpen(null)}><Icon icon={X} /></button>
            <motion.img key={open} src={photos[open].src} alt={photos[open].alt} className="lightbox__photo" drag="x" dragConstraints={{ left: 0, right: 0 }}
              onDragEnd={(_, info) => { if (Math.abs(info.offset.x) > 60) go(info.offset.x < 0 ? 1 : -1); }}
              initial={{ opacity: 0, scale: 0.96 }} animate={{ opacity: 1, scale: 1 }} />
            {photos.length > 1 && (
              <div className="lightbox__nav">
                <button className="icon-btn" type="button" aria-label="Photo précédente" onClick={() => go(-1)}><Icon icon={ChevronLeft} /></button>
                <span>{open + 1} / {photos.length}</span>
                <button className="icon-btn" type="button" aria-label="Photo suivante" onClick={() => go(1)}><Icon icon={ChevronRight} /></button>
              </div>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </>
  );
}

/** Une signature au doigt ; `onChange` reçoit l'image (PNG en texte) ou "" quand elle est effacée. */
export function SignaturePad({ label = "Signature", onChange }: { label?: string; onChange: (image: string) => void }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const drawing = useRef(false);
  const [empty, setEmpty] = useState(true);
  const point = (event: React.PointerEvent<HTMLCanvasElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    return [(event.clientX - box.left) * (event.currentTarget.width / box.width), (event.clientY - box.top) * (event.currentTarget.height / box.height)];
  };
  const start = (event: React.PointerEvent<HTMLCanvasElement>) => {
    const context = canvas.current?.getContext("2d");
    if (!context) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    drawing.current = true;
    const [x, y] = point(event);
    context.lineWidth = 3;
    context.lineCap = "round";
    context.strokeStyle = getComputedStyle(event.currentTarget).color;
    context.beginPath();
    context.moveTo(x, y);
  };
  const move = (event: React.PointerEvent<HTMLCanvasElement>) => {
    if (!drawing.current) return;
    const context = canvas.current?.getContext("2d");
    const [x, y] = point(event);
    context?.lineTo(x, y);
    context?.stroke();
  };
  const end = () => {
    if (!drawing.current || !canvas.current) return;
    drawing.current = false;
    setEmpty(false);
    onChange(canvas.current.toDataURL("image/png"));
  };
  const clear = () => {
    const context = canvas.current?.getContext("2d");
    if (canvas.current && context) context.clearRect(0, 0, canvas.current.width, canvas.current.height);
    setEmpty(true);
    onChange("");
  };
  return (
    <div className="signature">
      <span className="field__label">{label}</span>
      <canvas ref={canvas} className="signature__pad" width={600} height={220} aria-label={label} role="img"
        onPointerDown={start} onPointerMove={move} onPointerUp={end} onPointerCancel={end} />
      <div className="signature__foot">
        <small>{empty ? "Signez avec le doigt" : "Signé"}</small>
        <button className="btn btn--small" type="button" onClick={clear} disabled={empty}>Effacer</button>
      </div>
    </div>
  );
}

// --- Tenue de l'appli -----------------------------------------------------------------------

/** En ligne ou non (le téléphone a perdu le réseau). */
export function useOnline(): boolean {
  const [online, setOnline] = useState(typeof navigator === "undefined" ? true : navigator.onLine);
  useEffect(() => {
    const on = () => setOnline(true);
    const off = () => setOnline(false);
    window.addEventListener("online", on);
    window.addEventListener("offline", off);
    return () => { window.removeEventListener("online", on); window.removeEventListener("offline", off); };
  }, []);
  return online;
}

/** Un bandeau en haut quand le réseau manque : l'appli continue, ce qui est en ligne attend. */
export function OfflineBanner({ text = "Hors ligne : vos données restent sur ce téléphone." }: { text?: string }) {
  const online = useOnline();
  return (
    <AnimatePresence>
      {!online && (
        <motion.div className="offline-banner" role="status" initial={{ y: -40, opacity: 0 }} animate={{ y: 0, opacity: 1 }} exit={{ y: -40, opacity: 0 }}>
          <Icon icon={WifiOff} size={18} />{text}
        </motion.div>
      )}
    </AnimatePresence>
  );
}

/** Une partie de l'appli qui plante n'emporte pas tout l'écran : à sa place, une phrase claire
 * et « Réessayer ». Entourez chaque écran (`<SafeArea>…</SafeArea>`). */
export class SafeArea extends Component<{ children: ReactNode; text?: string }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Partie de l'appli en erreur :", error, info.componentStack);
  }
  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <div className="safe-area" role="alert">
        <p>{this.props.text ?? "Cette partie n'a pas pu s'afficher. Vos données sont intactes."}</p>
        <button className="btn btn--small" type="button" onClick={() => this.setState({ failed: false })}>Réessayer</button>
      </div>
    );
  }
}

/** Une cloche (ou une autre icône) avec le nombre de nouveautés en pastille. */
export function IconBadge({ icon = Bell, count, label, onClick }: { icon?: LucideIcon; count: number; label: string; onClick?: () => void }) {
  return (
    <button className="icon-btn icon-badge" type="button" aria-label={count ? `${label}, ${count} nouveau${count > 1 ? "x" : ""}` : label} onClick={onClick}>
      <Icon icon={icon} />
      <AnimatePresence>
        {count > 0 && (
          <motion.span key={count} className="icon-badge__count" initial={{ scale: 0.4 }} animate={{ scale: 1 }} exit={{ scale: 0 }} aria-hidden="true">
            {count > 99 ? "99+" : count}
          </motion.span>
        )}
      </AnimatePresence>
    </button>
  );
}

// --- Branchements ---------------------------------------------------------------------------

/** Lire une API (le serveur Express ou FastAPI des projets de départ) : `data`, `loading`,
 * `error` (une phrase claire) et `reload()`. `url` null : rien n'est demandé. */
export function useApi<T>(url: string | null, init?: RequestInit) {
  const [state, setState] = useState<{ data: T | null; loading: boolean; error: string }>({ data: null, loading: Boolean(url), error: "" });
  const [tick, setTick] = useState(0);
  const options = useRef(init);
  useEffect(() => {
    if (!url) return;
    let alive = true;
    setState((now) => ({ ...now, loading: true, error: "" }));
    fetch(url, options.current)
      .then(async (response) => {
        if (!response.ok) throw new Error(response.status === 404 ? "Introuvable." : `Le serveur a répondu ${response.status}.`);
        return (await response.json()) as T;
      })
      .then((data) => { if (alive) setState({ data, loading: false, error: "" }); })
      .catch((error: unknown) => {
        if (!alive) return;
        const offline = typeof navigator !== "undefined" && !navigator.onLine;
        setState((now) => ({ ...now, loading: false, error: offline ? "Pas de réseau : réessayez une fois en ligne." : error instanceof Error && !/fetch/i.test(error.message) ? error.message : "Le serveur ne répond pas." }));
      });
    return () => { alive = false; };
  }, [url, tick]);
  return { ...state, reload: () => setTick((count) => count + 1) };
}

/** Envoyer à une API (POST, PATCH, DELETE) ; rend la réponse, ou lance une Error à la phrase claire. */
export async function sendApi<T = unknown>(url: string, method: "POST" | "PATCH" | "PUT" | "DELETE", body?: unknown): Promise<T> {
  const response = await fetch(url, { method, headers: body === undefined ? {} : { "content-type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
  if (!response.ok) {
    const detail = await response.json().catch(() => null) as { error?: string; detail?: string } | null;
    throw new Error(detail?.error ?? detail?.detail ?? `Le serveur a répondu ${response.status}.`);
  }
  return (response.status === 204 ? null : await response.json()) as T;
}

/** Le temps réel de Socket.IO (le serveur node-api) : la dernière valeur de l'évènement
 * `event`, l'état de la connexion, et `emit` pour envoyer. Se reconnecte tout seul. */
export function useLive<T>(url: string, event: string, initial: T) {
  const [value, setValue] = useState<T>(initial);
  const [connected, setConnected] = useState(false);
  const socket = useRef<Socket | null>(null);
  useEffect(() => {
    const live = io(url, { transports: ["websocket", "polling"] });
    socket.current = live;
    live.on("connect", () => setConnected(true));
    live.on("disconnect", () => setConnected(false));
    live.on(event, (data: T) => setValue(data));
    return () => { live.close(); socket.current = null; };
  }, [url, event]);
  return { value, connected, emit: (name: string, data?: unknown) => socket.current?.emit(name, data) };
}

/** Un module Rust compilé en WebAssembly (le projet de départ rust-wasm le construit dans
 * public/) : ses fonctions exportées, ou null tant qu'il charge ; `error` s'il manque. */
export function useWasm<T extends Record<string, unknown>>(url = "./calc.wasm") {
  const [state, setState] = useState<{ exports: T | null; error: string }>({ exports: null, error: "" });
  useEffect(() => {
    let alive = true;
    fetch(url)
      .then((response) => {
        if (!response.ok) throw new Error(`${url} introuvable : construisez le module Rust (npm run wasm).`);
        return response.arrayBuffer();
      })
      .then((bytes) => WebAssembly.instantiate(bytes, {}))
      .then(({ instance }) => { if (alive) setState({ exports: instance.exports as T, error: "" }); })
      .catch((error: unknown) => { if (alive) setState({ exports: null, error: error instanceof Error ? error.message : String(error) }); });
    return () => { alive = false; };
  }, [url]);
  return state;
}
