// Gestes et animations encore : carte qui se retourne, bouton à maintenir, bouton + qui
// s'ouvre en plusieurs actions, cœur « j'aime » qui éclate, écrans d'accueil du premier
// lancement, visite guidée, « stories » qui avancent seules, appui long. Toutes se calment
// quand le téléphone demande moins de mouvement.

import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { Heart, Plus, X } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Icon, type LucideIcon } from "./icons";

const EASE = [0.2, 0.9, 0.3, 1] as const;

/** Lit et écrit une valeur sur le téléphone, sans planter quand il le refuse. */
function remembered(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}
function remember(key: string, value: string): void {
  try { localStorage.setItem(key, value); } catch { /* navigation privée */ }
}

/** Une carte à deux faces (une question et sa réponse, un mot et sa traduction) : un toucher
 * la retourne. `flipped` et `onFlip` la pilotent de l'extérieur si besoin. */
export function FlipCard({ front, back, label, flipped, onFlip }: {
  front: ReactNode; back: ReactNode; label: string; flipped?: boolean; onFlip?: (flipped: boolean) => void;
}) {
  const [own, setOwn] = useState(false);
  const shown = flipped ?? own;
  const still = useReducedMotion();
  const turn = () => { setOwn(!shown); onFlip?.(!shown); };
  return (
    <button type="button" className="flip-card" onClick={turn} aria-label={`${label} : ${shown ? "voir le recto" : "retourner"}`} aria-pressed={shown}>
      <motion.span className="flip-card__inner" animate={{ rotateY: shown ? 180 : 0 }} transition={still ? { duration: 0 } : { duration: 0.5, ease: EASE }}>
        <span className="flip-card__face" aria-hidden={shown}>{front}</span>
        <span className="flip-card__face flip-card__face--back" aria-hidden={!shown}>{back}</span>
      </motion.span>
    </button>
  );
}

/** Un appui long (`ms`) sur un élément : `useLongPress(() => …)` rend les propriétés à poser
 * dessus (<div {...useLongPress(ouvrir)}>). Un glissement annule. */
export function useLongPress(onLongPress: () => void, ms = 550) {
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const start = useRef<[number, number]>([0, 0]);
  const cancel = useCallback(() => { if (timer.current) clearTimeout(timer.current); timer.current = null; }, []);
  useEffect(() => cancel, [cancel]);
  return {
    onPointerDown: (event: React.PointerEvent) => {
      start.current = [event.clientX, event.clientY];
      cancel();
      timer.current = setTimeout(() => { timer.current = null; onLongPress(); }, ms);
    },
    onPointerMove: (event: React.PointerEvent) => {
      if (Math.hypot(event.clientX - start.current[0], event.clientY - start.current[1]) > 10) cancel();
    },
    onPointerUp: cancel,
    onPointerLeave: cancel,
    onContextMenu: (event: React.MouseEvent) => event.preventDefault(),
  };
}

/** Un bouton qu'il faut maintenir (`seconds`) pour un geste qui ne s'annule pas (tout effacer,
 * envoyer) : il se remplit sous le doigt, et relâché trop tôt il se vide. Entrée ou Espace
 * maintenus font pareil au clavier. */
export function HoldButton({ children, onDone, seconds = 1.2, tone = "danger" }: {
  children: ReactNode; onDone: () => void; seconds?: number; tone?: "danger" | "primary";
}) {
  const [holding, setHolding] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const stop = () => { setHolding(false); if (timer.current) clearTimeout(timer.current); timer.current = null; };
  const begin = () => {
    if (timer.current) return;
    setHolding(true);
    timer.current = setTimeout(() => { timer.current = null; setHolding(false); onDone(); }, seconds * 1000);
  };
  useEffect(() => () => { if (timer.current) clearTimeout(timer.current); }, []);
  return (
    <button type="button" className={`btn btn--${tone} btn--block hold${holding ? " is-holding" : ""}`} style={{ "--hold-time": `${seconds}s` } as React.CSSProperties}
      onPointerDown={begin} onPointerUp={stop} onPointerLeave={stop} onPointerCancel={stop} onContextMenu={(event) => event.preventDefault()}
      onKeyDown={(event) => { if ((event.key === "Enter" || event.key === " ") && !event.repeat) { event.preventDefault(); begin(); } }}
      onKeyUp={(event) => { if (event.key === "Enter" || event.key === " ") stop(); }}>
      <span className="hold__fill" aria-hidden="true" />
      <span className="hold__text">{children}</span>
      <span className="visually-hidden">(maintenir appuyé)</span>
    </button>
  );
}

/** Le bouton rond en bas à droite qui s'ouvre en plusieurs actions (une dépense, une
 * rentrée, un virement). Pour une seule action, le + de AppShell suffit. */
export function SpeedDial({ actions, label = "Ajouter" }: { actions: { label: string; icon: LucideIcon; onClick: () => void }[]; label?: string }) {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!open) return;
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") setOpen(false); };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [open]);
  return (
    <div className={`dial${open ? " is-open" : ""}`}>
      <AnimatePresence>
        {open && <motion.div className="dial__backdrop" onClick={() => setOpen(false)} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} />}
      </AnimatePresence>
      <AnimatePresence>
        {open && (
          <motion.ul className="dial__actions" initial="hidden" animate="shown" exit="hidden" variants={{ shown: { transition: { staggerChildren: 0.04, staggerDirection: -1 } }, hidden: {} }}>
            {actions.map((action) => (
              <motion.li key={action.label} variants={{ hidden: { opacity: 0, y: 12, scale: 0.9 }, shown: { opacity: 1, y: 0, scale: 1 } }}>
                <button type="button" className="dial__action" onClick={() => { setOpen(false); action.onClick(); }}>
                  <span>{action.label}</span><span className="dial__icon"><Icon icon={action.icon} /></span>
                </button>
              </motion.li>
            ))}
          </motion.ul>
        )}
      </AnimatePresence>
      <button type="button" className="dial__main" aria-expanded={open} aria-label={open ? "Fermer" : label} onClick={() => setOpen(!open)}>
        <motion.span animate={{ rotate: open ? 45 : 0 }} style={{ display: "grid" }}><Icon icon={open ? X : Plus} size={26} /></motion.span>
      </button>
    </div>
  );
}

/** Un cœur « j'aime » (ou une autre icône) qui éclate en petites bulles quand on l'allume. */
export function LikeButton({ liked, onChange, label = "J'aime", count, icon = Heart }: {
  liked: boolean; onChange: (liked: boolean) => void; label?: string; count?: number; icon?: LucideIcon;
}) {
  const still = useReducedMotion();
  const [burst, setBurst] = useState(0);
  return (
    <button type="button" className={`like${liked ? " is-liked" : ""}`} aria-pressed={liked} aria-label={label}
      onClick={() => { if (!liked) setBurst((n) => n + 1); onChange(!liked); }}>
      <motion.span className="like__icon" animate={liked && !still ? { scale: [1, 1.35, 1] } : { scale: 1 }} transition={{ duration: 0.35 }} key={burst}>
        <Icon icon={icon} size={22} />
      </motion.span>
      {!still && burst > 0 && liked && (
        <span className="like__burst" key={`b${burst}`} aria-hidden="true">
          {Array.from({ length: 8 }, (_, index) => (
            <motion.i key={index} initial={{ x: 0, y: 0, opacity: 1, scale: 1 }}
              animate={{ x: Math.cos((index / 8) * Math.PI * 2) * 22, y: Math.sin((index / 8) * Math.PI * 2) * 22, opacity: 0, scale: 0.4 }}
              transition={{ duration: 0.55, ease: "easeOut" }} />
          ))}
        </span>
      )}
      {count !== undefined && <span className="like__count">{count}</span>}
    </button>
  );
}

export type OnboardingSlide = { title: string; text: ReactNode; icon?: LucideIcon };

/** Les écrans d'accueil du premier lancement (2 à 4), avec leurs points, « Passer » et
 * « Commencer » ; ils ne reviennent plus ensuite (retenu sous `id` sur le téléphone). */
export function Onboarding({ id = "appli-accueil", slides, done = "Commencer" }: { id?: string; slides: OnboardingSlide[]; done?: string }) {
  const [open, setOpen] = useState(() => remembered(id) !== "vu");
  const [index, setIndex] = useState(0);
  const [direction, setDirection] = useState(1);
  const still = useReducedMotion();
  if (!open || !slides.length) return null;
  const close = () => { remember(id, "vu"); setOpen(false); };
  const go = (next: number) => { setDirection(next > index ? 1 : -1); setIndex(next); };
  const slide = slides[index];
  const last = index === slides.length - 1;
  return (
    <div className="onboarding" role="dialog" aria-modal="true" aria-label="Bienvenue">
      <button type="button" className="onboarding__skip" onClick={close}>Passer</button>
      <AnimatePresence mode="wait" initial={false} custom={direction}>
        <motion.div key={index} className="onboarding__slide" drag={still ? false : "x"} dragConstraints={{ left: 0, right: 0 }}
          onDragEnd={(_, info) => { if (info.offset.x < -60 && !last) go(index + 1); if (info.offset.x > 60 && index > 0) go(index - 1); }}
          initial={{ opacity: 0, x: still ? 0 : 50 * direction }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: still ? 0 : -50 * direction }} transition={{ duration: 0.25, ease: EASE }}>
          {slide.icon && <span className="onboarding__icon"><Icon icon={slide.icon} size={44} /></span>}
          <h2 className="onboarding__title">{slide.title}</h2>
          <p className="onboarding__text">{slide.text}</p>
        </motion.div>
      </AnimatePresence>
      <div className="onboarding__foot">
        <div className="onboarding__dots" role="tablist" aria-label="Écrans">
          {slides.map((item, dot) => (
            <button key={item.title} type="button" role="tab" aria-selected={dot === index} aria-label={`Écran ${dot + 1}`} onClick={() => go(dot)}><i /></button>
          ))}
        </div>
        <button type="button" className="btn btn--primary btn--block" onClick={() => (last ? close() : go(index + 1))}>{last ? done : "Suivant"}</button>
      </div>
    </div>
  );
}

export type TourStep = { target: string; title: string; text: ReactNode };

/** Une visite guidée : chaque étape éclaire un élément de l'écran (`target` : un sélecteur
 * CSS, comme "#ajouter" ou "[data-tour=solde]") et l'explique dans une bulle. Elle ne se
 * montre qu'une fois (retenue sous `id`), sauf si `open` la rouvre. */
export function Tour({ id = "appli-visite", steps, open: asked, onClose }: { id?: string; steps: TourStep[]; open?: boolean; onClose?: () => void }) {
  const [open, setOpen] = useState(() => asked ?? remembered(id) !== "vu");
  const [index, setIndex] = useState(0);
  const [box, setBox] = useState<DOMRect | null>(null);
  useEffect(() => { if (asked) { setIndex(0); setOpen(true); } }, [asked]);
  const step = steps[index];
  useLayoutEffect(() => {
    if (!open || !step) return;
    const place = () => {
      const element = document.querySelector(step.target);
      setBox(element ? element.getBoundingClientRect() : null);
    };
    document.querySelector(step.target)?.scrollIntoView({ block: "center", behavior: "instant" as ScrollBehavior });
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => { window.removeEventListener("resize", place); window.removeEventListener("scroll", place, true); };
  }, [open, step]);
  if (!open || !step) return null;
  const close = () => { remember(id, "vu"); setOpen(false); onClose?.(); };
  const pad = 6;
  const below = !box || box.bottom < window.innerHeight * 0.6;
  return (
    <div className="tour" role="dialog" aria-modal="true" aria-label={step.title}>
      <div className="tour__shade" onClick={close} style={box ? { clipPath: `polygon(0 0, 100% 0, 100% 100%, 0 100%, 0 ${box.top - pad}px, ${box.left - pad}px ${box.top - pad}px, ${box.left - pad}px ${box.bottom + pad}px, ${box.right + pad}px ${box.bottom + pad}px, ${box.right + pad}px ${box.top - pad}px, 0 ${box.top - pad}px)` } : undefined} />
      {box && <div className="tour__ring" style={{ top: box.top - pad, left: box.left - pad, width: box.width + pad * 2, height: box.height + pad * 2 }} />}
      <motion.div key={index} className="tour__bubble" initial={{ opacity: 0, y: below ? -8 : 8 }} animate={{ opacity: 1, y: 0 }}
        style={box ? (below ? { top: box.bottom + pad + 12 } : { bottom: window.innerHeight - box.top + pad + 12 }) : { top: "40%" }}>
        <small>{index + 1} / {steps.length}</small>
        <b>{step.title}</b>
        <p>{step.text}</p>
        <div className="btn-row">
          <button type="button" className="btn btn--small" onClick={close}>Fermer</button>
          <button type="button" className="btn btn--primary btn--small" onClick={() => (index < steps.length - 1 ? setIndex(index + 1) : close())}>
            {index < steps.length - 1 ? "Suivant" : "Terminé"}
          </button>
        </div>
      </motion.div>
    </div>
  );
}

/** Des « stories » : des pages qui avancent seules (`seconds` chacune) avec leurs barres en
 * haut ; un toucher à gauche recule, à droite avance, un appui maintenu met en pause. */
export function Stories({ items, seconds = 5, onEnd, label }: { items: ReactNode[]; seconds?: number; onEnd?: () => void; label: string }) {
  const [index, setIndex] = useState(0);
  const [paused, setPaused] = useState(false);
  const still = useReducedMotion();
  const next = useCallback(() => setIndex((current) => {
    if (current < items.length - 1) return current + 1;
    onEnd?.();
    return current;
  }), [items.length, onEnd]);
  useEffect(() => {
    if (paused || still) return;
    const timer = setTimeout(next, seconds * 1000);
    return () => clearTimeout(timer);
  }, [index, paused, still, seconds, next]);
  return (
    <div className="stories" role="region" aria-roledescription="stories" aria-label={label}
      onPointerDown={() => setPaused(true)} onPointerUp={() => setPaused(false)} onPointerLeave={() => setPaused(false)}>
      <div className="stories__bars" aria-hidden="true">
        {items.map((_, bar) => (
          <span key={bar} className="stories__bar">
            <i className={bar < index ? "is-done" : bar === index && !still ? "is-running" : ""}
              style={bar === index ? { animationDuration: `${seconds}s`, animationPlayState: paused ? "paused" : "running" } : undefined} key={`${bar}-${index}`} />
          </span>
        ))}
      </div>
      <div className="stories__page" aria-live="polite">{items[index]}</div>
      <button type="button" className="stories__tap stories__tap--back" aria-label="Précédente" onClick={() => setIndex(Math.max(0, index - 1))} />
      <button type="button" className="stories__tap stories__tap--next" aria-label="Suivante" onClick={next} />
    </div>
  );
}
