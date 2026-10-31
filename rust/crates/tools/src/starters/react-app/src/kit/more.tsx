// Widgets de téléphone : ligne à glisser, liste de tâches, étiquettes, choix d'une couleur et
// d'une icône, minuteur, chronomètre, année en carrés (comme GitHub), anneau de répartition,
// mini-courbe, liste à réordonner, photo, partage, invitation à installer l'appli. Habillés par
// app-kit.css, comme le reste du kit.

import { animate, motion, Reorder, useDragControls, useMotionValue, useTransform } from "framer-motion";
import {
  ArrowDown, ArrowUp, Baby, Bike, BookOpen, Brain, Briefcase, Camera, Car, Check, Coffee, Dog, Download, Droplet,
  Dumbbell, Flag, Gamepad2, Gift, GripVertical, Heart, House, Leaf, Moon, Music, Pause, Pencil, PiggyBank, Pill, Plane,
  Play, RotateCcw, Share2, ShoppingCart, Smartphone, Star, Sun, Trash2, Utensils, Wallet, X, Zap,
} from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent, type ReactNode } from "react";
import { List, Row, type Tone } from "./blocks";
import { lastDays, longDayLabel, shortMonth, today } from "./format";
import { AddField } from "./forms";
import { Icon, type LucideIcon } from "./icons";
import { celebrate, ProgressRing } from "./motion";
import { toast } from "./toast";

type RowLike = Parameters<typeof Row>[0];

/** Une ligne qu'on glisse au doigt : vers la gauche pour supprimer, vers la droite pour
 * marquer fait. Les deux restent des boutons pour le clavier et les lecteurs d'écran. */
export function SwipeRow({ onDelete, onDone, deleteLabel = "Supprimer", doneLabel = "Fait", ...row }: RowLike & {
  onDelete?: () => void; onDone?: () => void; deleteLabel?: string; doneLabel?: string;
}) {
  const x = useMotionValue(0);
  const leftShown = useTransform(x, [0, 72], [0, 1]);
  const rightShown = useTransform(x, [-72, 0], [1, 0]);
  return (
    <li className="swipe">
      {onDone && <motion.span className="swipe__under swipe__under--done" style={{ opacity: leftShown }} aria-hidden="true"><Icon icon={Check} />{doneLabel}</motion.span>}
      {onDelete && <motion.span className="swipe__under swipe__under--delete" style={{ opacity: rightShown }} aria-hidden="true">{deleteLabel}<Icon icon={Trash2} /></motion.span>}
      <motion.div
        className="swipe__front"
        style={{ x }}
        drag="x"
        dragDirectionLock
        dragConstraints={{ left: onDelete ? -120 : 0, right: onDone ? 120 : 0 }}
        dragElastic={0.15}
        onDragEnd={(_, info) => {
          const far = info.offset.x;
          if (far < -80 && onDelete) onDelete();
          else if (far > 80 && onDone) onDone();
          animate(x, 0, { type: "spring", stiffness: 400, damping: 35 });
        }}
      >
        <List><Row {...row} /></List>
      </motion.div>
      <span className="visually-hidden">
        {onDone && <button type="button" onClick={onDone}>{doneLabel}</button>}
        {onDelete && <button type="button" onClick={onDelete}>{deleteLabel}</button>}
      </span>
    </li>
  );
}

export type Task = { id: string; text: string; done: boolean };

/** Une liste de tâches à cocher : ajouter, cocher, supprimer, et l'avancement (« 2 sur 5 »).
 * Des confettis quand tout est fait. */
export function Checklist({ items, onChange, label = "Une tâche", emptyText = "Rien à faire pour l'instant." }: {
  items: Task[]; onChange: (items: Task[]) => void; label?: string; emptyText?: ReactNode;
}) {
  const done = items.filter((item) => item.done).length;
  const toggle = (id: string) => {
    const next = items.map((item) => (item.id === id ? { ...item, done: !item.done } : item));
    onChange(next);
    if (next.length > 0 && next.every((item) => item.done) && done < items.length) celebrate();
  };
  return (
    <div className="checklist">
      {items.length > 0 && (
        <div className="checklist__progress" aria-live="polite">
          <span>{done} sur {items.length}</span>
          <span className="checklist__bar"><i style={{ width: `${(done / items.length) * 100}%` }} /></span>
        </div>
      )}
      {items.length === 0 && <p className="checklist__empty">{emptyText}</p>}
      <ul className="checklist__list">
        {items.map((item) => (
          <motion.li key={item.id} layout="position" className={`checklist__item${item.done ? " is-done" : ""}`} initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }}>
            <label>
              <input type="checkbox" checked={item.done} onChange={() => toggle(item.id)} />
              <span className="checklist__box" aria-hidden="true"><Icon icon={Check} size={14} /></span>
              <span className="checklist__text">{item.text}</span>
            </label>
            <button type="button" className="icon-btn" aria-label={`Supprimer ${item.text}`} onClick={() => onChange(items.filter((other) => other.id !== item.id))}>
              <Icon icon={X} size={16} />
            </button>
          </motion.li>
        ))}
      </ul>
      <AddField
        label={label}
        onAdd={(text) => {
          if (items.some((item) => item.text.toLowerCase() === text.toLowerCase())) return `« ${text} » est déjà dans la liste.`;
          onChange([...items, { id: crypto.randomUUID(), text, done: false }]);
        }}
      />
    </div>
  );
}

/** Des étiquettes à taper (Entrée ou virgule pour en ajouter une, ✕ pour l'enlever), avec
 * des suggestions à toucher. */
export function TagsField({ label, value, onChange, suggestions = [], placeholder = "Ajouter…" }: {
  label: string; value: string[]; onChange: (tags: string[]) => void; suggestions?: string[]; placeholder?: string;
}) {
  const id = useId();
  const [draft, setDraft] = useState("");
  const add = (text: string) => {
    const tag = text.trim().replace(/,$/, "").trim();
    if (tag && !value.some((other) => other.toLowerCase() === tag.toLowerCase())) onChange([...value, tag]);
    setDraft("");
  };
  const onKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter" || event.key === ",") {
      event.preventDefault();
      add(draft);
    } else if (event.key === "Backspace" && !draft && value.length) onChange(value.slice(0, -1));
  };
  const left = suggestions.filter((tag) => !value.some((other) => other.toLowerCase() === tag.toLowerCase()));
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      <div className="tags">
        {value.map((tag) => (
          <span key={tag} className="tags__tag">
            {tag}
            <button type="button" aria-label={`Enlever ${tag}`} onClick={() => onChange(value.filter((other) => other !== tag))}><Icon icon={X} size={14} /></button>
          </span>
        ))}
        <input id={id} value={draft} placeholder={value.length ? "" : placeholder} enterKeyHint="done" autoComplete="off"
          onChange={(event) => (event.target.value.endsWith(",") ? add(event.target.value) : setDraft(event.target.value))} onKeyDown={onKey} onBlur={() => draft && add(draft)} />
      </div>
      {left.length > 0 && (
        <div className="tags__suggest">
          {left.map((tag) => <button key={tag} type="button" className="chip" onClick={() => add(tag)}>+ {tag}</button>)}
        </div>
      )}
    </div>
  );
}

export const COLORS = ["#6d5dfc", "#2f80ed", "#0ea5a4", "#22a06b", "#84cc16", "#f2b705", "#f97316", "#e5484d", "#e93d82", "#8e4ec6", "#64748b", "#a16207"];

/** Le choix d'une couleur parmi des pastilles (la couleur d'une catégorie, d'une habitude). */
export function ColorPicker({ label, value, onChange, colors = COLORS }: { label: string; value: string; onChange: (color: string) => void; colors?: string[] }) {
  const name = useId();
  return (
    <fieldset className="swatches">
      <legend className="field__label">{label}</legend>
      <div className="swatches__grid">
        {colors.map((color, at) => (
          <label key={color} className="swatch" style={{ "--swatch": color } as CSSProperties}>
            <input type="radio" name={name} value={color} checked={value === color} onChange={() => onChange(color)} aria-label={`Couleur ${at + 1}`} />
            <span aria-hidden="true"><Icon icon={Check} size={16} /></span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}

/** Les icônes à choisir, par nom : le nom se garde dans les données, iconByName le redessine. */
export const ICONS: Record<string, LucideIcon> = {
  wallet: Wallet, house: House, cart: ShoppingCart, utensils: Utensils, coffee: Coffee, car: Car, bike: Bike, plane: Plane,
  phone: Smartphone, game: Gamepad2, music: Music, book: BookOpen, brain: Brain, briefcase: Briefcase, piggy: PiggyBank,
  gift: Gift, heart: Heart, pill: Pill, dumbbell: Dumbbell, droplet: Droplet, leaf: Leaf, sun: Sun, moon: Moon, star: Star,
  flag: Flag, zap: Zap, dog: Dog, baby: Baby, pencil: Pencil, camera: Camera,
};

export const iconByName = (name: string | undefined): LucideIcon => (name && ICONS[name]) || Star;

/** Le choix d'une icône dans une grille ; `value` est son nom (« book »), à garder tel quel. */
export function IconPicker({ label, value, onChange, names = Object.keys(ICONS), color }: {
  label: string; value: string; onChange: (name: string) => void; names?: string[]; color?: string;
}) {
  const group = useId();
  return (
    <fieldset className="icon-picker" style={color ? ({ "--swatch": color } as CSSProperties) : undefined}>
      <legend className="field__label">{label}</legend>
      <div className="icon-picker__grid">
        {names.map((name) => (
          <label key={name} className="icon-picker__item">
            <input type="radio" name={group} value={name} checked={value === name} onChange={() => onChange(name)} aria-label={name} />
            <span><Icon icon={iconByName(name)} /></span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}

const clock = (seconds: number) => {
  const whole = Math.max(0, Math.round(seconds));
  const hours = Math.floor(whole / 3600);
  const rest = `${String(Math.floor(whole / 60) % 60).padStart(2, "0")}:${String(whole % 60).padStart(2, "0")}`;
  return hours ? `${hours}:${rest}` : rest;
};

/** Un minuteur (méditation, pomodoro, cuisson) : l'anneau se vide, Démarrer, Pause, Recommencer ;
 * `onDone` quand le temps est écoulé (le téléphone vibre). Le temps compte même en arrière-plan. */
export function Timer({ minutes, label = "Minuteur", onDone }: { minutes: number; label?: string; onDone?: () => void }) {
  const total = Math.max(1, Math.round(minutes * 60));
  const [left, setLeft] = useState(total);
  const [endsAt, setEndsAt] = useState<number | null>(null);
  const done = useRef(onDone);
  done.current = onDone;
  useEffect(() => { setLeft(total); setEndsAt(null); }, [total]);
  useEffect(() => {
    if (endsAt === null) return;
    const tick = () => {
      const remaining = Math.max(0, (endsAt - Date.now()) / 1000);
      setLeft(remaining);
      if (remaining <= 0) {
        setEndsAt(null);
        navigator.vibrate?.([200, 100, 200]);
        done.current?.();
      }
    };
    tick();
    const timer = setInterval(tick, 250);
    return () => clearInterval(timer);
  }, [endsAt]);
  const running = endsAt !== null;
  return (
    <div className="timer">
      <ProgressRing value={left / total} label={label} size={168}><span className="timer__time" role="timer" aria-live="off">{clock(Math.ceil(left))}</span></ProgressRing>
      <div className="timer__buttons">
        <button type="button" className="btn btn--primary" onClick={() => (running ? (setEndsAt(null)) : setEndsAt(Date.now() + (left > 0 ? left : total) * 1000))}>
          <Icon icon={running ? Pause : Play} size={18} />{running ? "Pause" : left < total && left > 0 ? "Reprendre" : "Démarrer"}
        </button>
        <button type="button" className="btn" disabled={!running && left === total} onClick={() => { setEndsAt(null); setLeft(total); }}>
          <Icon icon={RotateCcw} size={18} />Recommencer
        </button>
      </div>
    </div>
  );
}

/** Un chronomètre avec ses tours (course, révisions). */
export function Stopwatch({ onLap }: { onLap?: (seconds: number) => void }) {
  const [base, setBase] = useState(0);
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [laps, setLaps] = useState<number[]>([]);
  useEffect(() => {
    if (startedAt === null) return;
    const timer = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(timer);
  }, [startedAt]);
  const elapsed = base + (startedAt === null ? 0 : (now - startedAt) / 1000);
  const tenths = Math.floor((elapsed % 1) * 10);
  return (
    <div className="stopwatch">
      <b className="stopwatch__time" role="timer">{clock(Math.floor(elapsed))}<small>,{tenths}</small></b>
      <div className="timer__buttons">
        <button type="button" className="btn btn--primary" onClick={() => {
          if (startedAt === null) { setStartedAt(Date.now()); setNow(Date.now()); } else { setBase(elapsed); setStartedAt(null); }
        }}>
          <Icon icon={startedAt === null ? Play : Pause} size={18} />{startedAt === null ? (elapsed ? "Reprendre" : "Démarrer") : "Pause"}
        </button>
        {startedAt !== null
          ? <button type="button" className="btn" onClick={() => { setLaps([elapsed, ...laps]); onLap?.(elapsed); }}><Icon icon={Flag} size={18} />Tour</button>
          : <button type="button" className="btn" disabled={!elapsed} onClick={() => { setBase(0); setLaps([]); }}><Icon icon={RotateCcw} size={18} />Remettre à zéro</button>}
      </div>
      {laps.length > 0 && (
        <ol className="stopwatch__laps" reversed>
          {laps.map((lap, at) => <li key={laps.length - at}><span>Tour {laps.length - at}</span><span>{clock(lap - (laps[at + 1] ?? 0))}</span></li>)}
        </ol>
      )}
    </div>
  );
}

/** Les derniers mois en carrés, un par jour, plus foncés les jours les plus remplis (comme les
 * contributions de GitHub). `values` : { "2026-10-28": 2 } ou la liste des jours faits. */
export function YearHeatmap({ values, weeks = 20, label }: { values: Record<string, number> | string[]; weeks?: number; label: string }) {
  const counts = useMemo(() => (Array.isArray(values) ? Object.fromEntries(values.map((day) => [day, 1])) : values), [values]);
  const end = today();
  // Des colonnes de lundi à dimanche : la dernière s'arrête à aujourd'hui.
  const offset = (new Date(`${end}T12:00:00`).getDay() + 6) % 7;
  const days = lastDays(weeks * 7 - 6 + offset, end);
  const top = Math.max(1, ...Object.values(counts));
  const scroller = useRef<HTMLDivElement>(null);
  useEffect(() => { scroller.current?.scrollTo({ left: scroller.current.scrollWidth }); }, []);
  const level = (day: string) => {
    const count = counts[day] ?? 0;
    return count <= 0 ? 0 : Math.min(4, Math.ceil((count / top) * 4));
  };
  return (
    <figure className="heatmap" aria-label={label}>
      <div className="heatmap__scroll" ref={scroller}>
        <div className="heatmap__grid" style={{ gridTemplateColumns: `repeat(${weeks}, 14px)` }}>
          {days.map((day, at) => (
            <span key={day} className={`heatmap__day is-${level(day)}`} style={{ gridColumn: Math.floor(at / 7) + 1, gridRow: (at % 7) + 1 }} title={`${longDayLabel(day)} : ${counts[day] ?? 0}`} />
          ))}
          {days.filter((day, at) => at % 7 === 0 && day.slice(8) <= "07").map((day) => (
            <small key={`m${day}`} className="heatmap__month" style={{ gridColumn: Math.floor(days.indexOf(day) / 7) + 1, gridRow: 8 }}>{shortMonth(day.slice(0, 7))}</small>
          ))}
        </div>
      </div>
      <figcaption className="heatmap__legend">Moins {[0, 1, 2, 3, 4].map((step) => <span key={step} className={`heatmap__day is-${step}`} />)} Plus</figcaption>
    </figure>
  );
}

/** Un anneau de répartition (les dépenses par catégorie), sa légende et un total au centre. */
export function DonutChart({ data, label, center, format = (value) => String(value) }: {
  data: { name: string; value: number; color?: string }[]; label: string; center?: ReactNode; format?: (value: number) => string;
}) {
  const shown = data.filter((item) => item.value > 0);
  const sum = shown.reduce((total, item) => total + item.value, 0);
  let start = 0;
  return (
    <figure className="donut">
      <div className="donut__ring">
        <svg viewBox="0 0 36 36" role="img" aria-label={`${label} : ${shown.map((item) => `${item.name} ${format(item.value)}`).join(", ")}`}>
          <circle cx="18" cy="18" r="15.9155" pathLength={100} className="donut__track" />
          {shown.map((item, at) => {
            const part = (item.value / sum) * 100;
            const arc = (
              <motion.circle key={item.name} cx="18" cy="18" r="15.9155" pathLength={100} className="donut__part"
                stroke={item.color ?? COLORS[at % COLORS.length]} strokeDashoffset={-start}
                initial={{ strokeDasharray: "0 100" }} animate={{ strokeDasharray: `${Math.max(0, part - (shown.length > 1 ? 0.8 : 0))} 100` }}
                transition={{ duration: 0.6, delay: at * 0.05, ease: "easeOut" }} />
            );
            start += part;
            return arc;
          })}
        </svg>
        {center && <span className="donut__center">{center}</span>}
      </div>
      <figcaption>
        <ul className="donut__legend">
          {shown.map((item, at) => (
            <li key={item.name}>
              <i style={{ background: item.color ?? COLORS[at % COLORS.length] }} />
              <span>{item.name}</span>
              <b>{format(item.value)}</b>
              <small>{Math.round((item.value / sum) * 100)} %</small>
            </li>
          ))}
        </ul>
      </figcaption>
    </figure>
  );
}

/** Une mini-courbe sans axes, pour une tendance à côté d'un chiffre (le poids, le solde). */
export function Sparkline({ values, label, tone = "accent", width = 120, height = 36 }: {
  values: number[]; label: string; tone?: Tone; width?: number; height?: number;
}) {
  if (values.length < 2) return null;
  const low = Math.min(...values);
  const high = Math.max(...values);
  const span = high - low || 1;
  const points = values.map((value, at) => [(at / (values.length - 1)) * width, height - 3 - ((value - low) / span) * (height - 6)]);
  const line = points.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
  const [lastX, lastY] = points[points.length - 1];
  return (
    <svg className={`sparkline sparkline--${tone}`} width={width} height={height} viewBox={`0 0 ${width} ${height}`} role="img" aria-label={label}>
      <polygon points={`0,${height} ${line} ${width},${height}`} className="sparkline__area" />
      <motion.polyline points={line} className="sparkline__line" initial={{ pathLength: 0 }} animate={{ pathLength: 1 }} transition={{ duration: 0.6 }} />
      <circle cx={lastX} cy={lastY} r="3" className="sparkline__dot" />
    </svg>
  );
}

function SortableItem<T>({ item, label, children, move, index, count }: {
  item: T; label: string; children: ReactNode; move: (from: number, to: number) => void; index: number; count: number;
}) {
  const controls = useDragControls();
  return (
    <Reorder.Item value={item} dragListener={false} dragControls={controls} className="sortable__item">
      <span className="sortable__handle" onPointerDown={(event) => { event.preventDefault(); controls.start(event); }} aria-hidden="true"><Icon icon={GripVertical} size={18} /></span>
      <div className="sortable__content">{children}</div>
      <span className="sortable__moves">
        <button type="button" className="icon-btn" aria-label={`Monter ${label}`} disabled={index === 0} onClick={() => move(index, index - 1)}><Icon icon={ArrowUp} size={16} /></button>
        <button type="button" className="icon-btn" aria-label={`Descendre ${label}`} disabled={index === count - 1} onClick={() => move(index, index + 1)}><Icon icon={ArrowDown} size={16} /></button>
      </span>
    </Reorder.Item>
  );
}

/** Une liste qu'on réordonne en tirant la poignée (ou avec les flèches) : priorités, étapes. */
export function SortableList<T>({ items, onReorder, itemKey, itemLabel, render }: {
  items: T[]; onReorder: (items: T[]) => void; itemKey: (item: T) => string; itemLabel: (item: T) => string; render: (item: T) => ReactNode;
}) {
  const move = (from: number, to: number) => {
    const next = [...items];
    next.splice(to, 0, ...next.splice(from, 1));
    onReorder(next);
  };
  return (
    <Reorder.Group axis="y" values={items} onReorder={onReorder} className="sortable">
      {items.map((item, index) => (
        <SortableItem key={itemKey(item)} item={item} label={itemLabel(item)} move={move} index={index} count={items.length}>{render(item)}</SortableItem>
      ))}
    </Reorder.Group>
  );
}

/** Réduit une photo à `size` pixels au plus et la rend en texte (data:image/jpeg), à garder
 * avec les données de l'appli sur le téléphone. */
export async function shrinkPhoto(file: File, size = 512): Promise<string> {
  const bitmap = await createImageBitmap(file);
  const scale = Math.min(1, size / Math.max(bitmap.width, bitmap.height));
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(bitmap.width * scale);
  canvas.height = Math.round(bitmap.height * scale);
  canvas.getContext("2d")?.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close();
  return canvas.toDataURL("image/jpeg", 0.82);
}

/** Une photo à prendre ou à choisir (un ticket, un profil), réduite et montrée ; `value` est le
 * texte rendu par shrinkPhoto, vide sans photo. */
export function PhotoField({ label, value, onChange, round, size = 512 }: {
  label: string; value: string; onChange: (photo: string) => void; round?: boolean; size?: number;
}) {
  const input = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  return (
    <div className="field">
      <span className="field__label">{label}</span>
      <div className={`photo${round ? " photo--round" : ""}`}>
        <button type="button" className="photo__frame" aria-label={value ? `Changer : ${label}` : `Ajouter : ${label}`} onClick={() => input.current?.click()}>
          {value ? <img src={value} alt="" /> : <Icon icon={Camera} size={26} />}
        </button>
        {value && <button type="button" className="btn btn--small" onClick={() => onChange("")}><Icon icon={Trash2} size={16} />Retirer</button>}
        {busy && <span className="field__help">Préparation…</span>}
      </div>
      <input ref={input} type="file" accept="image/*" hidden onChange={async (event) => {
        const file = event.target.files?.[0];
        event.target.value = "";
        if (!file) return;
        setBusy(true);
        try {
          onChange(await shrinkPhoto(file, size));
        } catch {
          toast("Cette image ne s'ouvre pas", { tone: "error" });
        } finally {
          setBusy(false);
        }
      }} />
    </div>
  );
}

/** Partage un texte ou un lien par le menu du téléphone (Messages, WhatsApp…) ; sans ce menu,
 * il le copie. */
export function ShareButton({ title, text, url, label = "Partager" }: { title?: string; text?: string; url?: string; label?: string }) {
  const share = async () => {
    if (navigator.share) {
      try {
        await navigator.share({ title, text, url });
      } catch {
        // Partage annulé.
      }
      return;
    }
    try {
      await navigator.clipboard.writeText([text, url].filter(Boolean).join(" "));
      toast("Copié : collez-le où vous voulez");
    } catch {
      toast("Partage impossible sur cet appareil", { tone: "error" });
    }
  };
  return <button type="button" className="btn" onClick={share}><Icon icon={Share2} size={18} />{label}</button>;
}

type InstallEvent = Event & { prompt: () => Promise<void>; userChoice: Promise<{ outcome: string }> };

/** Invite à installer l'appli sur l'écran d'accueil (sur iPhone : la marche à suivre). Rien
 * quand elle est déjà installée ou que l'utilisateur a dit non. */
export function InstallBanner({ name = "l'appli" }: { name?: string }) {
  const [event, setEvent] = useState<InstallEvent | null>(null);
  const [hidden, setHidden] = useState(() => {
    try {
      return localStorage.getItem("kit-install-dismissed") === "1";
    } catch {
      return false;
    }
  });
  useEffect(() => {
    const keep = (prompt: Event) => { prompt.preventDefault(); setEvent(prompt as InstallEvent); };
    window.addEventListener("beforeinstallprompt", keep);
    return () => window.removeEventListener("beforeinstallprompt", keep);
  }, []);
  const standalone = window.matchMedia("(display-mode: standalone)").matches || (navigator as { standalone?: boolean }).standalone === true;
  const apple = /iPhone|iPad|iPod/.test(navigator.userAgent);
  if (hidden || standalone || (!event && !apple)) return null;
  const close = () => {
    setHidden(true);
    try { localStorage.setItem("kit-install-dismissed", "1"); } catch { /* navigation privée */ }
  };
  return (
    <div className="install" role="region" aria-label="Installer">
      <span className="install__icon"><Icon icon={Download} /></span>
      <span className="install__text">
        <b>Installer {name}</b>
        {event ? <span>Sur l'écran d'accueil, sans connexion.</span> : <span>Touchez Partager, puis « Sur l'écran d'accueil ».</span>}
      </span>
      {event && <button type="button" className="btn btn--primary btn--small" onClick={async () => { await event.prompt(); await event.userChoice; setEvent(null); }}>Installer</button>}
      <button type="button" className="icon-btn" aria-label="Plus tard" onClick={close}><Icon icon={X} size={16} /></button>
    </div>
  );
}
