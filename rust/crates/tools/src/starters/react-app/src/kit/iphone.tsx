// Ce que les applis d'iPhone les plus soignées font de mieux : la pile de cartes de Wallet, une
// carte bancaire qui se remplit pendant la saisie et se retourne pour le code, un billet à QR
// code, un ticket de caisse dentelé, des notifications empilées, la pastille du haut (Dynamic
// Island), un volet à crans comme dans Plans, la roue de sélection. Toutes se calment quand le
// téléphone demande moins de mouvement ; app-kit.css les habille en clair et en sombre.

import { AnimatePresence, animate, motion, useMotionValue, useReducedMotion, type PanInfo } from "framer-motion";
import { type LucideIcon } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { money } from "./format";
import { Icon } from "./icons";
import { QRCode } from "./qr";

const EASE = [0.2, 0.9, 0.3, 1] as const;
const SPRING = { type: "spring", stiffness: 320, damping: 32 } as const;
/** Ce qui dépasse de chaque carte dans la pile : son nom, son montant et sa ligne de détail. */
const STEP = 66;

// --- Cartes -------------------------------------------------------------------------------------

export type WalletItem = { id: string; title: string; sub?: string; amount?: number; colors?: [string, string]; content?: ReactNode };

/** La pile de cartes d'Apple Wallet : les cartes se chevauchent, celle qu'on touche se pose au
 * premier plan et montre son contenu ; un second toucher la remet dans la pile. */
export function WalletStack({ items, label = "Cartes" }: { items: WalletItem[]; label?: string }) {
  const [open, setOpen] = useState<string | null>(null);
  const still = useReducedMotion();
  const shown = open ? items.findIndex((item) => item.id === open) : -1;
  return (
    <div className="wallet" role="list" aria-label={label} style={{ height: open ? 230 + 46 * (items.length - 1) + 160 : 190 + STEP * (items.length - 1) } as CSSProperties}>
      {items.map((item, index) => {
        const isOpen = item.id === open;
        const behind = shown >= 0 && !isOpen;
        const rank = behind ? (index > shown ? index - 1 : index) : index;
        const y = shown < 0 ? index * STEP : isOpen ? 0 : 230 + 160 + rank * 46 - 210;
        return (
          <motion.div key={item.id} role="listitem" className="wallet__card"
            style={{ background: `linear-gradient(135deg, ${item.colors?.[0] ?? "var(--hero-1)"}, ${item.colors?.[1] ?? "var(--hero-2)"})`, zIndex: isOpen ? 50 : index }}
            animate={{ y, scale: behind ? 0.94 : 1 }} transition={still ? { duration: 0 } : SPRING}>
            {/* Seule la bande du haut se touche : celle qui dépasse quand les cartes sont empilées. */}
            <button type="button" className="wallet__tap" aria-expanded={isOpen} onClick={() => setOpen(isOpen ? null : item.id)}>
              <span className="wallet__head"><strong>{item.title}</strong>{item.amount != null && <b>{money(item.amount)}</b>}</span>
              {item.sub && <span className="wallet__sub">{item.sub}</span>}
            </button>
            <AnimatePresence>{isOpen && item.content && (
              <motion.span className="wallet__content" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }}>{item.content}</motion.span>
            )}</AnimatePresence>
          </motion.div>
        );
      })}
    </div>
  );
}

const groups = (digits: string) => digits.padEnd(16, "•").slice(0, 16).replace(/(.{4})/g, "$1 ").trim();

/** Une carte bancaire qui se remplit pendant qu'on tape (numéro, nom, date) et se retourne pour
 * le code de sécurité. Affichage seulement : posez-la au-dessus des champs du paiement. */
export function BankCard({ number = "", name = "", expiry = "", cvc = "", flipped = false, brand = "Claw Bank" }: {
  number?: string; name?: string; expiry?: string; cvc?: string; flipped?: boolean; brand?: string;
}) {
  const still = useReducedMotion();
  const digits = number.replace(/\D/g, "");
  return (
    <div className="bank-card" aria-label={`Carte se terminant par ${digits.slice(-4) || "…"}`} role="img">
      <motion.div className="bank-card__inner" animate={{ rotateY: flipped ? 180 : 0 }} transition={still ? { duration: 0 } : { duration: 0.6, ease: EASE }}>
        <div className="bank-card__face">
          <span className="bank-card__brand">{brand}</span>
          <span className="bank-card__chip" aria-hidden="true" />
          <span className="bank-card__number">{groups(digits)}</span>
          <span className="bank-card__foot">
            <span><small>Titulaire</small>{name.trim().toUpperCase() || "VOTRE NOM"}</span>
            <span><small>Expire</small>{expiry || "MM/AA"}</span>
          </span>
        </div>
        <div className="bank-card__face bank-card__face--back">
          <span className="bank-card__band" aria-hidden="true" />
          <span className="bank-card__cvc"><small>Code</small>{cvc.padEnd(3, "•").slice(0, 4)}</span>
        </div>
      </motion.div>
    </div>
  );
}

/** Un billet (concert, train, réservation) : deux parties séparées par des encoches et un
 * pointillé, et un QR code à présenter. `rows` : [{ label, value }]. */
export function TicketCard({ title, sub, rows = [], code, codeLabel = "QR code du billet", accent }: {
  title: string; sub?: string; rows?: { label: string; value: string }[]; code?: string; codeLabel?: string; accent?: string;
}) {
  return (
    <div className="ticket" style={accent ? ({ "--ticket": accent } as CSSProperties) : undefined}>
      <div className="ticket__top">
        <strong>{title}</strong>
        {sub && <span>{sub}</span>}
        {rows.length > 0 && <dl className="ticket__rows">{rows.map((row) => <div key={row.label}><dt>{row.label}</dt><dd>{row.value}</dd></div>)}</dl>}
      </div>
      <div className="ticket__cut" aria-hidden="true" />
      {code && <div className="ticket__code"><QRCode value={code} label={codeLabel} size={150} download={false} /></div>}
    </div>
  );
}

/** Un ticket de caisse au bord dentelé : les lignes, puis le total. `lines` : [{ label, amount,
 * qty }] ; montants en euros. */
export function Receipt({ title, date, lines, footer }: { title: string; date?: string; lines: { label: string; amount: number; qty?: number }[]; footer?: string }) {
  const total = lines.reduce((sum, line) => sum + line.amount * (line.qty ?? 1), 0);
  return (
    <div className="receipt">
      <strong className="receipt__title">{title}</strong>
      {date && <span className="receipt__date">{date}</span>}
      <ul className="receipt__lines">
        {lines.map((line, index) => (
          <li key={index}><span>{line.qty && line.qty > 1 ? `${line.qty} × ` : ""}{line.label}</span><span>{money(line.amount * (line.qty ?? 1))}</span></li>
        ))}
      </ul>
      <p className="receipt__total"><span>Total</span><strong>{money(total)}</strong></p>
      {footer && <p className="receipt__footer">{footer}</p>}
    </div>
  );
}

// --- Notifications et pastille ------------------------------------------------------------------

export type StackedNotice = { id: string; icon?: LucideIcon; app?: string; title: string; text?: string; time?: string };

/** Les notifications empilées de l'écran verrouillé : la plus récente devant, les autres
 * dépassent derrière ; un toucher les étale, un autre les regroupe. */
export function NotificationStack({ items, label = "Notifications" }: { items: StackedNotice[]; label?: string }) {
  const [open, setOpen] = useState(false);
  const still = useReducedMotion();
  const visible = items.slice(0, open ? items.length : 3);
  return (
    <div className="notices" role="list" aria-label={label}>
      {items.length > 1 && (
        <button type="button" className="notices__toggle" onClick={() => setOpen(!open)} aria-expanded={open}>
          {open ? "Regrouper" : `${items.length} notifications`}
        </button>
      )}
      <div className="notices__stack" style={{ height: open ? undefined : 92 + Math.min(2, items.length - 1) * 10 }}>
        {visible.map((item, index) => (
          <motion.div key={item.id} role="listitem" className={`notice${!open && index > 0 ? " is-behind" : ""}`} layout={!still}
            style={open ? { position: "relative" } : { position: "absolute", left: 0, right: 0, top: index * 10, zIndex: 10 - index }}
            animate={{ scale: open ? 1 : 1 - index * 0.05, opacity: open || index === 0 ? 1 : 0.85 - index * 0.2 }}
            transition={still ? { duration: 0 } : SPRING} onClick={() => !open && setOpen(true)}>
            {item.icon && <span className="notice__icon"><Icon icon={item.icon} size={18} /></span>}
            <span className="notice__body">
              <span className="notice__meta">{item.app && <b>{item.app}</b>}{item.time && <small>{item.time}</small>}</span>
              <strong>{item.title}</strong>
              {item.text && <span>{item.text}</span>}
            </span>
          </motion.div>
        ))}
      </div>
    </div>
  );
}

/** La pastille noire du haut de l'iPhone (Dynamic Island) : petite au repos, elle s'agrandit pour
 * une activité en cours (un minuteur, un envoi, une course). `active` : ce qu'elle montre. */
export function DynamicIsland({ active, start, end, children }: { active: boolean; start?: ReactNode; end?: ReactNode; children?: ReactNode }) {
  const still = useReducedMotion();
  return (
    <div className="island-wrap">
      <motion.div className={`island${active ? " is-active" : ""}`} layout={!still} transition={still ? { duration: 0 } : { type: "spring", stiffness: 400, damping: 30 }}
        role="status" aria-live="polite">
        <AnimatePresence mode="popLayout" initial={false}>
          {active ? (
            <motion.div key="on" className="island__content" initial={{ opacity: 0, filter: "blur(6px)" }} animate={{ opacity: 1, filter: "blur(0px)" }} exit={{ opacity: 0 }}>
              <span className="island__row"><span>{start}</span><span>{end}</span></span>
              {children && <span className="island__more">{children}</span>}
            </motion.div>
          ) : <motion.span key="off" className="island__dot" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} aria-hidden="true" />}
        </AnimatePresence>
      </motion.div>
    </div>
  );
}

// --- Volet à crans ------------------------------------------------------------------------------

/** Un volet posé en bas de l'écran, comme dans Plans : on le tire entre trois hauteurs (un
 * aperçu, la moitié, presque tout l'écran). `snap` : la hauteur de départ, 0, 1 ou 2. */
export function SnapSheet({ children, title, peek = 120, snap = 0 }: { children: ReactNode; title?: string; peek?: number; snap?: 0 | 1 | 2 }) {
  const [level, setLevel] = useState(snap);
  const [height, setHeight] = useState(800);
  useEffect(() => {
    const measure = () => setHeight(window.innerHeight);
    measure();
    addEventListener("resize", measure);
    return () => removeEventListener("resize", measure);
  }, []);
  const stops = [height - peek, height * 0.5, height * 0.08];
  const y = useMotionValue(stops[snap]);
  const still = useReducedMotion();
  useEffect(() => {
    const controls = animate(y, stops[level], still ? { duration: 0 } : { type: "spring", stiffness: 300, damping: 34 });
    return () => controls.stop();
  }, [level, height]);
  const release = (_: unknown, info: PanInfo) => {
    const at = y.get() + info.velocity.y * 0.15;
    const nearest = stops.reduce((best, stop, index) => (Math.abs(stop - at) < Math.abs(stops[best] - at) ? index : best), 0);
    setLevel(nearest as 0 | 1 | 2);
    animate(y, stops[nearest], { type: "spring", stiffness: 300, damping: 34 });
  };
  return (
    <motion.section className="snap-sheet" style={{ y }} drag="y" dragConstraints={{ top: stops[2], bottom: stops[0] }} dragElastic={0.08}
      dragMomentum={false} onDragEnd={release} aria-label={title}>
      <button type="button" className="snap-sheet__grip" onClick={() => setLevel(level === 0 ? 1 : level === 1 ? 2 : 0)}
        aria-label={level === 2 ? "Réduire le volet" : "Agrandir le volet"} />
      {title && <h2 className="snap-sheet__title">{title}</h2>}
      <div className="snap-sheet__body">{children}</div>
    </motion.section>
  );
}

// --- Roue de sélection --------------------------------------------------------------------------

const ROW = 40;

/** La roue de sélection d'iPhone (une heure, une quantité, un jour) : on la fait tourner au doigt,
 * à la molette ou aux flèches du clavier ; la valeur au centre est choisie. */
export function WheelPicker<T extends string | number>({ options, value, onChange, label }: {
  options: { value: T; label: string }[]; value: T; onChange: (value: T) => void; label: string;
}) {
  const list = useRef<HTMLDivElement>(null);
  const index = Math.max(0, options.findIndex((option) => option.value === value));
  const settle = useRef<number | undefined>(undefined);
  useEffect(() => {
    const el = list.current;
    if (el && Math.abs(el.scrollTop - index * ROW) > 2) el.scrollTo({ top: index * ROW });
  }, [index]);
  return (
    <div className="wheel" role="listbox" aria-label={label} tabIndex={0}
      onKeyDown={(event) => {
        if (event.key === "ArrowDown") { event.preventDefault(); onChange(options[Math.min(options.length - 1, index + 1)].value); }
        if (event.key === "ArrowUp") { event.preventDefault(); onChange(options[Math.max(0, index - 1)].value); }
      }}>
      <div className="wheel__lens" aria-hidden="true" />
      <div ref={list} className="wheel__list" onScroll={(event) => {
        const top = event.currentTarget.scrollTop;
        window.clearTimeout(settle.current);
        settle.current = window.setTimeout(() => {
          const next = options[Math.max(0, Math.min(options.length - 1, Math.round(top / ROW)))];
          if (next && next.value !== value) onChange(next.value);
        }, 90);
      }}>
        <div className="wheel__pad" />
        {options.map((option, position) => (
          <div key={String(option.value)} role="option" aria-selected={position === index} className="wheel__item"
            style={{ opacity: Math.max(0.25, 1 - Math.abs(position - index) * 0.28) }} onClick={() => onChange(option.value)}>
            {option.label}
          </div>
        ))}
        <div className="wheel__pad" />
      </div>
    </div>
  );
}
