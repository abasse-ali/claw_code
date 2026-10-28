// Le cadre de l'appli : la barre du haut, les écrans (un à la fois, l'écran ouvert suit
// l'adresse, #bilan) et la barre d'onglets flottante avec son bouton d'ajout au milieu.

import { MotionConfig } from "framer-motion";
import { ChevronLeft, ChevronRight, Plus } from "lucide-react";
import {
  Children,
  isValidElement,
  useEffect,
  useRef,
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

type ScreenProps = { id: string; title?: string; label?: string; children: ReactNode };

/** Un écran de l'appli ; `title` s'affiche en haut (l'accueil n'en a souvent pas, `label` le nomme alors). */
export function Screen({ id, title, label, children }: ScreenProps) {
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
  /** Le bouton + au milieu de la barre d'onglets (l'action principale). */
  onAdd?: () => void;
  addLabel?: string;
  /** Les <Screen>, un par onglet (et d'autres écrans qu'on ouvre par go("id")). */
  children: ReactNode;
};

/** Toute l'appli : la barre du haut, l'écran ouvert, la barre d'onglets et les messages. */
export function AppShell({ bar, tabs, onAdd, addLabel = "Ajouter", children }: AppShellProps) {
  const [hash] = useScreen();
  const screens = Children.toArray(children).filter(isValidElement) as ReactElement<ScreenProps>[];
  const shown = screens.find((screen) => screen.props.id === hash) ?? screens[0];
  const shownId = shown?.props.id;
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
        <main key={shownId}>{shown}</main>
        <nav className="tabbar" aria-label="Écrans de l'appli">
          {tabs.slice(0, half).map(tab)}
          {onAdd && (
            <button className="tabbar__add" type="button" aria-label={addLabel} onClick={onAdd}>
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
