// Les blocs des autres genres d'applis : son et vidéo, photo avant/après ; santé (anneaux
// d'activité, humeur du jour) ; école (quiz) ; jeu (roue de la chance) ; journée (planning
// heure par heure, horloge) ; et la tenue de l'appli : écrans qui s'empilent avec un retour,
// glisser pour confirmer, remonter en haut, clair ou sombre, numéro de téléphone.

import { AnimatePresence, animate, motion, useMotionValue, useReducedMotion, useTransform } from "framer-motion";
import { Angry, ArrowLeft, ArrowUp, Check, ChevronRight, Frown, Laugh, Meh, Monitor, Moon, Pause, Play, Smile, Sun, X } from "lucide-react";
import { createContext, useContext, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { Icon, type LucideIcon } from "./icons";

const EASE = [0.2, 0.9, 0.3, 1] as const;
const clock = (seconds: number) => {
  if (!Number.isFinite(seconds)) return "–:––";
  const whole = Math.max(0, Math.floor(seconds || 0));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
};

// --- Son, vidéo, photos ---------------------------------------------------------------------

/** Un lecteur de son (un podcast, une méditation, un message vocal) : lecture, pause, barre
 * de position à toucher, temps passé et durée. */
export function AudioPlayer({ src, title, artist }: { src: string; title: string; artist?: string }) {
  const audio = useRef<HTMLAudioElement>(null);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [length, setLength] = useState(0);
  const toggle = () => { const player = audio.current; if (!player) return; if (player.paused) player.play().catch(() => setPlaying(false)); else player.pause(); };
  return (
    <div className="audio">
      <audio ref={audio} src={src} preload="metadata" onPlay={() => setPlaying(true)} onPause={() => setPlaying(false)} onEnded={() => setPlaying(false)}
        onTimeUpdate={(event) => setTime(event.currentTarget.currentTime)} onLoadedMetadata={(event) => setLength(event.currentTarget.duration)} />
      <button className="audio__play" type="button" aria-label={playing ? "Pause" : "Lecture"} onClick={toggle}><Icon icon={playing ? Pause : Play} size={22} /></button>
      <div className="audio__main">
        <span className="audio__title"><b>{title}</b>{artist && <small>{artist}</small>}</span>
        <input className="audio__seek" type="range" min={0} max={Number.isFinite(length) ? length : 0} step={0.1} value={time} aria-label="Position"
          style={{ "--value": `${Number.isFinite(length) && length ? (time / length) * 100 : 0}%` } as React.CSSProperties}
          onChange={(event) => { if (audio.current) audio.current.currentTime = Number(event.target.value); setTime(Number(event.target.value)); }} />
        <span className="audio__time"><span>{clock(time)}</span><span>{clock(length)}</span></span>
      </div>
    </div>
  );
}

/** Une vidéo avec son image d'attente et les commandes du téléphone ; elle se lit dans la
 * page sur iPhone. */
export function VideoPlayer({ src, poster, label }: { src: string; poster?: string; label: string }) {
  return <video className="video" src={src} poster={poster} controls playsInline preload="metadata" aria-label={label} />;
}

/** Deux photos superposées (avant et après), séparées par une poignée qu'on fait glisser. */
export function ImageCompare({ before, after, label, beforeLabel = "Avant", afterLabel = "Après" }: {
  before: string; after: string; label: string; beforeLabel?: string; afterLabel?: string;
}) {
  const [split, setSplit] = useState(50);
  return (
    <figure className="compare-img" aria-label={label}>
      <img src={after} alt={afterLabel} />
      <img src={before} alt={beforeLabel} className="compare-img__before" style={{ clipPath: `inset(0 ${100 - split}% 0 0)` }} />
      <span className="compare-img__line" style={{ left: `${split}%` }} aria-hidden="true"><i /></span>
      <span className="compare-img__tag compare-img__tag--before">{beforeLabel}</span>
      <span className="compare-img__tag compare-img__tag--after">{afterLabel}</span>
      <input className="compare-img__range" type="range" min={0} max={100} value={split} aria-label={`${label} : part de « ${beforeLabel} »`} onChange={(event) => setSplit(Number(event.target.value))} />
    </figure>
  );
}

// --- Santé ----------------------------------------------------------------------------------

/** Des anneaux d'activité l'un dans l'autre (bouger, exercice, debout ; ou eau, sommeil…),
 * chacun de 0 à 1 et au-delà quand l'objectif est dépassé. */
export function ActivityRings({ rings, size = 160 }: { rings: { label: string; value: number; color?: string; text?: string }[]; size?: number }) {
  const still = useReducedMotion();
  const colors = ["#e5484d", "#46a758", "#0090ff", "#f5a524"];
  const stroke = Math.max(10, size / 12);
  return (
    <div className="rings">
      <svg viewBox={`0 0 ${size} ${size}`} width={size} height={size} role="img" aria-label={rings.map((ring) => `${ring.label} ${Math.round(ring.value * 100)} %`).join(", ")}>
        {rings.map((ring, index) => {
          const radius = size / 2 - stroke / 2 - index * (stroke + 3);
          const color = ring.color ?? colors[index % colors.length];
          return (
            <g key={ring.label} transform={`rotate(-90 ${size / 2} ${size / 2})`}>
              <circle cx={size / 2} cy={size / 2} r={radius} fill="none" stroke={color} strokeOpacity={0.18} strokeWidth={stroke} />
              <motion.circle cx={size / 2} cy={size / 2} r={radius} fill="none" stroke={color} strokeWidth={stroke} strokeLinecap="round"
                pathLength={1} strokeDasharray="1 1" initial={{ strokeDashoffset: still ? 1 - Math.min(1, ring.value) : 1 }}
                animate={{ strokeDashoffset: 1 - Math.min(1, Math.max(0, ring.value)) }} transition={{ duration: 1, delay: index * 0.15, ease: EASE }} />
            </g>
          );
        })}
      </svg>
      <ul className="rings__legend">
        {rings.map((ring, index) => (
          <li key={ring.label}><i style={{ background: ring.color ?? colors[index % colors.length] }} />{ring.label}<b>{ring.text ?? `${Math.round(ring.value * 100)} %`}</b></li>
        ))}
      </ul>
    </div>
  );
}

const MOODS: { value: number; label: string; icon: LucideIcon }[] = [
  { value: 1, label: "Très mal", icon: Angry }, { value: 2, label: "Pas bien", icon: Frown }, { value: 3, label: "Bof", icon: Meh },
  { value: 4, label: "Bien", icon: Smile }, { value: 5, label: "Très bien", icon: Laugh },
];

/** L'humeur du jour en un toucher, de 1 (très mal) à 5 (très bien). */
export function MoodPicker({ value, onChange, label = "Comment ça va ?" }: { value: number | null; onChange: (value: number) => void; label?: string }) {
  return (
    <fieldset className="mood">
      <legend className="field__label">{label}</legend>
      <div className="mood__faces">
        {MOODS.map((mood) => (
          <button key={mood.value} type="button" className={`mood__face mood__face--${mood.value}${value === mood.value ? " is-picked" : ""}`} aria-pressed={value === mood.value} onClick={() => onChange(mood.value)}>
            <motion.span animate={{ scale: value === mood.value ? 1.15 : 1 }} style={{ display: "grid" }}><Icon icon={mood.icon} size={28} /></motion.span>
            <small>{mood.label}</small>
          </button>
        ))}
      </div>
    </fieldset>
  );
}

/** Le nom d'une humeur (1 à 5) et son icône, pour l'historique. */
export const moodOf = (value: number) => MOODS.find((mood) => mood.value === Math.round(value)) ?? MOODS[2];

// --- École et jeux --------------------------------------------------------------------------

export type QuizQuestion = { question: string; options: string[]; answer: number; why?: string };

/** Un quiz : une question à la fois, la bonne réponse en vert (la mauvaise en rouge) et son
 * explication, puis le score et « Recommencer ». `onDone` reçoit le score. */
export function Quiz({ questions, onDone }: { questions: QuizQuestion[]; onDone?: (score: number) => void }) {
  const [index, setIndex] = useState(0);
  const [picked, setPicked] = useState<number | null>(null);
  const [score, setScore] = useState(0);
  const finished = index >= questions.length;
  if (finished) {
    return (
      <div className="quiz quiz--end" role="status">
        <b className="quiz__score">{score} / {questions.length}</b>
        <p>{score === questions.length ? "Parfait !" : score >= questions.length / 2 ? "Bien joué." : "Encore un essai ?"}</p>
        <button className="btn btn--primary" type="button" onClick={() => { setIndex(0); setPicked(null); setScore(0); }}>Recommencer</button>
      </div>
    );
  }
  const question = questions[index];
  const choose = (option: number) => {
    if (picked !== null) return;
    setPicked(option);
    if (option === question.answer) setScore(score + 1);
  };
  const next = () => {
    const last = index === questions.length - 1;
    setIndex(index + 1);
    setPicked(null);
    if (last) onDone?.(score);
  };
  return (
    <div className="quiz">
      <div className="quiz__progress" aria-hidden="true"><i style={{ width: `${(index / questions.length) * 100}%` }} /></div>
      <small className="quiz__count">Question {index + 1} sur {questions.length}</small>
      <motion.h3 key={index} className="quiz__question" initial={{ opacity: 0, x: 20 }} animate={{ opacity: 1, x: 0 }}>{question.question}</motion.h3>
      <div className="quiz__options">
        {question.options.map((option, number) => {
          const state = picked === null ? "" : number === question.answer ? " is-right" : number === picked ? " is-wrong" : " is-off";
          return (
            <button key={option} type="button" className={`quiz__option${state}`} disabled={picked !== null} onClick={() => choose(number)}>
              <span>{option}</span>
              {picked !== null && number === question.answer && <Icon icon={Check} size={18} />}
              {picked === number && number !== question.answer && <Icon icon={X} size={18} />}
            </button>
          );
        })}
      </div>
      {picked !== null && (
        <motion.div className="quiz__after" initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }}>
          <p role="status">{picked === question.answer ? "Bonne réponse." : `La réponse était : ${question.options[question.answer]}.`}{question.why ? ` ${question.why}` : ""}</p>
          <button className="btn btn--primary btn--block" type="button" onClick={next}>{index === questions.length - 1 ? "Voir le score" : "Suivante"}<Icon icon={ChevronRight} size={18} /></button>
        </motion.div>
      )}
    </div>
  );
}

/** Une roue de la chance qui tourne et s'arrête sur une part au hasard (qui paie, quel
 * resto, quelle tâche). `onResult` reçoit la part tirée. */
export function SpinWheel({ items, onResult, label = "Tourner" }: { items: string[]; onResult: (item: string) => void; label?: string }) {
  const rotation = useMotionValue(0);
  const [spinning, setSpinning] = useState(false);
  const still = useReducedMotion();
  const colors = ["var(--accent)", "#e0803a", "#2f9e6e", "#d0508a", "#3d8bd9", "#a07a2c", "#7a6fd0", "#5f7480"];
  const slice = 360 / Math.max(1, items.length);
  const spin = () => {
    if (spinning || !items.length) return;
    const winner = Math.floor(Math.random() * items.length);
    // La part gagnante s'arrête sous le repère du haut.
    const target = rotation.get() - (rotation.get() % 360) + 360 * (still ? 1 : 5) + (360 - (winner * slice + slice / 2));
    setSpinning(true);
    animate(rotation, target, { duration: still ? 0.01 : 3.2, ease: [0.12, 0.8, 0.2, 1] }).then(() => { setSpinning(false); onResult(items[winner]); });
  };
  const turn = useTransform(rotation, (value) => `rotate(${value}deg)`);
  const point = (angle: number, radius: number) => [50 + radius * Math.sin((angle * Math.PI) / 180), 50 - radius * Math.cos((angle * Math.PI) / 180)];
  return (
    <div className="wheel">
      <span className="wheel__pointer" aria-hidden="true" />
      <motion.svg className="wheel__disc" viewBox="0 0 100 100" style={{ transform: turn }} aria-hidden="true">
        {items.map((item, index) => {
          const [x1, y1] = point(index * slice, 48);
          const [x2, y2] = point((index + 1) * slice, 48);
          const [tx, ty] = point(index * slice + slice / 2, 30);
          return (
            <g key={item + index}>
              <path d={items.length === 1 ? "M50 2 A48 48 0 1 1 49.99 2 Z" : `M50 50 L${x1} ${y1} A48 48 0 ${slice > 180 ? 1 : 0} 1 ${x2} ${y2} Z`} fill={colors[index % colors.length]} />
              <text x={tx} y={ty} transform={`rotate(${index * slice + slice / 2} ${tx} ${ty})`} textAnchor="middle" dominantBaseline="middle" className="wheel__text">{item.length > 12 ? `${item.slice(0, 11)}…` : item}</text>
            </g>
          );
        })}
        <circle cx="50" cy="50" r="7" className="wheel__hub" />
      </motion.svg>
      <button className="btn btn--primary" type="button" onClick={spin} disabled={spinning || !items.length}>{spinning ? "…" : label}</button>
    </div>
  );
}

// --- Journée --------------------------------------------------------------------------------

export type Slot = { id: string; start: string; end: string; title: string; note?: string; tone?: "accent" | "in" | "out" | "warn" };

/** Le planning d'une journée heure par heure (de `from` à `to`), les rendez-vous posés à
 * leur place, et la ligne de l'heure qu'il est quand c'est aujourd'hui. */
export function DaySchedule({ slots, from = 8, to = 20, now }: { slots: Slot[]; from?: number; to?: number; now?: boolean }) {
  const hour = 56;
  const minutes = (time: string) => { const [h, m] = time.split(":").map(Number); return (h - from) * 60 + (m || 0); };
  const [current, setCurrent] = useState(() => new Date());
  useEffect(() => { if (!now) return; const timer = setInterval(() => setCurrent(new Date()), 60000); return () => clearInterval(timer); }, [now]);
  const nowLine = (current.getHours() - from) * 60 + current.getMinutes();
  return (
    <div className="schedule" style={{ height: (to - from) * hour }}>
      {Array.from({ length: to - from }, (_, index) => (
        <span key={index} className="schedule__hour" style={{ top: index * hour }}>{String(from + index).padStart(2, "0")}:00</span>
      ))}
      {slots.map((slot) => {
        const top = (minutes(slot.start) / 60) * hour;
        const height = Math.max(28, ((minutes(slot.end) - minutes(slot.start)) / 60) * hour - 3);
        return (
          <div key={slot.id} className={`schedule__slot schedule__slot--${slot.tone ?? "accent"}`} style={{ top, height }}>
            <b>{slot.title}</b><small>{slot.start} – {slot.end}{slot.note ? ` · ${slot.note}` : ""}</small>
          </div>
        );
      })}
      {now && nowLine >= 0 && nowLine <= (to - from) * 60 && <i className="schedule__now" style={{ top: (nowLine / 60) * hour }} aria-label="Maintenant" />}
    </div>
  );
}

/** Une horloge à aiguilles qui tourne (l'heure d'ici, ou d'un autre fuseau : `zone`
 * "America/New_York"), avec son nom dessous. */
export function AnalogClock({ zone, label, size = 120 }: { zone?: string; label?: string; size?: number }) {
  const [time, setTime] = useState(() => new Date());
  useEffect(() => { const timer = setInterval(() => setTime(new Date()), 1000); return () => clearInterval(timer); }, []);
  const parts = new Intl.DateTimeFormat("fr-FR", { timeZone: zone, hour: "numeric", minute: "numeric", second: "numeric", hourCycle: "h23" }).formatToParts(time);
  const get = (type: string) => Number(parts.find((part) => part.type === type)?.value ?? 0);
  const [h, m, s] = [get("hour"), get("minute"), get("second")];
  const text = `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
  const hand = (angle: number, length: number, className: string) => (
    <line x1="50" y1="50" x2={50 + length * Math.sin((angle * Math.PI) / 180)} y2={50 - length * Math.cos((angle * Math.PI) / 180)} className={className} />
  );
  return (
    <figure className="analog">
      <svg viewBox="0 0 100 100" width={size} height={size} role="img" aria-label={`${label ?? "Heure"} : ${text}`}>
        <circle cx="50" cy="50" r="47" className="analog__face" />
        {Array.from({ length: 12 }, (_, index) => hand(index * 30, 44, "analog__tick"))}
        {hand(((h % 12) + m / 60) * 30, 24, "analog__hour")}
        {hand((m + s / 60) * 6, 34, "analog__minute")}
        {hand(s * 6, 38, "analog__second")}
        <circle cx="50" cy="50" r="3" className="analog__pin" />
      </svg>
      {label && <figcaption><b>{label}</b><span>{text}</span></figcaption>}
    </figure>
  );
}

// --- Tenue de l'appli -----------------------------------------------------------------------

type Stack = { push: (page: string) => void; back: () => void; page: string; depth: number };
const StackContext = createContext<Stack>({ push: () => {}, back: () => {}, page: "", depth: 0 });

/** Des écrans qui s'empilent dans un onglet (une liste, puis le détail, puis la modification),
 * chacun glissant par-dessus le précédent, avec le bouton retour et le retour du téléphone.
 * `pages` : un écran par nom ; `useStack()` dans un écran donne push("nom") et back(). */
export function StackView({ pages, first }: { pages: Record<string, (page: string) => ReactNode>; first: string }) {
  const [history, setHistory] = useState([first]);
  const [forward, setForward] = useState(true);
  const still = useReducedMotion();
  const page = history[history.length - 1];
  const stack: Stack = {
    page, depth: history.length,
    push: (next) => { setForward(true); setHistory((all) => [...all, next]); window.history.pushState({ claw: "stack" }, ""); },
    back: () => { if (history.length > 1) window.history.back(); },
  };
  useEffect(() => {
    const pop = () => { setForward(false); setHistory((all) => (all.length > 1 ? all.slice(0, -1) : all)); };
    window.addEventListener("popstate", pop);
    return () => window.removeEventListener("popstate", pop);
  }, []);
  const name = page.split(":")[0];
  return (
    <StackContext.Provider value={stack}>
      <div className="stack">
        <AnimatePresence mode="popLayout" initial={false} custom={forward}>
          <motion.div key={`${history.length}-${page}`} className="stack__page"
            initial={{ x: still ? 0 : forward ? "100%" : "-25%", opacity: still ? 0 : 1 }} animate={{ x: 0, opacity: 1 }}
            exit={{ x: still ? 0 : forward ? "-25%" : "100%", opacity: still ? 0 : forward ? 0.6 : 1 }} transition={{ duration: 0.3, ease: EASE }}>
            {(pages[name] ?? pages[first])(page)}
          </motion.div>
        </AnimatePresence>
      </div>
    </StackContext.Provider>
  );
}

/** Dans un écran de StackView : push("détail:42") ouvre un écran (ce qui suit « : » lui est
 * passé), back() revient, page est l'écran montré. */
export const useStack = () => useContext(StackContext);

/** Le haut d'un écran empilé : la flèche retour et son titre. */
export function BackBar({ title, children }: { title: ReactNode; children?: ReactNode }) {
  const { back, depth } = useStack();
  return (
    <div className="back-bar">
      {depth > 1 && <button className="icon-btn" type="button" aria-label="Retour" onClick={back}><Icon icon={ArrowLeft} /></button>}
      <h2 className="back-bar__title">{title}</h2>
      {children}
    </div>
  );
}

/** Glisser pour confirmer (payer, envoyer, terminer), comme le déverrouillage d'un téléphone :
 * la pastille doit aller au bout. Au clavier, Entrée confirme. */
export function SlideToConfirm({ label, onConfirm, done = "C'est fait" }: { label: string; onConfirm: () => void; done?: string }) {
  const track = useRef<HTMLDivElement>(null);
  const x = useMotionValue(0);
  const [confirmed, setConfirmed] = useState(false);
  const fade = useTransform(x, [0, 120], [1, 0]);
  const finish = () => { setConfirmed(true); onConfirm(); };
  const release = () => {
    const width = (track.current?.offsetWidth ?? 0) - 56;
    if (x.get() > width * 0.85) { animate(x, width, { duration: 0.15 }); finish(); }
    else animate(x, 0, { type: "spring", stiffness: 400, damping: 30 });
  };
  useEffect(() => { if (!confirmed) x.set(0); }, [confirmed, x]);
  return (
    <div ref={track} className={`slide-confirm${confirmed ? " is-done" : ""}`}>
      <motion.span className="slide-confirm__label" style={{ opacity: confirmed ? 1 : fade }}>{confirmed ? done : label}</motion.span>
      {!confirmed && (
        <motion.button type="button" className="slide-confirm__knob" aria-label={`${label} (Entrée pour confirmer)`} drag="x" dragConstraints={track} dragElastic={0} dragMomentum={false}
          style={{ x }} onDragEnd={release} onKeyDown={(event) => { if (event.key === "Enter") finish(); }}>
          <Icon icon={ChevronRight} size={24} />
        </motion.button>
      )}
      {confirmed && <span className="slide-confirm__check"><Icon icon={Check} size={22} /></span>}
    </div>
  );
}

/** Le bouton rond « remonter en haut », qui apparaît après un long défilement. */
export function ScrollTopButton({ after = 600 }: { after?: number }) {
  const [shown, setShown] = useState(false);
  const still = useReducedMotion();
  useEffect(() => {
    const check = () => setShown(window.scrollY > after);
    check();
    window.addEventListener("scroll", check, { passive: true });
    return () => window.removeEventListener("scroll", check);
  }, [after]);
  return (
    <AnimatePresence>
      {shown && (
        <motion.button className="scroll-top" type="button" aria-label="Remonter en haut" initial={{ opacity: 0, scale: 0.6 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0, scale: 0.6 }}
          onClick={() => window.scrollTo({ top: 0, behavior: still ? "auto" : "smooth" })}>
          <Icon icon={ArrowUp} />
        </motion.button>
      )}
    </AnimatePresence>
  );
}

type Theme = "auto" | "light" | "dark";
const readTheme = (): Theme => { try { const saved = localStorage.getItem("appli-theme"); return saved === "light" || saved === "dark" ? saved : "auto"; } catch { return "auto"; } };
const applyTheme = (theme: Theme) => {
  if (theme === "auto") delete document.documentElement.dataset.theme; else document.documentElement.dataset.theme = theme;
  try { localStorage.setItem("appli-theme", theme); } catch { /* navigation privée */ }
};
if (typeof document !== "undefined") applyTheme(readTheme());

/** Clair, sombre ou comme le téléphone : l'appli entière change, et s'en souvient. */
export function ThemeToggle({ label = "Apparence" }: { label?: string }) {
  const [theme, setTheme] = useState<Theme>(readTheme);
  const choices: [Theme, string, LucideIcon][] = [["light", "Clair", Sun], ["dark", "Sombre", Moon], ["auto", "Auto", Monitor]];
  return (
    <div className="theme-toggle" role="radiogroup" aria-label={label}>
      {choices.map(([value, name, icon]) => (
        <button key={value} type="button" role="radio" aria-checked={theme === value} className={theme === value ? "is-on" : ""} onClick={() => { setTheme(value); applyTheme(value); }}>
          {theme === value && <motion.span layoutId="theme-toggle-pill" className="theme-toggle__pill" transition={{ duration: 0.25, ease: EASE }} />}
          <span className="theme-toggle__text"><Icon icon={icon} size={18} />{name}</span>
        </button>
      ))}
    </div>
  );
}

/** Un numéro de téléphone français, écrit au fur et à mesure « 06 12 34 56 78 » ; `onChange`
 * reçoit les chiffres seuls. Un numéro incomplet est signalé en quittant le champ. */
export function PhoneField({ label = "Téléphone", value, onChange }: { label?: string; value: string; onChange: (digits: string) => void }) {
  const id = useId();
  const [left, setLeft] = useState(false);
  const digits = value.replace(/\D/g, "").replace(/^33/, "0").slice(0, 10);
  const shown = digits.replace(/(\d{2})(?=\d)/g, "$1 ");
  const wrong = left && digits.length > 0 && digits.length < 10;
  return (
    <div className={`field${wrong ? " is-invalid" : ""}`}>
      <label htmlFor={id}>{label}</label>
      <input id={id} type="tel" inputMode="tel" autoComplete="tel-national" placeholder="06 12 34 56 78" value={shown} aria-invalid={wrong}
        onChange={(event) => onChange(event.target.value.replace(/\D/g, "").replace(/^33/, "0").slice(0, 10))} onBlur={() => setLeft(true)} />
      {wrong && <p className="field__error">Un numéro a 10 chiffres.</p>}
    </div>
  );
}
