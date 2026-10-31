// Encore des animations, faites avec framer-motion : texte qui s'écrit, bandeau qui défile,
// carte qui s'incline sous le doigt, onde au toucher, reflet qui passe, chiffres qui tournent,
// profondeur au défilement, flottement, éclair quand une valeur change, pile de cartes à
// balayer, glissement entre deux pages, compteur animé de la page. Toutes s'arrêtent quand le
// téléphone demande moins de mouvement (useReducedMotion).

import { animate, AnimatePresence, motion, useMotionValue, useReducedMotion, useScroll, useSpring, useTransform } from "framer-motion";
import { useEffect, useRef, useState, type CSSProperties, type MouseEvent, type ReactNode } from "react";

const EASE = [0.2, 0.9, 0.3, 1] as const;

/** Un texte qui s'écrit lettre par lettre (`speed` : millisecondes par lettre), puis reste. */
export function Typewriter({ text, speed = 35, className }: { text: string; speed?: number; className?: string }) {
  const still = useReducedMotion();
  const [count, setCount] = useState(still ? text.length : 0);
  useEffect(() => {
    if (still) {
      setCount(text.length);
      return;
    }
    setCount(0);
    const timer = setInterval(() => setCount((shown) => (shown >= text.length ? shown : shown + 1)), speed);
    return () => clearInterval(timer);
  }, [text, speed, still]);
  return (
    <span className={`typewriter${className ? ` ${className}` : ""}`} aria-label={text}>
      <span aria-hidden="true">{text.slice(0, count)}</span>
      {count < text.length && <span className="typewriter__caret" aria-hidden="true" />}
    </span>
  );
}

/** Un bandeau qui défile sans fin (annonces, cours, conseils) ; il s'arrête sous le doigt et
 * quand le téléphone demande moins de mouvement. `seconds` : un tour complet. */
export function Marquee({ items, seconds = 20, label }: { items: ReactNode[]; seconds?: number; label: string }) {
  const still = useReducedMotion();
  return (
    <div className={`marquee${still ? " is-still" : ""}`} role="region" aria-label={label}>
      <div className="marquee__track" style={{ "--marquee-time": `${seconds}s` } as CSSProperties}>
        {[0, 1].map((copy) => (
          <div className="marquee__group" key={copy} aria-hidden={copy === 1}>
            {items.map((item, index) => <span className="marquee__item" key={index}>{item}</span>)}
          </div>
        ))}
      </div>
    </div>
  );
}

/** Une carte qui s'incline vers le doigt ou la souris, et revient en place. */
export function Tilt({ children, max = 8, className }: { children: ReactNode; max?: number; className?: string }) {
  const still = useReducedMotion();
  const x = useMotionValue(0.5);
  const y = useMotionValue(0.5);
  const rotateX = useSpring(useTransform(y, [0, 1], [max, -max]), { stiffness: 200, damping: 20 });
  const rotateY = useSpring(useTransform(x, [0, 1], [-max, max]), { stiffness: 200, damping: 20 });
  const move = (event: { currentTarget: HTMLElement; clientX: number; clientY: number }) => {
    const box = event.currentTarget.getBoundingClientRect();
    x.set((event.clientX - box.left) / box.width);
    y.set((event.clientY - box.top) / box.height);
  };
  const reset = () => { x.set(0.5); y.set(0.5); };
  if (still) return <div className={className}>{children}</div>;
  return (
    <motion.div className={`tilt${className ? ` ${className}` : ""}`} style={{ rotateX, rotateY, transformPerspective: 800 }}
      onPointerMove={move} onPointerLeave={reset} onPointerUp={reset}>
      {children}
    </motion.div>
  );
}

/** Un bouton dont le toucher fait une onde partant du doigt (`variant` comme Button). */
export function RippleButton({ children, onClick, variant = "primary", block, type = "button" }: {
  children: ReactNode; onClick?: () => void; variant?: "primary" | "plain" | "danger"; block?: boolean; type?: "button" | "submit";
}) {
  const [waves, setWaves] = useState<{ id: number; x: number; y: number; size: number }[]>([]);
  const still = useReducedMotion();
  const press = (event: MouseEvent<HTMLButtonElement>) => {
    if (!still) {
      const box = event.currentTarget.getBoundingClientRect();
      const size = Math.max(box.width, box.height) * 2;
      const wave = { id: Date.now() + Math.random(), x: event.clientX - box.left - size / 2, y: event.clientY - box.top - size / 2, size };
      setWaves((all) => [...all, wave]);
      setTimeout(() => setWaves((all) => all.filter((item) => item.id !== wave.id)), 600);
    }
    onClick?.();
  };
  const classes = ["btn", "ripple", variant !== "plain" && `btn--${variant}`, block && "btn--block"].filter(Boolean).join(" ");
  return (
    <button className={classes} type={type} onClick={press}>
      {children}
      {waves.map((wave) => (
        <motion.span key={wave.id} className="ripple__wave" style={{ left: wave.x, top: wave.y, width: wave.size, height: wave.size }}
          initial={{ scale: 0, opacity: 0.35 }} animate={{ scale: 1, opacity: 0 }} transition={{ duration: 0.6, ease: "easeOut" }} aria-hidden="true" />
      ))}
    </button>
  );
}

/** Un texte sur lequel passe un reflet (un titre, une offre), en boucle lente. */
export function ShineText({ children, className }: { children: ReactNode; className?: string }) {
  return <span className={`shine${className ? ` ${className}` : ""}`}>{children}</span>;
}

/** Un nombre dont les chiffres tournent comme un compteur quand il change (« 1 235 »). */
export function FlipNumber({ value, format = (number) => new Intl.NumberFormat("fr-FR").format(number), label }: {
  value: number; format?: (value: number) => string; label?: string;
}) {
  const text = format(value);
  const still = useReducedMotion();
  return (
    <span className="flip-number" aria-label={label ? `${label} : ${text}` : text} role="img">
      {[...text].map((char, index) => (
        <span className="flip-number__slot" key={`${text.length - index}`} aria-hidden="true">
          <AnimatePresence mode="popLayout" initial={false}>
            <motion.span key={char} className="flip-number__char"
              initial={still ? false : { y: "-100%", opacity: 0 }} animate={{ y: "0%", opacity: 1 }} exit={still ? undefined : { y: "100%", opacity: 0 }}
              transition={{ duration: 0.35, ease: EASE }}>
              {char === " " || char === " " || char === " " ? " " : char}
            </motion.span>
          </AnimatePresence>
        </span>
      ))}
    </span>
  );
}

/** Un bloc (une image, une illustration) qui défile un peu moins vite que la page : de la
 * profondeur. `strength` : pixels de décalage sur toute la traversée de l'écran. */
export function Parallax({ children, strength = 40 }: { children: ReactNode; strength?: number }) {
  const box = useRef<HTMLDivElement>(null);
  const still = useReducedMotion();
  const { scrollYProgress } = useScroll({ target: box, offset: ["start end", "end start"] });
  const y = useTransform(scrollYProgress, [0, 1], still ? [0, 0] : [-strength / 2, strength / 2]);
  return (
    <div ref={box} className="parallax">
      <motion.div style={{ y }}>{children}</motion.div>
    </div>
  );
}

/** Un élément qui flotte doucement (une icône d'état vide, une mascotte). */
export function Float({ children, distance = 6, seconds = 3 }: { children: ReactNode; distance?: number; seconds?: number }) {
  const still = useReducedMotion();
  return (
    <motion.span className="float" animate={still ? undefined : { y: [0, -distance, 0] }}
      transition={{ duration: seconds, repeat: Infinity, ease: "easeInOut" }}>
      {children}
    </motion.span>
  );
}

/** Un éclair de couleur derrière une valeur quand elle change (un solde mis à jour, un score). */
export function Flash({ value, children, tone = "accent" }: { value: unknown; children: ReactNode; tone?: "accent" | "in" | "out" }) {
  const first = useRef(true);
  const [key, setKey] = useState(0);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    setKey((count) => count + 1);
  }, [value]);
  return (
    <span className={`flash flash--${tone}`}>
      {key > 0 && <motion.span key={key} className="flash__glow" initial={{ opacity: 0.55 }} animate={{ opacity: 0 }} transition={{ duration: 0.9 }} aria-hidden="true" />}
      {children}
    </span>
  );
}

/** Une pile de cartes à balayer (gauche : non, droite : oui), comme une appli de tri ; les
 * boutons font la même chose pour qui ne glisse pas. */
export function SwipeDeck<T>({ items, itemKey, render, onSwipe, noLabel = "Passer", yesLabel = "Garder", emptyText = "Plus rien à trier." }: {
  items: T[]; itemKey: (item: T) => string; render: (item: T) => ReactNode; onSwipe: (item: T, yes: boolean) => void;
  noLabel?: string; yesLabel?: string; emptyText?: ReactNode;
}) {
  const top = items[0];
  const x = useMotionValue(0);
  const rotate = useTransform(x, [-200, 200], [-12, 12]);
  const yesShown = useTransform(x, [20, 120], [0, 1]);
  const noShown = useTransform(x, [-120, -20], [1, 0]);
  const decide = (yes: boolean) => {
    if (top === undefined) return;
    animate(x, yes ? 400 : -400, { duration: 0.25 }).then(() => {
      onSwipe(top, yes);
      x.set(0);
    });
  };
  if (top === undefined) return <p className="swipe-deck__empty">{emptyText}</p>;
  return (
    <div className="swipe-deck">
      <div className="swipe-deck__stack">
        {items[1] !== undefined && <div className="swipe-deck__card swipe-deck__card--next" aria-hidden="true">{render(items[1])}</div>}
        <motion.div key={itemKey(top)} className="swipe-deck__card" style={{ x, rotate }} drag="x" dragConstraints={{ left: 0, right: 0 }} dragElastic={0.9}
          onDragEnd={(_, info) => { if (Math.abs(info.offset.x) > 110) decide(info.offset.x > 0); }}>
          <motion.span className="swipe-deck__stamp swipe-deck__stamp--yes" style={{ opacity: yesShown }} aria-hidden="true">{yesLabel}</motion.span>
          <motion.span className="swipe-deck__stamp swipe-deck__stamp--no" style={{ opacity: noShown }} aria-hidden="true">{noLabel}</motion.span>
          {render(top)}
        </motion.div>
      </div>
      <div className="btn-row">
        <button className="btn btn--small" type="button" onClick={() => decide(false)}>{noLabel}</button>
        <button className="btn btn--primary btn--small" type="button" onClick={() => decide(true)}>{yesLabel}</button>
      </div>
    </div>
  );
}

/** Deux vues qui glissent l'une sur l'autre (une liste puis son détail) : `page` est la vue
 * montrée, `back` dit si l'on revient (glissement vers la droite). */
export function SlideView({ page, back, children }: { page: string; back?: boolean; children: ReactNode }) {
  const still = useReducedMotion();
  const from = still ? 0 : back ? -40 : 40;
  return (
    <AnimatePresence mode="wait" initial={false}>
      <motion.div key={page} initial={{ opacity: 0, x: from }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: -from }} transition={{ duration: 0.22, ease: EASE }}>
        {children}
      </motion.div>
    </AnimatePresence>
  );
}

/** La barre d'avancement de lecture en haut de la page (un long texte, une aide). */
export function ScrollProgress() {
  const { scrollYProgress } = useScroll();
  const scaleX = useSpring(scrollYProgress, { stiffness: 200, damping: 30 });
  return <motion.div className="scroll-progress" style={{ scaleX }} aria-hidden="true" />;
}

/** Un nombre qui monte de 0 à `to` quand il arrive à l'écran (« 1 250 utilisateurs »). */
export function CountUp({ to, duration = 1.2, format = (value: number) => new Intl.NumberFormat("fr-FR").format(Math.round(value)) }: {
  to: number; duration?: number; format?: (value: number) => string;
}) {
  const still = useReducedMotion();
  const value = useMotionValue(still ? to : 0);
  const text = useTransform(value, format);
  const seen = useRef(false);
  return (
    <motion.span className="count-up" onViewportEnter={() => {
      if (seen.current || still) return;
      seen.current = true;
      animate(value, to, { duration, ease: EASE });
    }} viewport={{ once: true }}>{text}</motion.span>
  );
}
