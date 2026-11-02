// Effets et mises en page des plus beaux sites (Apple, Linear, Stripe, Vercel, Raycast) : une
// grille « bento » de cartes, une bordure lumineuse qui tourne, un bouton qui brille, un mot qui
// change dans un titre, un texte qui s'allume au fil du défilement, des cartes qui s'empilent,
// des onglets à pastille glissante, une palette de commandes (⌘K), des fonds (points, étoiles
// filantes, faisceau de lumière), un panneau de verre, une orbite d'icônes, un dock qui grossit
// sous le doigt, une image qui s'avance à l'arrivée. Toutes se calment quand le téléphone demande
// moins de mouvement ; app-kit.css les habille en clair et en sombre.

import { AnimatePresence, motion, useMotionTemplate, useMotionValue, useReducedMotion, useScroll, useSpring, useTransform, type MotionValue } from "framer-motion";
import { Check, ChevronLeft, ChevronRight, LoaderCircle, Search, type LucideIcon } from "lucide-react";
import { Children, useEffect, useId, useMemo, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { Icon } from "./icons";
import { money } from "./format";

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

// --- Widgets de vitrine -----------------------------------------------------------------------

/** Le bouton d'une action qui prend du temps (payer, envoyer, enregistrer) : il tourne pendant
 * l'attente, puis montre une coche et `done` (« Payé »), et revient. `onClick` rend une promesse ;
 * une erreur le remet en place avec `failed`. */
export function ActionButton({ children, onClick, done = "C'est fait", failed = "Réessayer", icon }: {
  children: ReactNode; onClick: () => Promise<unknown> | unknown; done?: string; failed?: string; icon?: LucideIcon;
}) {
  const [state, setState] = useState<"idle" | "busy" | "done" | "failed">("idle");
  const press = async () => {
    if (state === "busy") return;
    setState("busy");
    try {
      await onClick();
      setState("done");
      setTimeout(() => setState("idle"), 2200);
    } catch {
      setState("failed");
    }
  };
  return (
    <motion.button type="button" layout className={`action-btn action-btn--${state}`} onClick={press} aria-busy={state === "busy"} transition={{ duration: 0.25, ease: EASE }}>
      <AnimatePresence mode="popLayout" initial={false}>
        <motion.span key={state} className="action-btn__face" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -8 }} transition={{ duration: 0.2 }}>
          {state === "busy" ? <Icon icon={LoaderCircle} size={20} /> : state === "done" ? <><Icon icon={Check} size={20} />{done}</> : <>{icon && state === "idle" && <Icon icon={icon} size={18} />}{state === "failed" ? failed : children}</>}
        </motion.span>
      </AnimatePresence>
      <span className="visually-hidden" role="status">{state === "busy" ? "En cours" : state === "done" ? done : ""}</span>
    </motion.button>
  );
}

export type Plan = { name: string; monthly: number; yearly: number; text?: string; features: string[]; best?: boolean; action: string; onChoose: () => void };

/** Les forfaits côte à côte, avec la bascule « Mensuel / Annuel » dont les prix défilent ; le
 * forfait `best` est mis en avant par la bordure lumineuse. Prix en euros. */
export function PricingTable({ plans, saving }: { plans: Plan[]; saving?: string }) {
  const [yearly, setYearly] = useState(false);
  return (
    <div className="pricing">
      <PillTabs label="Facturation" value={yearly ? "an" : "mois"} onChange={(value) => setYearly(value === "an")}
        tabs={[{ id: "mois", label: "Mensuel" }, { id: "an", label: saving ? `Annuel · ${saving}` : "Annuel" }]} />
      <div className="pricing__plans">
        {plans.map((plan) => {
          const body = (
            <div className="pricing__plan">
              <span className="pricing__name">{plan.name}{plan.best && <span className="badge badge--accent">Conseillé</span>}</span>
              <span className="pricing__price">
                <AnimatePresence mode="popLayout" initial={false}>
                  <motion.strong key={yearly ? "y" : "m"} initial={{ y: 14, opacity: 0 }} animate={{ y: 0, opacity: 1 }} exit={{ y: -14, opacity: 0 }} transition={{ duration: 0.25, ease: EASE }}>
                    {money(yearly ? plan.yearly / 12 : plan.monthly)}
                  </motion.strong>
                </AnimatePresence>
                <small>par mois{yearly ? `, ${money(plan.yearly)} par an` : ""}</small>
              </span>
              {plan.text && <p className="pricing__text">{plan.text}</p>}
              <FeatureList items={plan.features} />
              {plan.best ? <ShimmerButton onClick={plan.onChoose}>{plan.action}</ShimmerButton>
                : <button type="button" className="btn btn--block" onClick={plan.onChoose}>{plan.action}</button>}
            </div>
          );
          return <div key={plan.name} className={`pricing__slot${plan.best ? " is-best" : ""}`}>{plan.best ? <BorderBeam>{body}</BorderBeam> : body}</div>;
        })}
      </div>
    </div>
  );
}

/** Une liste d'avantages dont les coches se dessinent une à une quand elle arrive à l'écran. */
export function FeatureList({ items }: { items: string[] }) {
  const still = useReducedMotion();
  return (
    <ul className="feature-list">
      {items.map((item, index) => (
        <li key={item}>
          <svg className="feature-list__tick" viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
            <motion.path d="M7 12.5l3.2 3.2L17 9" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"
              initial={still ? false : { pathLength: 0 }} whileInView={{ pathLength: 1 }} viewport={{ once: true }} transition={{ duration: 0.35, delay: 0.15 + index * 0.08 }} />
          </svg>
          <span>{item}</span>
        </li>
      ))}
    </ul>
  );
}

/** Une galerie en mosaïque, comme Pinterest : les images gardent leurs proportions et se rangent
 * en colonnes (2 au téléphone, 3 puis 4 plus large). `onOpen` : toucher une image (la Lightbox). */
export function Masonry({ images, onOpen }: { images: { src: string; alt: string }[]; onOpen?: (index: number) => void }) {
  const still = useReducedMotion();
  return (
    <div className="masonry">
      {images.map((image, index) => (
        <motion.button key={image.src} type="button" className="masonry__item" onClick={() => onOpen?.(index)} aria-label={image.alt}
          initial={still ? false : { opacity: 0, y: 16 }} whileInView={{ opacity: 1, y: 0 }} viewport={{ once: true }} transition={{ duration: 0.45, delay: (index % 4) * 0.06 }}>
          <img src={image.src} alt="" loading="lazy" />
        </motion.button>
      ))}
    </div>
  );
}

/** Un carrousel en 3D (Cover Flow) : la carte du milieu en avant, les voisines inclinées
 * derrière ; on glisse ou on touche les flèches. */
export function Coverflow({ items, label }: { items: { id: string; content: ReactNode }[]; label: string }) {
  const [index, setIndex] = useState(0);
  const still = useReducedMotion();
  const go = (next: number) => setIndex(Math.max(0, Math.min(items.length - 1, next)));
  return (
    <div className="coverflow" role="region" aria-roledescription="carrousel" aria-label={label}>
      <motion.div className="coverflow__stage" drag="x" dragConstraints={{ left: 0, right: 0 }} dragElastic={0.2}
        onDragEnd={(_, info) => { if (info.offset.x < -40) go(index + 1); if (info.offset.x > 40) go(index - 1); }}>
        {items.map((item, position) => {
          const offset = position - index;
          const hidden = Math.abs(offset) > 2;
          return (
            <motion.div key={item.id} className="coverflow__card" aria-hidden={offset !== 0}
              animate={{ x: `${offset * 96}%`, rotateY: still ? 0 : offset * -38, scale: offset === 0 ? 1 : 0.82, opacity: hidden ? 0 : 1 - Math.abs(offset) * 0.25, zIndex: 10 - Math.abs(offset) }}
              transition={{ type: "spring", stiffness: 260, damping: 30 }} onClick={() => go(position)}>
              {item.content}
            </motion.div>
          );
        })}
      </motion.div>
      <div className="coverflow__nav">
        <button type="button" className="icon-btn" onClick={() => go(index - 1)} disabled={index === 0} aria-label="Précédent"><Icon icon={ChevronLeft} size={20} /></button>
        <span>{index + 1} / {items.length}</span>
        <button type="button" className="icon-btn" onClick={() => go(index + 1)} disabled={index === items.length - 1} aria-label="Suivant"><Icon icon={ChevronRight} size={20} /></button>
      </div>
    </div>
  );
}

/** Des panneaux côte à côte : celui qu'on touche s'élargit et montre son texte (les saisons, les
 * destinations, les collections). `items` : [{ title, text, image }]. */
export function ExpandingPanels({ items }: { items: { title: string; text?: string; image: string }[] }) {
  const [open, setOpen] = useState(0);
  return (
    <div className="panels">
      {items.map((item, index) => (
        <motion.button key={item.title} type="button" layout className={`panels__panel${index === open ? " is-open" : ""}`} aria-expanded={index === open}
          onClick={() => setOpen(index)} style={{ backgroundImage: `url("${item.image}")` }} transition={{ duration: 0.45, ease: EASE }}>
          <span className="panels__text">
            <strong>{item.title}</strong>
            {index === open && item.text && <motion.span initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.2 }}>{item.text}</motion.span>}
          </span>
        </motion.button>
      ))}
    </div>
  );
}

/** Un mot souligné au surligneur, le trait se dessine quand il arrive à l'écran. */
export function Marker({ children }: { children: ReactNode }) {
  const still = useReducedMotion();
  return (
    <span className="marker">
      <span className="marker__text">{children}</span>
      <svg className="marker__stroke" viewBox="0 0 200 20" preserveAspectRatio="none" aria-hidden="true">
        <motion.path d="M3 13 C 40 6, 90 16, 130 9 S 185 8, 197 11" fill="none" strokeWidth="7" strokeLinecap="round"
          initial={still ? false : { pathLength: 0 }} whileInView={{ pathLength: 1 }} viewport={{ once: true }} transition={{ duration: 0.7, ease: EASE, delay: 0.2 }} />
      </svg>
    </span>
  );
}

/** Un texte écrit en cercle qui tourne lentement autour d'une icône (« NOUVEAU · 2026 · »), le
 * badge des pages de lancement. */
export function CircleText({ text, icon, size = 112 }: { text: string; icon?: LucideIcon; size?: number }) {
  const id = useId().replace(/:/g, "");
  return (
    <span className="circle-text" style={{ width: size, height: size }} role="img" aria-label={text}>
      <svg viewBox="0 0 100 100" aria-hidden="true">
        <defs><path id={`${id}-c`} d="M50 50 m-38 0 a38 38 0 1 1 76 0 a38 38 0 1 1 -76 0" /></defs>
        <text><textPath href={`#${id}-c`} textLength="236" lengthAdjust="spacing">{text}</textPath></text>
      </svg>
      {icon && <span className="circle-text__icon"><Icon icon={icon} size={Math.round(size / 4)} /></span>}
    </span>
  );
}

/** Une carte à gratter : on frotte du doigt la couche argentée pour découvrir ce qu'il y a
 * dessous (un code promo, une surprise) ; `onReveal` quand plus de la moitié est grattée. */
export function ScratchCard({ children, onReveal, label = "Grattez ici" }: { children: ReactNode; onReveal?: () => void; label?: string }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [revealed, setRevealed] = useState(false);
  const drawing = useRef(false);
  useEffect(() => {
    const el = canvas.current;
    const context = el?.getContext("2d");
    if (!el || !context) return;
    const box = el.getBoundingClientRect();
    el.width = box.width * devicePixelRatio;
    el.height = box.height * devicePixelRatio;
    context.scale(devicePixelRatio, devicePixelRatio);
    const gradient = context.createLinearGradient(0, 0, box.width, box.height);
    gradient.addColorStop(0, "#c9ccd3"); gradient.addColorStop(0.5, "#eef0f3"); gradient.addColorStop(1, "#b9bdc6");
    context.fillStyle = gradient;
    context.fillRect(0, 0, box.width, box.height);
    context.fillStyle = "#6b6f78";
    context.font = "600 15px system-ui, sans-serif";
    context.textAlign = "center";
    context.fillText(label, box.width / 2, box.height / 2 + 5);
  }, [label]);
  const scratch = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    if (!drawing.current || revealed) return;
    const el = event.currentTarget;
    const context = el.getContext("2d");
    if (!context) return;
    const box = el.getBoundingClientRect();
    context.globalCompositeOperation = "destination-out";
    context.beginPath();
    context.arc(event.clientX - box.left, event.clientY - box.top, 22, 0, Math.PI * 2);
    context.fill();
    const data = context.getImageData(0, 0, el.width, el.height).data;
    let clear = 0;
    for (let i = 3; i < data.length; i += 64) if (data[i] === 0) clear += 1;
    if (clear / (data.length / 64) > 0.5) {
      setRevealed(true);
      onReveal?.();
    }
  };
  return (
    <div className="scratch">
      <div className="scratch__prize">{children}</div>
      {!revealed && (
        <canvas ref={canvas} className="scratch__cover" aria-label={label} role="img"
          onPointerDown={(event) => { drawing.current = true; event.currentTarget.setPointerCapture(event.pointerId); scratch(event); }}
          onPointerMove={scratch} onPointerUp={() => { drawing.current = false; }} />
      )}
      {!revealed && <button type="button" className="scratch__skip" onClick={() => { setRevealed(true); onReveal?.(); }}>Tout découvrir</button>}
    </div>
  );
}

/** Une loupe qui s'ouvre en champ de recherche quand on la touche (et se referme vide). */
export function ExpandSearch({ value, onChange, placeholder = "Rechercher" }: { value: string; onChange: (value: string) => void; placeholder?: string }) {
  const [open, setOpen] = useState(Boolean(value));
  const input = useRef<HTMLInputElement>(null);
  return (
    <motion.div className={`expand-search${open ? " is-open" : ""}`} layout transition={{ duration: 0.3, ease: EASE }}>
      <button type="button" className="expand-search__btn" aria-label={placeholder} aria-expanded={open}
        onClick={() => { setOpen(true); setTimeout(() => input.current?.focus(), 50); }}>
        <Icon icon={Search} size={20} />
      </button>
      {open && (
        <motion.input ref={input} value={value} placeholder={placeholder} aria-label={placeholder}
          initial={{ opacity: 0 }} animate={{ opacity: 1 }} onChange={(event) => onChange(event.target.value)}
          onBlur={() => { if (!value) setOpen(false); }} onKeyDown={(event) => { if (event.key === "Escape") { onChange(""); setOpen(false); } }} />
      )}
    </motion.div>
  );
}

/** Un en-tête collé en haut dont le grand titre rétrécit et dont le fond se floute quand on fait
 * défiler (les titres d'iOS). `end` : un bouton à droite. */
export function ShrinkHeader({ title, end }: { title: string; end?: ReactNode }) {
  const { scrollY } = useScroll();
  const still = useReducedMotion();
  const size = useTransform(scrollY, [0, 80], [30, 18]);
  const shade = useTransform(scrollY, [0, 80], [0, 1]);
  const fontSize = useMotionTemplate`${size}px`;
  return (
    <header className="shrink-header">
      <motion.span className="shrink-header__shade" style={{ opacity: still ? 1 : shade }} aria-hidden="true" />
      <motion.h1 style={still ? undefined : { fontSize }}>{title}</motion.h1>
      {end}
    </header>
  );
}
