// Effets et mises en page des plus beaux sites (Apple, Linear, Stripe, Vercel, Raycast) : une
// grille « bento » de cartes, une bordure lumineuse qui tourne, un bouton qui brille, un mot qui
// change dans un titre, un texte qui s'allume au fil du défilement, des cartes qui s'empilent,
// des onglets à pastille glissante, une palette de commandes (⌘K), des fonds (points, étoiles
// filantes, faisceau de lumière), un panneau de verre, une orbite d'icônes, un dock qui grossit
// sous le doigt, une image qui s'avance à l'arrivée. Toutes se calment quand le téléphone demande
// moins de mouvement ; app-kit.css les habille en clair et en sombre.

import { AnimatePresence, motion, useMotionTemplate, useMotionValue, useReducedMotion, useScroll, useSpring, useTransform, type MotionValue } from "framer-motion";
import { Search, type LucideIcon } from "lucide-react";
import { Children, useEffect, useId, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { Icon } from "./icons";

const EASE = [0.2, 0.9, 0.3, 1] as const;

// --- Bento ------------------------------------------------------------------------------------

/** Une grille de cartes de tailles variées, comme les pages produit d'Apple et de Linear : sur un
 * téléphone, une colonne ; plus large, 2 puis 4 colonnes. Ses enfants sont des BentoCard. */
export function BentoGrid({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={`bento${className ? ` ${className}` : ""}`}>{children}</div>;
}

/** Une carte de la grille bento : `size` "wide" (2 colonnes), "tall" (2 lignes), "big" (les deux)
 * ou rien ; une lumière suit le doigt ou la souris. `visual` : ce qui illustre la carte (un
 * graphique, une image, un chiffre). */
export function BentoCard({ title, text, icon, visual, size, onClick, children }: {
  title: string; text?: ReactNode; icon?: LucideIcon; visual?: ReactNode; size?: "wide" | "tall" | "big"; onClick?: () => void; children?: ReactNode;
}) {
  const x = useMotionValue(-300);
  const y = useMotionValue(-300);
  const light = useMotionTemplate`radial-gradient(260px circle at ${x}px ${y}px, color-mix(in srgb, var(--accent) 16%, transparent), transparent 70%)`;
  const still = useReducedMotion();
  const Tag = onClick ? "button" : "div";
  return (
    <motion.div className={`bento__card${size ? ` bento__card--${size}` : ""}`}
      initial={still ? false : { opacity: 0, y: 18 }} whileInView={{ opacity: 1, y: 0 }} viewport={{ once: true, margin: "-40px" }}
      transition={{ duration: 0.55, ease: EASE }}
      onPointerMove={(event) => { const box = event.currentTarget.getBoundingClientRect(); x.set(event.clientX - box.left); y.set(event.clientY - box.top); }}
      onPointerLeave={() => { x.set(-300); y.set(-300); }}>
      {!still && <motion.span className="bento__light" style={{ background: light }} aria-hidden="true" />}
      <Tag className="bento__body" {...(onClick ? { type: "button" as const, onClick } : {})}>
        {visual && <span className="bento__visual">{visual}</span>}
        <span className="bento__text">
          {icon && <span className="bento__icon"><Icon icon={icon} size={20} /></span>}
          <strong>{title}</strong>
          {text && <span className="bento__sub">{text}</span>}
        </span>
        {children}
      </Tag>
    </motion.div>
  );
}

// --- Bordure lumineuse, bouton brillant ---------------------------------------------------------

/** Une lumière qui fait le tour du bord d'une carte (l'offre mise en avant, le forfait conseillé),
 * comme sur Vercel et Linear. `duration` : un tour, en secondes. */
export function BorderBeam({ children, duration = 6, className }: { children: ReactNode; duration?: number; className?: string }) {
  return (
    <div className={`beam${className ? ` ${className}` : ""}`} style={{ "--beam-time": `${duration}s` } as CSSProperties}>
      <span className="beam__ring" aria-hidden="true" />
      <div className="beam__body">{children}</div>
    </div>
  );
}

/** Le bouton principal d'un écran avec un reflet qui passe dessus de temps en temps (Stripe) :
 * « Commencer », « Payer ». */
export function ShimmerButton({ children, onClick, type = "button", icon, disabled }: {
  children: ReactNode; onClick?: () => void; type?: "button" | "submit"; icon?: LucideIcon; disabled?: boolean;
}) {
  return (
    <button type={type} className="shimmer-btn" onClick={onClick} disabled={disabled}>
      <span className="shimmer-btn__shine" aria-hidden="true" />
      {icon && <Icon icon={icon} size={18} />}
      <span>{children}</span>
    </button>
  );
}

// --- Textes -------------------------------------------------------------------------------------

/** Un mot qui change dans une phrase (« Gérez vos [courses, économies, projets] ») : chacun
 * monte et laisse la place au suivant. Lu en entier par les lecteurs d'écran. */
export function WordRotate({ words, interval = 2200, className }: { words: string[]; interval?: number; className?: string }) {
  const [index, setIndex] = useState(0);
  const still = useReducedMotion();
  useEffect(() => {
    if (still || words.length < 2) return;
    const timer = setInterval(() => setIndex((value) => (value + 1) % words.length), interval);
    return () => clearInterval(timer);
  }, [still, words.length, interval]);
  const longest = useMemo(() => words.reduce((a, b) => (b.length > a.length ? b : a), ""), [words]);
  return (
    <span className={`word-rotate${className ? ` ${className}` : ""}`} aria-label={words.join(", ")}>
      <span className="word-rotate__ghost" aria-hidden="true">{longest}</span>
      <AnimatePresence mode="popLayout" initial={false}>
        <motion.span key={words[index]} className="word-rotate__word" aria-hidden="true"
          initial={{ y: "70%", opacity: 0, filter: "blur(4px)" }} animate={{ y: "0%", opacity: 1, filter: "blur(0px)" }}
          exit={{ y: "-70%", opacity: 0, filter: "blur(4px)" }} transition={{ duration: 0.45, ease: EASE }}>
          {words[index]}
        </motion.span>
      </AnimatePresence>
    </span>
  );
}

function ScrollWord({ word, progress, from, to }: { word: string; progress: MotionValue<number>; from: number; to: number }) {
  const opacity = useTransform(progress, [from, to], [0.18, 1]);
  return <motion.span style={{ opacity }}>{word} </motion.span>;
}

/** Un paragraphe dont les mots s'allument un à un pendant qu'on fait défiler la page, comme les
 * textes d'Apple : une promesse, un manifeste, l'histoire de l'appli. */
export function ScrollWords({ text, className }: { text: string; className?: string }) {
  const ref = useRef<HTMLParagraphElement>(null);
  const still = useReducedMotion();
  const { scrollYProgress } = useScroll({ target: ref, offset: ["start 0.85", "end 0.45"] });
  const words = text.split(/\s+/).filter(Boolean);
  if (still) return <p className={`scroll-words${className ? ` ${className}` : ""}`}>{text}</p>;
  return (
    <p ref={ref} className={`scroll-words${className ? ` ${className}` : ""}`} aria-label={text}>
      <span aria-hidden="true">
        {words.map((word, index) => <ScrollWord key={index} word={word} progress={scrollYProgress} from={index / words.length} to={(index + 1) / words.length} />)}
      </span>
    </p>
  );
}

// --- Défilement ---------------------------------------------------------------------------------

function StackCard({ children, index, count, progress }: { children: ReactNode; index: number; count: number; progress: MotionValue<number> }) {
  const scale = useTransform(progress, [index / count, 1], [1, 1 - (count - index) * 0.04]);
  return (
    <div className="scroll-stack__slot" style={{ top: `calc(72px + ${index * 14}px)` }}>
      <motion.div className="scroll-stack__card" style={{ scale }}>{children}</motion.div>
    </div>
  );
}

/** Des cartes qui restent collées en haut et s'empilent pendant qu'on fait défiler, celles du
 * dessous reculant un peu (les étapes, les avantages, les forfaits). */
export function ScrollStack({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const still = useReducedMotion();
  const { scrollYProgress } = useScroll({ target: ref, offset: ["start start", "end end"] });
  const cards = Children.toArray(children);
  if (still) return <div className="scroll-stack scroll-stack--still">{cards.map((card, index) => <div key={index} className="scroll-stack__card">{card}</div>)}</div>;
  return (
    <div ref={ref} className="scroll-stack">
      {cards.map((card, index) => <StackCard key={index} index={index} count={cards.length} progress={scrollYProgress}>{card}</StackCard>)}
    </div>
  );
}

/** Une image ou une carte qui s'avance et s'éclaire en arrivant à l'écran, comme les photos de
 * produit d'Apple. */
export function ZoomIn({ children, className }: { children: ReactNode; className?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const still = useReducedMotion();
  const { scrollYProgress } = useScroll({ target: ref, offset: ["start end", "center 0.6"] });
  const scale = useTransform(scrollYProgress, [0, 1], [0.86, 1]);
  const opacity = useTransform(scrollYProgress, [0, 1], [0.35, 1]);
  const radius = useTransform(scrollYProgress, [0, 1], [36, 20]);
  return (
    <motion.div ref={ref} className={`zoom-in${className ? ` ${className}` : ""}`} style={still ? undefined : { scale, opacity, borderRadius: radius }}>
      {children}
    </motion.div>
  );
}

// --- Navigation ---------------------------------------------------------------------------------

/** Des onglets dont la pastille glisse d'un choix à l'autre (et suit la souris en survol, comme
 * Vercel). `value` et `onChange` : le choix ; `tabs` : [{ id, label, icon }]. */
export function PillTabs<T extends string>({ tabs, value, onChange, label }: {
  tabs: { id: T; label: string; icon?: LucideIcon }[]; value: T; onChange: (id: T) => void; label?: string;
}) {
  const id = useId();
  const [hover, setHover] = useState<T | null>(null);
  return (
    <div className="pill-tabs" role="tablist" aria-label={label} onPointerLeave={() => setHover(null)}>
      {tabs.map((tab) => (
        <button key={tab.id} type="button" role="tab" aria-selected={tab.id === value} className="pill-tabs__tab"
          onClick={() => onChange(tab.id)} onPointerEnter={() => setHover(tab.id)}>
          {hover === tab.id && hover !== value && <motion.span layoutId={`${id}-hover`} className="pill-tabs__hover" transition={{ type: "spring", stiffness: 420, damping: 34 }} />}
          {tab.id === value && <motion.span layoutId={`${id}-pill`} className="pill-tabs__pill" transition={{ type: "spring", stiffness: 420, damping: 34 }} />}
          <span className="pill-tabs__label">{tab.icon && <Icon icon={tab.icon} size={16} />}{tab.label}</span>
        </button>
      ))}
    </div>
  );
}

export type Command = { id: string; label: string; icon?: LucideIcon; hint?: string; group?: string; keywords?: string; run: () => void };

/** Ouvre la palette de commandes avec ⌘K (ou Ctrl K) : `const [open, setOpen] = useCommandK();`. */
export function useCommandK(): [boolean, (open: boolean) => void] {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setOpen((value) => !value);
      }
    };
    addEventListener("keydown", key);
    return () => removeEventListener("keydown", key);
  }, []);
  return [open, setOpen];
}

const fold = (text: string) => text.toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "");

/** La palette de commandes de Raycast et Linear : un champ qui cherche parmi les actions de
 * l'appli (ajouter, aller à un écran, exporter), au clavier (↑ ↓ Entrée Échap) ou au doigt. */
export function CommandPalette({ open, onClose, commands, placeholder = "Que voulez-vous faire ?" }: {
  open: boolean; onClose: () => void; commands: Command[]; placeholder?: string;
}) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const listId = useId();
  const found = useMemo(() => {
    const words = fold(query).split(/\s+/).filter(Boolean);
    return commands.filter((command) => words.every((word) => fold(`${command.label} ${command.group || ""} ${command.keywords || ""}`).includes(word)));
  }, [commands, query]);
  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActive(0);
    setTimeout(() => input.current?.focus(), 30);
  }, [open]);
  useEffect(() => setActive(0), [query]);
  const run = (command: Command | undefined) => {
    if (!command) return;
    onClose();
    command.run();
  };
  const groups = [...new Set(found.map((command) => command.group || ""))];
  return (
    <AnimatePresence>
      {open && (
        <motion.div className="cmdk" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onPointerDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
          <motion.div className="cmdk__panel" role="dialog" aria-modal="true" aria-label="Commandes"
            initial={{ opacity: 0, scale: 0.96, y: -8 }} animate={{ opacity: 1, scale: 1, y: 0 }} exit={{ opacity: 0, scale: 0.97, y: -6 }} transition={{ duration: 0.18, ease: EASE }}>
            <label className="cmdk__search">
              <Icon icon={Search} size={18} />
              <input ref={input} value={query} placeholder={placeholder} aria-label={placeholder} role="combobox" aria-expanded="true" aria-controls={listId}
                aria-activedescendant={found[active] ? `${listId}-${found[active].id}` : undefined}
                onChange={(event) => setQuery(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "ArrowDown") { event.preventDefault(); setActive((value) => Math.min(found.length - 1, value + 1)); }
                  if (event.key === "ArrowUp") { event.preventDefault(); setActive((value) => Math.max(0, value - 1)); }
                  if (event.key === "Enter") { event.preventDefault(); run(found[active]); }
                  if (event.key === "Escape") { event.preventDefault(); onClose(); }
                }} />
              <kbd>Échap</kbd>
            </label>
            <div className="cmdk__list" id={listId} role="listbox">
              {!found.length && <p className="cmdk__empty">Aucune commande pour « {query} »</p>}
              {groups.map((group) => (
                <div key={group} role="group" aria-label={group || undefined}>
                  {group && <p className="cmdk__group">{group}</p>}
                  {found.filter((command) => (command.group || "") === group).map((command) => {
                    const index = found.indexOf(command);
                    return (
                      <button key={command.id} id={`${listId}-${command.id}`} type="button" role="option" aria-selected={index === active}
                        className="cmdk__item" onPointerMove={() => setActive(index)} onClick={() => run(command)}>
                        {command.icon && <Icon icon={command.icon} size={18} />}
                        <span>{command.label}</span>
                        {command.hint && <kbd>{command.hint}</kbd>}
                      </button>
                    );
                  })}
                </div>
              ))}
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function DockItem({ item, pointer }: { item: { icon: LucideIcon; label: string; onClick: () => void; active?: boolean }; pointer: MotionValue<number> }) {
  const ref = useRef<HTMLButtonElement>(null);
  const distance = useTransform(pointer, (x) => {
    const box = ref.current?.getBoundingClientRect();
    return box ? x - (box.left + box.width / 2) : Infinity;
  });
  const size = useSpring(useTransform(distance, [-120, 0, 120], [46, 70, 46]), { stiffness: 260, damping: 20 });
  return (
    <motion.button ref={ref} type="button" className={`dock__item${item.active ? " is-active" : ""}`} style={{ width: size, height: size }}
      onClick={item.onClick} aria-label={item.label} title={item.label} whileTap={{ scale: 0.88 }}>
      <Icon icon={item.icon} size={22} />
    </motion.button>
  );
}

/** Le dock de macOS : des icônes qui grossissent sous la souris (au doigt, elles rebondissent
 * quand on les touche). `items` : [{ icon, label, onClick, active }]. */
export function Dock({ items }: { items: { icon: LucideIcon; label: string; onClick: () => void; active?: boolean }[] }) {
  const pointer = useMotionValue(Infinity);
  return (
    <nav className="dock" aria-label="Raccourcis" onPointerMove={(event) => { if (event.pointerType === "mouse") pointer.set(event.clientX); }} onPointerLeave={() => pointer.set(Infinity)}>
      {items.map((item) => <DockItem key={item.label} item={item} pointer={pointer} />)}
    </nav>
  );
}

// --- Fonds et décors ----------------------------------------------------------------------------

/** Un fond de points (ou de lignes : `grid`) qui s'efface vers les bords, derrière un titre
 * d'accueil ou une carte mise en avant. */
export function DotBackground({ children, grid, className }: { children: ReactNode; grid?: boolean; className?: string }) {
  return (
    <div className={`dot-bg${grid ? " dot-bg--grid" : ""}${className ? ` ${className}` : ""}`}>
      <span className="dot-bg__pattern" aria-hidden="true" />
      <div className="dot-bg__content">{children}</div>
    </div>
  );
}

/** Des étoiles filantes qui traversent un bloc sombre (un écran d'accueil, une carte de
 * bienvenue). `count` : combien à la fois. */
export function Meteors({ count = 14 }: { count?: number }) {
  const still = useReducedMotion();
  const meteors = useMemo(() => Array.from({ length: count }, (_, index) => ({
    left: `${Math.round((index / count) * 120 - 10 + Math.random() * 10)}%`,
    delay: `${(Math.random() * 6).toFixed(2)}s`,
    duration: `${(3 + Math.random() * 5).toFixed(2)}s`,
  })), [count]);
  if (still) return null;
  return (
    <span className="meteors" aria-hidden="true">
      {meteors.map((meteor, index) => <i key={index} style={{ left: meteor.left, animationDelay: meteor.delay, animationDuration: meteor.duration }} />)}
    </span>
  );
}

/** Un faisceau de lumière qui tombe en biais sur le haut d'un bloc (le titre d'une page
 * d'accueil), comme les pages de lancement de produit. */
export function Spotlight({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div className={`spot${className ? ` ${className}` : ""}`}>
      <span className="spot__beam" aria-hidden="true" />
      <div className="spot__content">{children}</div>
    </div>
  );
}

/** Un panneau de verre dépoli, au reflet clair sur le bord (iOS, visionOS) : posé sur une image
 * ou un fond coloré, il le laisse deviner derrière. */
export function GlassPanel({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={`glass${className ? ` ${className}` : ""}`}>{children}</div>;
}

/** Des icônes qui tournent en orbite autour d'un centre (le logo de l'appli, un avatar) : les
 * services reliés, les membres d'un groupe. `icons` : jusqu'à 8 icônes Lucide. */
export function Orbit({ center, icons, size = 260, duration = 24 }: { center: ReactNode; icons: LucideIcon[]; size?: number; duration?: number }) {
  // Petite orbite : une seule couronne ; grande : deux, assez écartées pour ne pas se toucher.
  const two = size >= 240 && icons.length > 3;
  const inner = two ? icons.slice(0, Math.ceil(icons.length / 2)) : [];
  const outer = two ? icons.slice(inner.length) : icons;
  const outerRadius = size / 2 - 22;
  const core = Math.round(Math.min(64, size * 0.26));
  const ring = (list: LucideIcon[], radius: number, reverse: boolean) => (
    <span className={`orbit__ring${reverse ? " orbit__ring--reverse" : ""}`} style={{ width: `${(radius * 200) / size}%`, height: `${(radius * 200) / size}%`, animationDuration: `${duration}s` }}>
      {list.map((icon, index) => {
        const angle = (index / list.length) * Math.PI * 2;
        return (
          <span key={index} className="orbit__planet" style={{ left: `${50 + Math.cos(angle) * 50}%`, top: `${50 + Math.sin(angle) * 50}%`, animationDuration: `${duration}s` }}>
            <Icon icon={icon} size={20} />
          </span>
        );
      })}
    </span>
  );
  return (
    <div className="orbit" style={{ width: `min(${size}px, 100%)` }} aria-hidden="true">
      {ring(outer, outerRadius, true)}
      {two && ring(inner, outerRadius - 56, false)}
      <span className="orbit__center" style={{ width: core, height: core }}>{center}</span>
    </div>
  );
}
