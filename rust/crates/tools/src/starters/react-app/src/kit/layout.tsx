// Le cadre de l'appli : la barre du haut, les écrans (un à la fois, l'écran ouvert suit
// l'adresse, #bilan) et la barre d'onglets flottante avec son bouton d'ajout au milieu.

import { MotionConfig } from "framer-motion";
import { ChevronLeft, ChevronRight, Plus } from "lucide-react";
import {
  Children,
  createContext,
  isValidElement,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactElement,
  type ReactNode,
} from "react";
import { monthName, shiftMonth } from "./format";
import { Icon, type LucideIcon } from "./icons";
import { useEntries } from "./store";
import { Toaster } from "./toast";

export type Tab = { id: string; label: string; icon: LucideIcon };

const subscribe = (changed: () => void) => {
  window.addEventListener("hashchange", changed);
  return () => window.removeEventListener("hashchange", changed);
};
const currentHash = () => decodeURIComponent(window.location.hash.slice(1));

/** L'écran ouvert (« bilan ») et de quoi en ouvrir un autre : go("reglages"). */
export function useScreen(): [string, (id: string) => void] {
  const hash = useSyncExternalStore(subscribe, currentHash, () => "");
  return [hash, (id: string) => { window.location.hash = id; }];
}

type ScreenProps = {
  id: string;
  title?: string;
  label?: string;
  /** Ce que fait le bouton + quand cet écran est ouvert (ouvrir sa fenêtre d'ajout). */
  onAdd?: () => void;
  children: ReactNode;
};

/** Ce que l'AppShell dit à ses écrans : celui qui est ouvert, et où se signaler. */
type Shell = { shown: string | undefined; register: (id: string, add?: () => void) => () => void };
const ShellContext = createContext<Shell | null>(null);

/** Un écran de l'appli ; `title` s'affiche en haut (l'accueil n'en a souvent pas, `label` le nomme alors). */
export function Screen({ id, title, label, onAdd, children }: ScreenProps) {
  // Chaque écran se signale à l'AppShell et ne s'affiche que s'il est l'écran ouvert : un écran
  // écrit dans un composant à soi (<DepensesScreen />) suit l'adresse comme un autre. Vu le
  // 30 octobre 2026 (essai C5) : l'AppShell cherchait l'id sur ses enfants directs, ne le
  // trouvait pas sur ces composants et montrait toujours le premier écran, onglets sans effet.
  const shell = useContext(ShellContext);
  const add = useRef(onAdd);
  useLayoutEffect(() => {
    add.current = onAdd;
  });
  const hasAdd = Boolean(onAdd);
  const register = shell?.register;
  useLayoutEffect(() => register?.(id, hasAdd ? () => add.current?.() : undefined), [register, id, hasAdd]);
  if (shell && shell.shown !== id) return null;
  return (
    <section className="app-screen" id={id} data-screen aria-label={title ? undefined : label ?? id} aria-labelledby={title ? `${id}-titre` : undefined}>
      {title && <h2 className="app-screen__title" id={`${id}-titre`}>{title}</h2>}
      {children}
    </section>
  );
}

/** La barre du haut : un titre, des boutons à droite. */
export function AppBar({ title, start, children }: { title: ReactNode; start?: ReactNode; children?: ReactNode }) {
  return (
    <header className="app-bar">
      {start}
      <h1 className="app-bar__title">{title}</h1>
      <span className="app-bar__spacer" />
      {children}
    </header>
  );
}

/** La barre du haut d'une appli au mois : mois précédent, le mois affiché, mois suivant. */
export function MonthBar({ children }: { children?: ReactNode }) {
  const month = useEntries((state) => state.month);
  const setMonth = useEntries((state) => state.setMonth);
  return (
    <header className="app-bar">
      <IconButton icon={ChevronLeft} label="Mois précédent" onClick={() => setMonth(shiftMonth(month, -1))} />
      <h1 className="app-bar__title" aria-live="polite">{monthName(month)}</h1>
      <IconButton icon={ChevronRight} label="Mois suivant" onClick={() => setMonth(shiftMonth(month, 1))} />
      <span className="app-bar__spacer" />
      {children}
    </header>
  );
}

/** Un bouton rond avec une icône seule ; `label` est lu à voix haute. */
export function IconButton({ icon, label, onClick, href, className }: { icon: LucideIcon; label: string; onClick?: () => void; href?: string; className?: string }) {
  const classes = `icon-btn${className ? ` ${className}` : ""}`;
  return href ? (
    <a className={classes} href={href} aria-label={label}><Icon icon={icon} /></a>
  ) : (
    <button className={classes} type="button" aria-label={label} onClick={onClick}><Icon icon={icon} /></button>
  );
}

type AppShellProps = {
  /** La barre du haut : <MonthBar /> ou <AppBar title="…" />. */
  bar?: ReactNode;
  /** 3 à 5 onglets, dans l'ordre des écrans. */
  tabs: Tab[];
  /** Le bouton + au milieu de la barre d'onglets (l'action principale) ; un écran qui a son
   *  propre onAdd le remplace quand il est ouvert. */
  onAdd?: () => void;
  addLabel?: string;
  /** Les <Screen>, un par onglet (et d'autres écrans qu'on ouvre par go("id")), écrits ici
   *  ou dans des composants à soi. */
  children: ReactNode;
};

/** Toute l'appli : la barre du haut, l'écran ouvert, la barre d'onglets et les messages. */
export function AppShell({ bar, tabs, onAdd, addLabel = "Ajouter", children }: AppShellProps) {
  const [hash] = useScreen();
  // Les écrans signalés (et leur bouton +), dans l'ordre où ils sont rendus.
  const [screens, setScreens] = useState<Record<string, (() => void) | undefined>>({});
  const register = useCallback((id: string, add?: () => void) => {
    setScreens((all) => ({ ...all, [id]: add }));
    return () =>
      setScreens((all) => {
        const rest = { ...all };
        delete rest[id];
        return rest;
      });
  }, []);
  const direct = Children.toArray(children)
    .filter(isValidElement)
    .map((child) => (child as ReactElement<Partial<ScreenProps>>).props.id)
    .filter((id): id is string => typeof id === "string");
  const known = [...direct, ...Object.keys(screens), ...tabs.map((item) => item.id)];
  const shownId = known.includes(hash) ? hash : direct[0] ?? tabs[0]?.id ?? Object.keys(screens)[0];
  const add = (shownId !== undefined ? screens[shownId] : undefined) ?? onAdd;
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    window.scrollTo({ top: 0 });
    const title = document.querySelector<HTMLElement>(".app-screen h1, .app-screen h2");
    if (title) {
      title.tabIndex = -1;
      title.focus({ preventScroll: true });
    }
  }, [shownId]);
  const half = Math.ceil(tabs.length / 2);
  const tab = (item: Tab) => (
    <a key={item.id} className="tabbar__tab" href={`#${item.id}`} aria-current={item.id === shownId ? "page" : undefined}>
      <Icon icon={item.icon} size={22} />
      <span>{item.label}</span>
    </a>
  );
  return (
    <MotionConfig reducedMotion="user">
      <div className="app">
        {bar}
        <ShellContext.Provider value={{ shown: shownId, register }}>
          <main key={shownId}>{children}</main>
        </ShellContext.Provider>
        <nav className="tabbar" aria-label="Écrans de l'appli">
          {tabs.slice(0, half).map(tab)}
          {add && (
            <button className="tabbar__add" type="button" aria-label={addLabel} onClick={add}>
              <Icon icon={Plus} size={28} />
            </button>
          )}
          {tabs.slice(half).map(tab)}
        </nav>
      </div>
      <Toaster />
    </MotionConfig>
  );
}
