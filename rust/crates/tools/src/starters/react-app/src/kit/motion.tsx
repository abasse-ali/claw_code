// Animations prêtes à l'emploi : apparitions (une à une, au défilement), contenu qui se déplie,
// changement de contenu qui glisse (mois suivant), rebond, secousse d'erreur, pulsation,
// coche de réussite dessinée, anneau de progression, confettis. Faites avec framer-motion ;
// AppShell (MotionConfig reducedMotion="user") les calme quand le téléphone demande moins de
// mouvement, et celles qui ne sont pas des déplacements vérifient useReducedMotion() elles-mêmes.

import confetti from "canvas-confetti";
import { AnimatePresence, animate, motion, useMotionValue, useReducedMotion, useTransform, type Variants } from "framer-motion";
import { Children, useEffect, useRef, type ReactNode } from "react";

const EASE = [0.2, 0.9, 0.3, 1] as const;

/** Un bloc qui apparaît en montant légèrement : à l'ouverture, ou quand on l'atteint en
 * faisant défiler (`onScroll`). */
export function Appear({ children, delay = 0, onScroll, className }: {
  children: ReactNode; delay?: number; onScroll?: boolean; className?: string;
}) {
  const shown = { opacity: 1, y: 0 };
  return (
    <motion.div
      className={className}
      initial={{ opacity: 0, y: 12 }}
      animate={onScroll ? undefined : shown}
      whileInView={onScroll ? shown : undefined}
      viewport={{ once: true, amount: 0.2 }}
      transition={{ duration: 0.35, delay, ease: EASE }}
    >
      {children}
    </motion.div>
  );
}

const staggerParent: Variants = { hidden: {}, shown: { transition: { staggerChildren: 0.06 } } };
const staggerChild: Variants = {
  hidden: { opacity: 0, y: 10 },
  shown: { opacity: 1, y: 0, transition: { duration: 0.3, ease: EASE } },
};

/** Ses enfants (des tuiles, des cartes) apparaissent l'un après l'autre. `gap` : secondes entre
 * deux. Pour les lignes d'une List, inutile : Row apparaît déjà seule. */
export function Stagger({ children, gap = 0.06, className }: { children: ReactNode; gap?: number; className?: string }) {
  const parent = gap === 0.06 ? staggerParent : { hidden: {}, shown: { transition: { staggerChildren: gap } } };
  return (
    <motion.div className={className} variants={parent} initial="hidden" animate="shown">
      {Children.map(children, (child) => child == null || child === false ? child : <motion.div variants={staggerChild}>{child}</motion.div>)}
    </motion.div>
  );
}

/** Un contenu qui se déplie et se replie en douceur quand `open` change (un détail, une option). */
export function Reveal({ open, children }: { open: boolean; children: ReactNode }) {
  return (
    <AnimatePresence initial={false}>
      {open && (
        <motion.div
          key="reveal"
          style={{ overflow: "hidden" }}
          initial={{ height: 0, opacity: 0 }}
          animate={{ height: "auto", opacity: 1 }}
          exit={{ height: 0, opacity: 0 }}
          transition={{ duration: 0.25, ease: EASE }}
        >
          {children}
        </motion.div>
      )}
    </AnimatePresence>
  );
}

/** Le contenu glisse quand `id` change : le mois suivant arrive de la droite (`direction` 1),
 * le précédent de la gauche (-1), sans direction il se fond. */
export function Swap({ id, direction = 0, children }: { id: string | number; direction?: number; children: ReactNode }) {
  const shift = direction * 40;
  return (
    <div style={{ overflow: "hidden" }}>
      <AnimatePresence mode="popLayout" initial={false}>
        <motion.div
          key={id}
          initial={{ opacity: 0, x: shift }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0, x: -shift }}
          transition={{ duration: 0.25, ease: EASE }}
        >
          {children}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}

/** Rejoue une animation à chaque changement de `trigger`, pas au premier affichage. */
function useReplay(trigger: unknown, run: () => void) {
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    run();
    // run change à chaque rendu : seul trigger compte.
  }, [trigger]);
}

function Replayed({ trigger, keyframes, duration, children }: {
  trigger: unknown; keyframes: Record<string, number[]>; duration: number; children: ReactNode;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const still = useReducedMotion();
  useReplay(trigger, () => {
    if (!still && ref.current) animate(ref.current, keyframes, { duration, ease: "easeOut" });
  });
  return <span ref={ref} style={{ display: "inline-block" }}>{children}</span>;
}

/** Un petit rebond quand `trigger` change : une case cochée, un compteur qui monte. */
export function Pop({ trigger, children }: { trigger: unknown; children: ReactNode }) {
  return <Replayed trigger={trigger} keyframes={{ scale: [1, 1.18, 0.96, 1] }} duration={0.35}>{children}</Replayed>;
}

/** Une secousse de gauche à droite quand `trigger` change : un code faux, un champ refusé. */
export function Shake({ trigger, children }: { trigger: unknown; children: ReactNode }) {
  return <Replayed trigger={trigger} keyframes={{ x: [0, -8, 8, -6, 6, -3, 0] }} duration={0.4}>{children}</Replayed>;
}

/** Un halo qui pulse autour de ce qu'il entoure tant que `active` : ce qui attend l'utilisateur
 * (la tâche du jour, une nouveauté). */
export function Pulse({ active = true, children }: { active?: boolean; children: ReactNode }) {
  return <span className={`pulse${active ? " is-active" : ""}`}>{children}</span>;
}

/** Une coche de réussite qui se dessine dans un cercle (après un envoi, un objectif atteint). */
export function SuccessCheck({ size = 72, label = "Réussi" }: { size?: number; label?: string }) {
  const still = useReducedMotion();
  const draw = (delay: number) => still
    ? { initial: false as const }
    : { initial: { pathLength: 0 }, animate: { pathLength: 1 }, transition: { duration: 0.45, delay, ease: EASE } };
  return (
    <motion.svg
      className="success-check"
      width={size}
      height={size}
      viewBox="0 0 52 52"
      role="img"
      aria-label={label}
      initial={still ? false : { scale: 0.6, opacity: 0 }}
      animate={{ scale: 1, opacity: 1 }}
      transition={{ type: "spring", stiffness: 260, damping: 18 }}
    >
      <motion.circle cx="26" cy="26" r="24" fill="none" strokeWidth="3" {...draw(0)} />
      <motion.path d="M15 27 l7 7 l15 -16" fill="none" strokeWidth="4" strokeLinecap="round" strokeLinejoin="round" {...draw(0.3)} />
    </motion.svg>
  );
}

/** Un anneau de progression qui se remplit jusqu'à `value` (0 à 1), avec le texte au centre
 * (« 3/5 », « 60 % » par défaut). */
export function ProgressRing({ value, label, size = 96, children }: {
  value: number; label: string; size?: number; children?: ReactNode;
}) {
  const clamped = Math.min(1, Math.max(0, value));
  const still = useReducedMotion();
  const progress = useMotionValue(still ? clamped : 0);
  const offset = useTransform(progress, (current) => 100 - current * 100);
  // À zéro, le bout arrondi du trait laisserait un point.
  const visible = useTransform(progress, (current) => (current > 0.004 ? 1 : 0));
  useEffect(() => {
    if (still) {
      progress.set(clamped);
      return;
    }
    const controls = animate(progress, clamped, { duration: 0.8, ease: EASE });
    return () => controls.stop();
  }, [clamped, still, progress]);
  return (
    <span className="progress-ring" style={{ width: size, height: size }} role="img" aria-label={`${label} : ${Math.round(clamped * 100)} %`}>
      <svg viewBox="0 0 36 36" width={size} height={size} aria-hidden="true">
        <circle className="progress-ring__track" cx="18" cy="18" r="15.9155" pathLength={100} />
        <motion.circle className="progress-ring__value" cx="18" cy="18" r="15.9155" pathLength={100} strokeDasharray="100 100" style={{ strokeDashoffset: offset, opacity: visible }} />
      </svg>
      <span className="progress-ring__text">{children ?? `${Math.round(clamped * 100)} %`}</span>
    </span>
  );
}

/** Des confettis pour fêter un objectif atteint, une série record. Rien quand le téléphone
 * demande moins de mouvement. `big` : une salve des deux côtés. */
export function celebrate(big = false) {
  const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim();
  const colors = accent ? [accent, "#ffd166", "#06d6a0", "#ef476f"] : undefined;
  const fire = (options: confetti.Options) => confetti({ disableForReducedMotion: true, colors, zIndex: 1000, ...options });
  if (!big) {
    fire({ particleCount: 80, spread: 70, origin: { y: 0.7 } });
    return;
  }
  fire({ particleCount: 70, angle: 60, spread: 60, origin: { x: 0, y: 0.75 } });
  fire({ particleCount: 70, angle: 120, spread: 60, origin: { x: 1, y: 0.75 } });
  setTimeout(() => fire({ particleCount: 60, spread: 100, startVelocity: 35, origin: { y: 0.6 } }), 250);
}
