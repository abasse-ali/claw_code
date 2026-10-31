// Encore des animations, faites avec framer-motion : des mots qui apparaissent un à un, des
// lettres qui se mélangent puis se posent, un fond aux couleurs qui bougent, une lumière qui
// suit le doigt, un bouton aimanté, une icône qui se secoue pour qu'on la remarque, une carte
// qui s'agrandit en plein écran, une liste dont les lignes arrivent et partent en glissant, la
// barre de chargement du haut, des étoiles qui scintillent, un cercle de respiration, les
// trois points « en train d'écrire », un verre qui se remplit, un trait qui se dessine, de la
// neige (ou des cœurs, des feuilles) qui tombe. Toutes se calment quand le téléphone demande
// moins de mouvement.

import { AnimatePresence, animate, motion, useMotionTemplate, useMotionValue, useReducedMotion, useSpring } from "framer-motion";
import { X } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { Icon } from "./icons";

const EASE = [0.2, 0.9, 0.3, 1] as const;

/** Une phrase dont les mots apparaissent un à un, en sortant d'un léger flou (un titre
 * d'accueil, un message de réussite). `onScroll` : quand on l'atteint en faisant défiler. */
export function TextReveal({ text, gap = 0.06, onScroll, className }: { text: string; gap?: number; onScroll?: boolean; className?: string }) {
  const still = useReducedMotion();
  const words = text.split(/(\s+)/);
  const shown = { opacity: 1, y: 0, filter: "blur(0px)" };
  return (
    <span className={`text-reveal${className ? ` ${className}` : ""}`} aria-label={text}>
      {words.map((word, index) => /^\s+$/.test(word) ? word : (
        <motion.span key={index} aria-hidden="true" className="text-reveal__word"
          initial={still ? false : { opacity: 0, y: 8, filter: "blur(6px)" }}
          animate={onScroll ? undefined : shown} whileInView={onScroll ? shown : undefined} viewport={{ once: true }}
          transition={{ duration: 0.45, delay: (index / 2) * gap, ease: EASE }}>
          {word}
        </motion.span>
      ))}
    </span>
  );
}

const SIGNS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789#%&*+";

/** Un texte dont les lettres se mélangent puis se posent une à une (un code, un résultat,
 * un nom révélé). Rejoue quand `text` change. */
export function ScrambleText({ text, duration = 0.9, className }: { text: string; duration?: number; className?: string }) {
  const still = useReducedMotion();
  const [shown, setShown] = useState(text);
  useEffect(() => {
    if (still) { setShown(text); return; }
    const start = performance.now();
    let frame = 0;
    const tick = (now: number) => {
      const done = Math.min(1, (now - start) / (duration * 1000));
      const fixed = Math.floor(done * text.length);
      setShown([...text].map((char, index) => index < fixed || char === " " ? char : SIGNS[Math.floor(Math.random() * SIGNS.length)]).join(""));
      if (done < 1) frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [text, duration, still]);
  return <span className={`scramble${className ? ` ${className}` : ""}`} aria-label={text}><span aria-hidden="true">{shown}</span></span>;
}

/** Un fond aux couleurs de l'appli qui bougent lentement (derrière un écran d'accueil, une
 * carte de solde) ; ses enfants passent devant. */
export function Aurora({ children, className }: { children?: ReactNode; className?: string }) {
  const still = useReducedMotion();
  const blob = (index: number) => still ? undefined : {
    x: [0, index % 2 ? 40 : -40, 0], y: [0, index === 1 ? -30 : 30, 0], scale: [1, 1.15, 1],
  };
  return (
    <div className={`aurora${className ? ` ${className}` : ""}`}>
      <div className="aurora__sky" aria-hidden="true">
        {[0, 1, 2].map((index) => (
          <motion.i key={index} className={`aurora__blob aurora__blob--${index + 1}`} animate={blob(index)}
            transition={{ duration: 12 + index * 4, repeat: Infinity, ease: "easeInOut" }} />
        ))}
      </div>
      <div className="aurora__content">{children}</div>
    </div>
  );
}

/** Une carte sur laquelle une lumière douce suit le doigt ou la souris. */
export function GlowCard({ children, className }: { children: ReactNode; className?: string }) {
  const x = useMotionValue(-200);
  const y = useMotionValue(-200);
  const light = useMotionTemplate`radial-gradient(220px circle at ${x}px ${y}px, color-mix(in srgb, var(--accent) 18%, transparent), transparent 70%)`;
  const still = useReducedMotion();
  return (
    <div className={`spotlight${className ? ` ${className}` : ""}`}
      onPointerMove={(event) => { const box = event.currentTarget.getBoundingClientRect(); x.set(event.clientX - box.left); y.set(event.clientY - box.top); }}
      onPointerLeave={() => { x.set(-200); y.set(-200); }}>
      {!still && <motion.span className="spotlight__light" style={{ background: light }} aria-hidden="true" />}
      {children}
    </div>
  );
}

/** Un élément (un bouton principal) attiré par le doigt ou la souris quand il s'approche,
 * qui revient en place en rebondissant. */
export function Magnetic({ children, strength = 0.35 }: { children: ReactNode; strength?: number }) {
  const still = useReducedMotion();
  const x = useSpring(0, { stiffness: 220, damping: 15 });
  const y = useSpring(0, { stiffness: 220, damping: 15 });
  if (still) return <>{children}</>;
  return (
    <motion.span className="magnetic" style={{ x, y }}
      onPointerMove={(event) => {
        const box = event.currentTarget.getBoundingClientRect();
        x.set((event.clientX - box.left - box.width / 2) * strength);
        y.set((event.clientY - box.top - box.height / 2) * strength);
      }}
      onPointerLeave={() => { x.set(0); y.set(0); }}>
      {children}
    </motion.span>
  );
}

/** Une icône ou un bouton qui se secoue de temps en temps (toutes les `every` secondes) pour
 * qu'on le remarque, tant que `active` (une notification non lue, un rappel). */
export function Wiggle({ children, active = true, every = 4 }: { children: ReactNode; active?: boolean; every?: number }) {
  const still = useReducedMotion();
  const on = active && !still;
  return (
    <motion.span className="wiggle" animate={on ? { rotate: [0, -14, 12, -8, 6, 0] } : { rotate: 0 }}
      transition={on ? { duration: 0.6, repeat: Infinity, repeatDelay: every, ease: "easeInOut" } : { duration: 0.2 }}>
      {children}
    </motion.span>
  );
}

/** Une carte (un article, une photo, une opération) qui s'agrandit en plein écran quand on la
 * touche, et y montre `detail` ; la croix ou Échap la remet à sa place. */
export function ExpandCard({ children, detail, label }: { children: ReactNode; detail: ReactNode; label: string }) {
  const [open, setOpen] = useState(false);
  const id = useId();
  const still = useReducedMotion();
  useEffect(() => {
    if (!open) return;
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") setOpen(false); };
    document.addEventListener("keydown", key);
    const scroll = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => { document.removeEventListener("keydown", key); document.body.style.overflow = scroll; };
  }, [open]);
  return (
    <>
      <motion.button type="button" layoutId={still ? undefined : `expand-${id}`} className="expand-card" onClick={() => setOpen(true)} aria-label={`Ouvrir : ${label}`}
        style={{ visibility: open && !still ? "hidden" : "visible" }} transition={{ duration: 0.35, ease: EASE }}>
        {children}
      </motion.button>
      <AnimatePresence>
        {open && (
          <motion.div className="expand-card__shade" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onClick={() => setOpen(false)} />
        )}
        {open && (
          <motion.div layoutId={still ? undefined : `expand-${id}`} className="expand-card__open" role="dialog" aria-modal="true" aria-label={label}
            initial={still ? { opacity: 0 } : undefined} animate={still ? { opacity: 1 } : undefined} exit={still ? { opacity: 0 } : undefined}
            transition={{ duration: 0.35, ease: EASE }}>
            <button className="icon-btn expand-card__close" type="button" aria-label="Fermer" onClick={() => setOpen(false)}><Icon icon={X} /></button>
            <div className="expand-card__head">{children}</div>
            <motion.div className="expand-card__detail" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.2, duration: 0.3 }}>{detail}</motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </>
  );
}

/** Une liste de blocs faits à la main (des cartes, des messages, des photos) qui arrivent en
 * glissant, partent en se repliant et se replacent en douceur quand l'ordre change.
 * `render` dessine un bloc, jamais un Row : une List de Row fait déjà tout cela. */
export function AnimatedList<T>({ items, itemKey, render, className }: { items: T[]; itemKey: (item: T) => string; render: (item: T) => ReactNode; className?: string }) {
  return (
    <ul className={`animated-list${className ? ` ${className}` : ""}`}>
      <AnimatePresence initial={false}>
        {items.map((item) => (
          <motion.li key={itemKey(item)} layout initial={{ opacity: 0, x: -24, height: 0 }} animate={{ opacity: 1, x: 0, height: "auto" }}
            exit={{ opacity: 0, x: 40, height: 0 }} transition={{ duration: 0.3, ease: EASE }} style={{ overflow: "hidden" }}>
            {render(item)}
          </motion.li>
        ))}
      </AnimatePresence>
    </ul>
  );
}

/** La fine barre de chargement en haut de l'écran, tant que `active` (une page qui se charge,
 * un envoi au serveur). */
export function LoadingBar({ active }: { active: boolean }) {
  return (
    <AnimatePresence>
      {active && (
        <motion.div className="loading-bar" role="progressbar" aria-label="Chargement" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0, transition: { delay: 0.15 } }}>
          <i />
        </motion.div>
      )}
    </AnimatePresence>
  );
}

/** Des petites étoiles qui scintillent autour d'un élément (un badge gagné, une offre). */
export function Twinkle({ children, count = 6 }: { children: ReactNode; count?: number }) {
  const still = useReducedMotion();
  const stars = useMemo(() => Array.from({ length: count }, (_, index) => ({
    left: `${8 + ((index * 37) % 84)}%`, top: `${6 + ((index * 53) % 80)}%`, size: 8 + ((index * 5) % 8), delay: (index * 0.37) % 1.8,
  })), [count]);
  return (
    <span className="sparkles">
      {!still && stars.map((star, index) => (
        <motion.svg key={index} className="sparkles__star" viewBox="0 0 24 24" aria-hidden="true" style={{ left: star.left, top: star.top, width: star.size, height: star.size }}
          animate={{ scale: [0, 1, 0], rotate: [0, 90, 180], opacity: [0, 1, 0] }} transition={{ duration: 1.6, delay: star.delay, repeat: Infinity, repeatDelay: 0.6 }}>
          <path d="M12 0 L14.5 9.5 L24 12 L14.5 14.5 L12 24 L9.5 14.5 L0 12 L9.5 9.5 Z" />
        </motion.svg>
      ))}
      <span className="sparkles__child">{children}</span>
    </span>
  );
}

/** Un cercle de respiration (méditation, calme) : il grandit pendant l'inspiration, tient,
 * puis rétrécit à l'expiration, avec la consigne au centre. Temps en secondes. */
export function Breathe({ inhale = 4, hold = 4, exhale = 6, running = true }: { inhale?: number; hold?: number; exhale?: number; running?: boolean }) {
  const [phase, setPhase] = useState<"in" | "hold" | "out">("in");
  const still = useReducedMotion();
  useEffect(() => {
    if (!running) { setPhase("in"); return; }
    const length = { in: inhale, hold, out: exhale }[phase];
    const timer = setTimeout(() => setPhase(phase === "in" ? (hold ? "hold" : "out") : phase === "hold" ? "out" : "in"), length * 1000);
    return () => clearTimeout(timer);
  }, [phase, running, inhale, hold, exhale]);
  const words = { in: "Inspirez", hold: "Retenez", out: "Expirez" };
  const scale = !running ? 0.7 : phase === "out" ? 0.7 : 1;
  return (
    <div className="breathe" role="timer" aria-live="polite">
      <motion.span className="breathe__circle" aria-hidden="true" animate={{ scale: still ? 1 : scale }}
        transition={{ duration: phase === "in" ? inhale : phase === "out" ? exhale : 0.3, ease: "easeInOut" }} />
      <span className="breathe__word">{running ? words[phase] : "Prêt"}</span>
    </div>
  );
}

/** Les trois points « en train d'écrire » d'une conversation. */
export function TypingDots({ label = "En train d'écrire" }: { label?: string }) {
  const still = useReducedMotion();
  return (
    <span className="typing-dots" role="status" aria-label={label}>
      {[0, 1, 2].map((index) => (
        <motion.i key={index} animate={still ? undefined : { y: [0, -4, 0], opacity: [0.4, 1, 0.4] }}
          transition={{ duration: 0.9, repeat: Infinity, delay: index * 0.15, ease: "easeInOut" }} />
      ))}
    </span>
  );
}

/** Un verre (ou une jauge ronde) qui se remplit d'eau en vagues jusqu'à `value` (0 à 1) :
 * l'eau bue dans la journée, une cagnotte. */
export function LiquidFill({ value, label, text, shape = "glass" }: { value: number; label: string; text?: ReactNode; shape?: "glass" | "circle" }) {
  const still = useReducedMotion();
  const level = Math.min(1, Math.max(0, value || 0));
  return (
    <div className={`liquid liquid--${shape}`} role="img" aria-label={`${label} : ${Math.round(level * 100)} %`}>
      <motion.div className="liquid__water" initial={{ height: "0%" }} animate={{ height: `${level * 100}%` }} transition={{ duration: still ? 0 : 1, ease: EASE }}>
        <motion.svg className="liquid__wave" viewBox="0 0 200 20" preserveAspectRatio="none" aria-hidden="true"
          animate={still ? undefined : { x: ["0%", "-50%"] }} transition={{ duration: 3, repeat: Infinity, ease: "linear" }}>
          <path d="M0 10 Q 25 0 50 10 T 100 10 T 150 10 T 200 10 V 20 H 0 Z" />
        </motion.svg>
      </motion.div>
      <span className="liquid__text">{text ?? `${Math.round(level * 100)} %`}</span>
    </div>
  );
}

/** Un trait SVG qui se dessine (une signature, un parcours, une flèche, un logo au trait).
 * `d` : le chemin SVG ; `viewBox` : sa boîte. */
export function DrawPath({ d, viewBox = "0 0 100 100", duration = 1.4, width = 3, label, onScroll }: {
  d: string; viewBox?: string; duration?: number; width?: number; label: string; onScroll?: boolean;
}) {
  const still = useReducedMotion();
  const shown = { pathLength: 1, opacity: 1 };
  return (
    <svg className="draw-path" viewBox={viewBox} role="img" aria-label={label}>
      <motion.path d={d} fill="none" stroke="currentColor" strokeWidth={width} strokeLinecap="round" strokeLinejoin="round"
        initial={still ? false : { pathLength: 0, opacity: 0 }} animate={onScroll ? undefined : shown} whileInView={onScroll ? shown : undefined}
        viewport={{ once: true }} transition={{ duration, ease: "easeInOut" }} />
    </svg>
  );
}

/** De la neige (ou des cœurs, des feuilles, des confettis) qui tombe doucement sur tout
 * l'écran, sans gêner les touchers : une fête, une saison. */
export function Particles({ kind = "snow", count = 24 }: { kind?: "snow" | "hearts" | "leaves" | "confetti"; count?: number }) {
  const still = useReducedMotion();
  const drops = useMemo(() => Array.from({ length: count }, (_, index) => ({
    left: (index * 61) % 100, size: 6 + ((index * 7) % 10), duration: 7 + ((index * 13) % 8), delay: -((index * 1.7) % 10), drift: ((index * 29) % 60) - 30,
    hue: (index * 47) % 360,
  })), [count]);
  if (still) return null;
  return (
    <div className={`particles particles--${kind}`} aria-hidden="true">
      {drops.map((drop, index) => (
        <motion.i key={index} style={{ left: `${drop.left}%`, width: drop.size, height: drop.size, ["--hue" as string]: drop.hue }}
          initial={{ y: "-10vh" }} animate={{ y: "110vh", x: [0, drop.drift, 0], rotate: kind === "snow" ? 0 : [0, 180, 360] }}
          transition={{ duration: drop.duration, delay: drop.delay, repeat: Infinity, ease: "linear" }} />
      ))}
    </div>
  );
}

/** Un nombre qui grossit et change de couleur un instant quand il augmente ou baisse
 * (+ vert, − rouge) : un score, un solde, un compteur de pas. */
export function Bump({ value, children }: { value: number; children: ReactNode }) {
  const previous = useRef(value);
  const scale = useMotionValue(1);
  const [tone, setTone] = useState<"" | "up" | "down">("");
  const still = useReducedMotion();
  useEffect(() => {
    if (value === previous.current) return;
    setTone(value > previous.current ? "up" : "down");
    previous.current = value;
    if (!still) animate(scale, [1, 1.18, 1], { duration: 0.4 });
    const timer = setTimeout(() => setTone(""), 700);
    return () => clearTimeout(timer);
  }, [value, still, scale]);
  return <motion.span className={`bump${tone ? ` bump--${tone}` : ""}`} style={{ scale }}>{children}</motion.span>;
}
