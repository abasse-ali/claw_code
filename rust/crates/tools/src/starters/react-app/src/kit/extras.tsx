// Encore des widgets : zone de texte, mot de passe, alerte, fil de messages, frise, carrousel,
// compte à rebours, chiffre avec tendance, liste de détails, bouton copier, bouton à bascule,
// écran verrouillé par un code. Inspirés de shadcn/ui, HyperUI, daisyUI et Preline (MIT) ;
// habillés par app-kit.css, comme le reste du kit.

import { motion } from "framer-motion";
import { ArrowDown, ArrowUp, Check, ChevronLeft, ChevronRight, CircleAlert, CircleCheck, Copy, Eye, EyeOff, Info, Lock, Send, TriangleAlert, X } from "lucide-react";
import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Icon, type LucideIcon } from "./icons";
import { Shake } from "./motion";
import { toast } from "./toast";
import { CodeInput } from "./widgets";

/** Un texte sur plusieurs lignes (une note, un commentaire) qui grandit avec son contenu ;
 * avec `max`, le nombre de caractères restants. */
export function TextArea({ label, value, onChange, placeholder, max, rows = 3 }: {
  label: string; value: string; onChange: (value: string) => void; placeholder?: string; max?: number; rows?: number;
}) {
  const id = useId();
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const area = ref.current;
    if (!area) return;
    area.style.height = "auto";
    area.style.height = `${area.scrollHeight + 2}px`;
  }, [value]);
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      <textarea ref={ref} id={id} className="textarea" rows={rows} value={value} placeholder={placeholder} maxLength={max} onChange={(event) => onChange(event.target.value)} />
      {max !== undefined && <p className="field__help textarea__count">{max - value.length} caractères restants</p>}
    </div>
  );
}

const STRENGTH = ["Trop court", "Faible", "Moyen", "Bon", "Excellent"];

/** La solidité d'un mot de passe, de 0 (trop court) à 4. */
export function passwordStrength(password: string) {
  if (password.length < 8) return 0;
  const kinds = [/[a-z]/, /[A-Z]/, /\d/, /[^A-Za-z0-9]/].filter((kind) => kind.test(password)).length;
  return Math.min(4, Math.max(1, kinds - 1 + (password.length >= 12 ? 1 : 0)));
}

/** Un mot de passe, avec l'œil pour l'afficher ; `strength` montre sa solidité (à la création). */
export function PasswordField({ label, value, onChange, strength, error }: {
  label: string; value: string; onChange: (value: string) => void; strength?: boolean; error?: string;
}) {
  const id = useId();
  const [shown, setShown] = useState(false);
  const level = passwordStrength(value);
  return (
    <div className={`field${error ? " is-invalid" : ""}`}>
      <label htmlFor={id}>{label}</label>
      <span className="password">
        <input id={id} type={shown ? "text" : "password"} value={value} autoComplete={strength ? "new-password" : "current-password"} aria-invalid={Boolean(error)} onChange={(event) => onChange(event.target.value)} />
        <button type="button" className="icon-btn" aria-label={shown ? "Cacher le mot de passe" : "Afficher le mot de passe"} aria-pressed={shown} onClick={() => setShown(!shown)}>
          <Icon icon={shown ? EyeOff : Eye} size={18} />
        </button>
      </span>
      {strength && value && (
        <span className={`password__meter is-${level}`} aria-live="polite">
          <span aria-hidden="true">{[1, 2, 3, 4].map((step) => <i key={step} className={step <= level ? "is-on" : ""} />)}</span>
          {STRENGTH[level]}
        </span>
      )}
      {error && <p className="field__error">{error}</p>}
    </div>
  );
}

type AlertTone = "info" | "success" | "warn" | "error";
const ALERT_ICONS: Record<AlertTone, LucideIcon> = { info: Info, success: CircleCheck, warn: TriangleAlert, error: CircleAlert };

/** Un message dans l'écran (pas un toast) : ce qui manque, ce qui a réussi, un dépassement. */
export function Alert({ tone = "info", title, children, action, onClose }: {
  tone?: AlertTone; title?: ReactNode; children?: ReactNode; action?: { label: string; onClick: () => void }; onClose?: () => void;
}) {
  return (
    <div className={`alert alert--${tone}`} role={tone === "error" || tone === "warn" ? "alert" : "status"}>
      <Icon icon={ALERT_ICONS[tone]} size={20} />
      <div className="alert__main">
        {title && <b>{title}</b>}
        {children && <span>{children}</span>}
        {action && <button type="button" className="alert__action" onClick={action.onClick}>{action.label}</button>}
      </div>
      {onClose && (
        <button type="button" className="icon-btn alert__close" aria-label="Fermer" onClick={onClose}><Icon icon={X} size={16} /></button>
      )}
    </div>
  );
}

export type Message = { id: string; mine?: boolean; name?: string; text: ReactNode; time?: string };

/** Un fil de messages en bulles (les miens à droite), qui descend tout seul au dernier ; avec
 * `onSend`, le champ pour écrire en bas. */
export function Chat({ messages, onSend, placeholder = "Votre message", emptyText = "Aucun message pour l'instant." }: {
  messages: Message[]; onSend?: (text: string) => void; placeholder?: string; emptyText?: ReactNode;
}) {
  const [draft, setDraft] = useState("");
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => { end.current?.scrollIntoView({ block: "nearest" }); }, [messages.length]);
  const send = () => {
    const text = draft.trim();
    if (!text || !onSend) return;
    onSend(text);
    setDraft("");
  };
  return (
    <div className="chat">
      <div className="chat__list" role="log" aria-live="polite">
        {messages.length === 0 && <p className="chat__empty">{emptyText}</p>}
        {messages.map((message) => (
          <motion.div
            key={message.id}
            className={`chat__msg${message.mine ? " is-mine" : ""}`}
            initial={{ opacity: 0, y: 8, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            transition={{ duration: 0.2 }}
          >
            {message.name && !message.mine && <span className="chat__name">{message.name}</span>}
            <span className="chat__bubble">{message.text}</span>
            {message.time && <span className="chat__time">{message.time}</span>}
          </motion.div>
        ))}
        <div ref={end} />
      </div>
      {onSend && (
        <form className="chat__composer" onSubmit={(event) => { event.preventDefault(); send(); }}>
          <input value={draft} placeholder={placeholder} aria-label={placeholder} onChange={(event) => setDraft(event.target.value)} />
          <button type="submit" className="icon-btn chat__send" aria-label="Envoyer" disabled={!draft.trim()}><Icon icon={Send} size={18} /></button>
        </form>
      )}
    </div>
  );
}

/** Une frise d'événements, du plus récent au plus ancien ou l'inverse (le suivi d'une
 * commande, l'historique d'un objectif). */
export function Timeline({ items }: {
  items: { id: string; title: ReactNode; time?: ReactNode; text?: ReactNode; icon?: LucideIcon; tone?: "accent" | "in" | "out" | "warn"; done?: boolean }[];
}) {
  return (
    <ol className="timeline">
      {items.map((item) => (
        <li key={item.id} className={`timeline__item timeline__item--${item.tone ?? "accent"}${item.done === false ? " is-todo" : ""}`}>
          <span className="timeline__dot" aria-hidden="true">{item.icon && <Icon icon={item.icon} size={14} />}</span>
          <div className="timeline__body">
            <span className="timeline__head"><b>{item.title}</b>{item.time && <small>{item.time}</small>}</span>
            {item.text && <span className="timeline__text">{item.text}</span>}
          </div>
        </li>
      ))}
    </ol>
  );
}

/** Des cartes qui défilent de côté au doigt, avec les points et les flèches. */
export function Carousel({ items, label }: { items: { id: string; content: ReactNode }[]; label: string }) {
  const track = useRef<HTMLDivElement>(null);
  const [index, setIndex] = useState(0);
  const go = (next: number) => {
    const target = Math.max(0, Math.min(items.length - 1, next));
    const slide = track.current?.children[target] as HTMLElement | undefined;
    track.current?.scrollTo({ left: slide?.offsetLeft ?? 0, behavior: "smooth" });
  };
  return (
    <section className="carousel" aria-roledescription="carrousel" aria-label={label}>
      <div
        ref={track}
        className="carousel__track"
        onScroll={(event) => {
          const box = event.currentTarget;
          setIndex(Math.round(box.scrollLeft / Math.max(1, box.clientWidth)));
        }}
      >
        {items.map((item, at) => (
          <div key={item.id} className="carousel__slide" role="group" aria-roledescription="diapositive" aria-label={`${at + 1} sur ${items.length}`}>{item.content}</div>
        ))}
      </div>
      {items.length > 1 && (
        <div className="carousel__nav">
          <button type="button" className="icon-btn" aria-label="Précédent" disabled={index === 0} onClick={() => go(index - 1)}><Icon icon={ChevronLeft} size={18} /></button>
          <span className="carousel__dots">
            {items.map((item, at) => <button key={item.id} type="button" aria-label={`Aller à ${at + 1}`} aria-current={at === index} onClick={() => go(at)} />)}
          </span>
          <button type="button" className="icon-btn" aria-label="Suivant" disabled={index === items.length - 1} onClick={() => go(index + 1)}><Icon icon={ChevronRight} size={18} /></button>
        </div>
      )}
    </section>
  );
}

/** Le temps qui reste jusqu'à une date (« 2026-12-24T18:00 », à l'heure du téléphone), en
 * jours, heures, minutes et secondes ; `doneText` une fois l'heure passée. */
export function Countdown({ to, label, doneText = "C'est l'heure !" }: { to: string; label: string; doneText?: ReactNode }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  const left = Math.max(0, Math.floor((new Date(to).getTime() - now) / 1000));
  if (left === 0) return <p className="countdown__done">{doneText}</p>;
  const days = Math.floor(left / 86400);
  const parts: [number, string][] = [[days, days > 1 ? "jours" : "jour"], [Math.floor(left / 3600) % 24, "h"], [Math.floor(left / 60) % 60, "min"], [left % 60, "s"]];
  return (
    <div className="countdown" role="timer" aria-label={`${label} : ${parts.map(([value, unit]) => `${value} ${unit}`).join(" ")}`}>
      {parts.map(([value, unit]) => (
        <span key={unit} className="countdown__part" aria-hidden="true"><b>{String(value).padStart(unit.startsWith("jour") ? 1 : 2, "0")}</b><small>{unit}</small></span>
      ))}
    </div>
  );
}

/** Un chiffre clé avec son évolution (« +12 % sur le mois dernier ») ; `goodWhenUp` à false
 * quand une hausse est une mauvaise nouvelle (les dépenses). */
export function Stat({ label, value, delta, deltaText, goodWhenUp = true }: {
  label: ReactNode; value: ReactNode; delta?: number; deltaText?: ReactNode; goodWhenUp?: boolean;
}) {
  const up = (delta ?? 0) > 0;
  const good = delta === undefined || delta === 0 ? undefined : up === goodWhenUp;
  return (
    <div className="stat">
      <span className="stat__label">{label}</span>
      <b className="stat__value">{value}</b>
      {delta !== undefined && (
        <span className={`stat__delta${good === undefined ? "" : good ? " is-good" : " is-bad"}`}>
          {delta !== 0 && <Icon icon={up ? ArrowUp : ArrowDown} size={14} />}
          {`${up ? "+" : ""}${new Intl.NumberFormat("fr-FR", { maximumFractionDigits: 1 }).format(delta)} %`}
          {deltaText && <small>{deltaText}</small>}
        </span>
      )}
    </div>
  );
}

/** Des informations en lignes « libellé : valeur » (le détail d'une dépense, d'un contact). */
export function DetailList({ items }: { items: { label: ReactNode; value: ReactNode }[] }) {
  return (
    <dl className="details">
      {items.map((item, at) => <div key={at} className="details__row"><dt>{item.label}</dt><dd>{item.value}</dd></div>)}
    </dl>
  );
}

/** Un bouton qui copie un texte (un IBAN, un lien de partage) et le confirme. */
export function CopyButton({ text, label = "Copier", done = "Copié" }: { text: string; label?: string; done?: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      toast("Copie impossible : sélectionnez le texte à la main", { tone: "error" });
    }
  };
  return (
    <button type="button" className="btn btn--small copy-btn" onClick={copy} aria-live="polite">
      <Icon icon={copied ? Check : Copy} size={16} />{copied ? done : label}
    </button>
  );
}

/** Un bouton qu'on allume et qu'on éteint (favori, épinglé), qui rebondit quand on l'allume. */
export function ToggleButton({ pressed, onChange, icon, label }: { pressed: boolean; onChange: (pressed: boolean) => void; icon: LucideIcon; label: string }) {
  return (
    <motion.button
      type="button"
      className="icon-btn toggle-btn"
      aria-label={label}
      aria-pressed={pressed}
      whileTap={{ scale: 0.85 }}
      animate={pressed ? { scale: [1, 1.25, 1] } : { scale: 1 }}
      transition={{ duration: 0.3 }}
      onClick={() => onChange(!pressed)}
    >
      <Icon icon={icon} />
    </motion.button>
  );
}

/** Un écran qui cache l'appli jusqu'au bon code (`check(code)` → vrai), à mettre autour
 * d'AppShell ; le code et sa vérification sont ceux de l'appli. */
export function LockScreen({ check, length = 4, title = "Appli verrouillée", children }: {
  check: (code: string) => boolean; length?: number; title?: string; children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [code, setCode] = useState("");
  const [misses, setMisses] = useState(0);
  const box = useRef<HTMLDivElement>(null);
  if (open) return <>{children}</>;
  return (
    <main className="lock" ref={box}>
      <span className="lock__icon"><Icon icon={Lock} size={28} /></span>
      <h1>{title}</h1>
      <div className="lock__code">
        <Shake trigger={misses}>
          <CodeInput
            label="Code"
            length={length}
            value={code}
            onChange={(next) => {
              setCode(next);
              if (next.length < length) return;
              if (check(next)) setOpen(true);
              else {
                setMisses((count) => count + 1);
                setCode("");
                // Après CodeInput, qui place lui-même le curseur sur la dernière case.
                setTimeout(() => box.current?.querySelector("input")?.focus());
              }
            }}
          />
        </Shake>
      </div>
      <p className={`lock__note${misses ? " is-error" : ""}`} aria-live="polite">{misses ? "Code incorrect, réessayez" : `Entrez votre code à ${length} chiffres`}</p>
    </main>
  );
}
