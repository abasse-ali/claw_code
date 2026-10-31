// Encore des façons de montrer et de choisir : la bande des jours de la semaine, un curseur
// à deux poignées (un prix de… à…), une période avec ses raccourcis, une barre de
// répartition, le prévu face au réel, un sondage, un classement, un groupe d'avatars, les
// mots cherchés mis en évidence, un formulaire en plusieurs étapes.

import { motion, useReducedMotion } from "framer-motion";
import { Check, ChevronLeft, ChevronRight, Crown } from "lucide-react";
import { Slider as RadixSlider } from "radix-ui";
import { useId, useState, type FormEvent, type ReactNode } from "react";
import { Avatar } from "./blocks";
import { money, shiftDay, today } from "./format";
import { Icon } from "./icons";

const EASE = [0.2, 0.9, 0.3, 1] as const;
const dayNumber = (date: string) => Number(date.slice(8, 10));
const weekdayShort = new Intl.DateTimeFormat("fr-FR", { weekday: "short" });
const parse = (date: string) => { const [y, m, d] = date.split("-").map(Number); return new Date(y, m - 1, d); };

/** La bande des 7 jours d'une semaine (lun. à dim.), pour choisir un jour ; les flèches
 * changent de semaine. `marked` : les jours qui ont quelque chose (un point dessous). */
export function WeekStrip({ value, onChange, marked = [] }: { value: string; onChange: (date: string) => void; marked?: string[] }) {
  const weekday = (parse(value).getDay() + 6) % 7;
  const monday = shiftDay(value, -weekday);
  const days = Array.from({ length: 7 }, (_, index) => shiftDay(monday, index));
  const now = today();
  return (
    <div className="week-strip">
      <button className="icon-btn" type="button" aria-label="Semaine précédente" onClick={() => onChange(shiftDay(value, -7))}><Icon icon={ChevronLeft} /></button>
      <div className="week-strip__days" role="radiogroup" aria-label="Jour">
        {days.map((date) => (
          <button key={date} type="button" role="radio" aria-checked={date === value}
            className={`week-strip__day${date === value ? " is-picked" : ""}${date === now ? " is-today" : ""}`}
            aria-label={new Intl.DateTimeFormat("fr-FR", { weekday: "long", day: "numeric", month: "long" }).format(parse(date))}
            onClick={() => onChange(date)}>
            <small>{weekdayShort.format(parse(date)).replace(".", "")}</small>
            <b>{dayNumber(date)}</b>
            <i className={marked.includes(date) ? "is-marked" : ""} />
          </button>
        ))}
      </div>
      <button className="icon-btn" type="button" aria-label="Semaine suivante" onClick={() => onChange(shiftDay(value, 7))}><Icon icon={ChevronRight} /></button>
    </div>
  );
}

/** Un curseur à deux poignées : de… à… (un budget, un prix, des heures). */
export function RangeSlider({ label, value, onChange, min = 0, max = 100, step = 1, format = String }: {
  label: string; value: [number, number]; onChange: (value: [number, number]) => void; min?: number; max?: number; step?: number; format?: (value: number) => string;
}) {
  return (
    <div className="field">
      <span className="field__label slider__label">{label}<b>{format(value[0])} – {format(value[1])}</b></span>
      <RadixSlider.Root className="slider" value={value} min={min} max={max} step={step} minStepsBetweenThumbs={1} onValueChange={([low, high]) => onChange([low, high])}>
        <RadixSlider.Track className="slider__track"><RadixSlider.Range className="slider__range" /></RadixSlider.Track>
        <RadixSlider.Thumb className="slider__thumb" aria-label={`${label} : minimum`} />
        <RadixSlider.Thumb className="slider__thumb" aria-label={`${label} : maximum`} />
      </RadixSlider.Root>
    </div>
  );
}

export type Period = { from: string; to: string };

/** Une période (du… au…) avec des raccourcis : 7 jours, 30 jours, ce mois, cette année. */
export function DateRange({ label, value, onChange }: { label: string; value: Period; onChange: (value: Period) => void }) {
  const id = useId();
  const now = today();
  const presets: [string, Period][] = [
    ["7 jours", { from: shiftDay(now, -6), to: now }],
    ["30 jours", { from: shiftDay(now, -29), to: now }],
    ["Ce mois", { from: `${now.slice(0, 7)}-01`, to: now }],
    ["Cette année", { from: `${now.slice(0, 4)}-01-01`, to: now }],
  ];
  const wrong = value.from && value.to && value.from > value.to;
  return (
    <fieldset className={`date-range field${wrong ? " is-invalid" : ""}`}>
      <legend className="field__label">{label}</legend>
      <div className="date-range__presets">
        {presets.map(([name, period]) => (
          <button key={name} type="button" className="chip"
            aria-pressed={value.from === period.from && value.to === period.to} onClick={() => onChange(period)}>{name}</button>
        ))}
      </div>
      <div className="field-row">
        <div className="field"><label htmlFor={`${id}-du`}>Du</label><input id={`${id}-du`} type="date" value={value.from} max={value.to || undefined} onChange={(event) => onChange({ ...value, from: event.target.value })} /></div>
        <div className="field"><label htmlFor={`${id}-au`}>Au</label><input id={`${id}-au`} type="date" value={value.to} min={value.from || undefined} onChange={(event) => onChange({ ...value, to: event.target.value })} /></div>
      </div>
      {wrong && <p className="field__error">La fin vient avant le début.</p>}
    </fieldset>
  );
}

/** Une date est-elle dans la période ? (`inPeriod(op.date, période)`) */
export const inPeriod = (date: string, period: Period) => (!period.from || date >= period.from) && (!period.to || date <= period.to);

const PALETTE = ["var(--accent)", "#e0803a", "#2f9e6e", "#d0508a", "#3d8bd9", "#a07a2c", "#7a6fd0", "#5f7480"];

/** Une barre de répartition (où part l'argent, le temps) avec sa légende ; `format` écrit
 * chaque valeur (money par défaut). */
export function StackedBar({ parts, label, format = money }: { parts: { name: string; value: number; color?: string }[]; label: string; format?: (value: number) => string }) {
  const shown = parts.filter((part) => part.value > 0);
  const total = shown.reduce((sum, part) => sum + part.value, 0);
  const still = useReducedMotion();
  if (!total) return <p className="muted">Rien à répartir.</p>;
  return (
    <figure className="stacked">
      <div className="stacked__bar" role="img" aria-label={`${label} : ${shown.map((part) => `${part.name} ${Math.round((part.value / total) * 100)} %`).join(", ")}`}>
        {shown.map((part, index) => (
          <motion.i key={part.name} style={{ background: part.color ?? PALETTE[index % PALETTE.length] }}
            initial={{ flexGrow: still ? part.value : 0 }} animate={{ flexGrow: part.value }} transition={{ duration: 0.6, ease: EASE }} />
        ))}
      </div>
      <figcaption className="stacked__legend">
        {shown.map((part, index) => (
          <span key={part.name}><i style={{ background: part.color ?? PALETTE[index % PALETTE.length] }} />{part.name}<b>{format(part.value)}</b><small>{Math.round((part.value / total) * 100)} %</small></span>
        ))}
      </figcaption>
    </figure>
  );
}

/** Le prévu face au réel, ligne par ligne (le budget de chaque catégorie et ce qui est
 * dépensé) : la barre passe au rouge au-delà du prévu. */
export function CompareBars({ rows, plannedLabel = "prévu", format = money }: {
  rows: { name: string; planned: number; actual: number }[]; plannedLabel?: string; format?: (value: number) => string;
}) {
  return (
    <ul className="compare">
      {rows.map((row) => {
        const share = row.planned > 0 ? row.actual / row.planned : row.actual > 0 ? 2 : 0;
        const over = share > 1;
        return (
          <li key={row.name} className={`compare__row${over ? " is-over" : ""}`}>
            <span className="compare__head"><b>{row.name}</b><span>{format(row.actual)} <small>/ {format(row.planned)} {plannedLabel}</small></span></span>
            <span className="compare__track" role="progressbar" aria-label={row.name} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(share * 100)}>
              <i style={{ width: `${Math.min(100, share * 100)}%` }} />
              {over && <em style={{ left: `${100 / share}%` }} aria-hidden="true" />}
            </span>
            <small className="compare__note">{over ? `${format(row.actual - row.planned)} de plus que prévu` : `reste ${format(row.planned - row.actual)}`}</small>
          </li>
        );
      })}
    </ul>
  );
}

/** Un sondage : on choisit une réponse, puis les pourcentages s'affichent. `votes` : le
 * nombre de voix de chaque réponse (sans celle de l'utilisateur), `voted` : sa réponse. */
export function Poll({ question, options, votes, voted, onVote }: {
  question: string; options: string[]; votes: number[]; voted?: number | null; onVote: (index: number) => void;
}) {
  const counts = options.map((_, index) => (votes[index] ?? 0) + (voted === index ? 1 : 0));
  const total = counts.reduce((sum, count) => sum + count, 0);
  const done = voted !== undefined && voted !== null;
  return (
    <fieldset className="poll">
      <legend className="poll__question">{question}</legend>
      {options.map((option, index) => {
        const share = total ? Math.round((counts[index] / total) * 100) : 0;
        return (
          <button key={option} type="button" className={`poll__option${voted === index ? " is-mine" : ""}`} disabled={done} aria-pressed={voted === index} onClick={() => onVote(index)}>
            {done && <motion.i className="poll__fill" initial={{ width: 0 }} animate={{ width: `${share}%` }} transition={{ duration: 0.6, ease: EASE }} aria-hidden="true" />}
            <span className="poll__text">{voted === index && <Icon icon={Check} size={16} />}{option}</span>
            {done && <b className="poll__share">{share} %</b>}
          </button>
        );
      })}
      <small className="poll__total">{total} vote{total > 1 ? "s" : ""}</small>
    </fieldset>
  );
}

/** Un classement (des joueurs, des colocataires, des produits) : rang, nom, score, le premier
 * couronné ; `me` met en avant la ligne de l'utilisateur. */
export function Leaderboard({ rows, me, format = String }: { rows: { name: string; score: number }[]; me?: string; format?: (score: number) => string }) {
  const sorted = [...rows].sort((a, b) => b.score - a.score);
  return (
    <ol className="leaderboard">
      {sorted.map((row, index) => (
        <li key={row.name} className={`leaderboard__row${row.name === me ? " is-me" : ""}`}>
          <span className={`leaderboard__rank leaderboard__rank--${index + 1}`}>{index === 0 ? <Icon icon={Crown} size={16} /> : index + 1}</span>
          <Avatar name={row.name} size={34} />
          <span className="leaderboard__name">{row.name}{row.name === me && <small> (vous)</small>}</span>
          <b className="leaderboard__score">{format(row.score)}</b>
        </li>
      ))}
    </ol>
  );
}

/** Des avatars qui se chevauchent (les membres d'un groupe), « +3 » au-delà de `max`. */
export function AvatarGroup({ names, max = 4, size = 34 }: { names: string[]; max?: number; size?: number }) {
  const shown = names.slice(0, max);
  const rest = names.length - shown.length;
  return (
    <span className="avatar-group" role="img" aria-label={names.join(", ")}>
      {shown.map((name) => <Avatar key={name} name={name} size={size} />)}
      {rest > 0 && <span className="avatar avatar-group__more" style={{ width: size, height: size }}>+{rest}</span>}
    </span>
  );
}

/** Un texte où les mots cherchés sont surlignés (sans tenir compte des accents ni des
 * majuscules) : <Highlight text={op.label} query={recherche} />. */
export function Highlight({ text, query }: { text: string; query: string }) {
  const plain = (value: string) => value.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
  const words = query.trim().split(/\s+/).filter(Boolean).map(plain);
  if (!words.length) return <>{text}</>;
  const flat = plain(text);
  const marks = new Array<boolean>(text.length).fill(false);
  for (const word of words) {
    let at = flat.indexOf(word);
    while (at !== -1) { for (let i = at; i < at + word.length; i++) marks[i] = true; at = flat.indexOf(word, at + word.length); }
  }
  const pieces: ReactNode[] = [];
  let start = 0;
  for (let i = 1; i <= text.length; i++) {
    if (i === text.length || marks[i] !== marks[start]) {
      const piece = text.slice(start, i);
      pieces.push(marks[start] ? <mark key={start}>{piece}</mark> : piece);
      start = i;
    }
  }
  return <>{pieces}</>;
}

export type WizardStep = { title: string; content: ReactNode; check?: () => string | void };

/** Un formulaire en plusieurs étapes (identité, adresse, confirmation) : les étapes en haut,
 * Retour et Suivant en bas. `check` d'une étape rend une erreur à afficher, ou rien. */
export function Wizard({ steps, onFinish, finish = "Valider" }: { steps: WizardStep[]; onFinish: () => void; finish?: string }) {
  const [index, setIndex] = useState(0);
  const [error, setError] = useState("");
  const still = useReducedMotion();
  const step = steps[index];
  const last = index === steps.length - 1;
  const next = (event: FormEvent) => {
    event.preventDefault();
    const problem = step.check?.();
    if (problem) { setError(problem); return; }
    setError("");
    if (last) onFinish(); else setIndex(index + 1);
  };
  return (
    <form className="wizard" onSubmit={next} noValidate>
      <ol className="steps wizard__steps" aria-label="Étapes">
        {steps.map((item, number) => (
          <li key={item.title} className={number < index ? "is-done" : number === index ? "is-current" : ""} aria-current={number === index ? "step" : undefined}>
            <span className="steps__dot">{number < index ? <Icon icon={Check} size={14} /> : number + 1}</span><span className="wizard__name">{item.title}</span>
          </li>
        ))}
      </ol>
      <motion.div key={index} className="wizard__body" initial={{ opacity: 0, x: still ? 0 : 24 }} animate={{ opacity: 1, x: 0 }} transition={{ duration: 0.22, ease: EASE }}>
        <h3 className="wizard__title">{step.title}</h3>
        {step.content}
      </motion.div>
      {error && <p className="form__error" role="alert">{error}</p>}
      <div className="btn-row">
        {index > 0 && <button type="button" className="btn" onClick={() => { setError(""); setIndex(index - 1); }}>Retour</button>}
        <button type="submit" className="btn btn--primary">{last ? finish : "Suivant"}</button>
      </div>
    </form>
  );
}
