// Widgets de plus : onglets dans un écran, accordéon, liste de choix avec recherche, menu « ⋯ »,
// infobulle, tableau triable et paginé, chargement, étapes, quantité, note, curseur, code à
// usage unique, envoi de fichiers, calendrier du mois. Inspirés de shadcn/ui (MIT,
// https://ui.shadcn.com), faits avec radix-ui et habillés par app-kit.css, comme le reste du kit.

import { Check, ChevronDown, ChevronLeft, ChevronRight, ChevronsUpDown, Minus, MoreHorizontal, Plus, Star, Upload } from "lucide-react";
import { Accordion as RadixAccordion, DropdownMenu, Popover, Slider as RadixSlider, Tabs as RadixTabs, Tooltip } from "radix-ui";
import { useId, useMemo, useRef, useState, type ReactNode } from "react";
import { monthName, shiftMonth, today } from "./format";
import { Icon, type LucideIcon } from "./icons";

/** Des onglets à l'intérieur d'un écran (« Dépenses », « Remboursements ») ; les onglets de
 * l'appli, eux, sont ceux d'AppShell. */
export function Tabs({ tabs, value, onChange, label }: {
  tabs: { id: string; label: ReactNode; content: ReactNode }[]; value?: string; onChange?: (id: string) => void; label: string;
}) {
  return (
    <RadixTabs.Root className="tabs" value={value} defaultValue={value === undefined ? tabs[0]?.id : undefined} onValueChange={onChange}>
      <RadixTabs.List className="tabs__list" aria-label={label}>
        {tabs.map((tab) => <RadixTabs.Trigger key={tab.id} value={tab.id} className="tabs__tab">{tab.label}</RadixTabs.Trigger>)}
      </RadixTabs.List>
      {tabs.map((tab) => <RadixTabs.Content key={tab.id} value={tab.id} className="tabs__panel">{tab.content}</RadixTabs.Content>)}
    </RadixTabs.Root>
  );
}

/** Des sections qui se déplient (questions fréquentes, détails d'une dépense). */
export function Accordion({ items, multiple }: { items: { id: string; title: ReactNode; content: ReactNode }[]; multiple?: boolean }) {
  const parts = items.map((item) => (
    <RadixAccordion.Item key={item.id} value={item.id} className="accordion__item">
      <RadixAccordion.Header className="accordion__header">
        <RadixAccordion.Trigger className="accordion__trigger">{item.title}<Icon icon={ChevronDown} size={18} /></RadixAccordion.Trigger>
      </RadixAccordion.Header>
      <RadixAccordion.Content className="accordion__content">{item.content}</RadixAccordion.Content>
    </RadixAccordion.Item>
  ));
  return multiple
    ? <RadixAccordion.Root type="multiple" className="accordion">{parts}</RadixAccordion.Root>
    : <RadixAccordion.Root type="single" collapsible className="accordion">{parts}</RadixAccordion.Root>;
}

type Choice<T extends string> = { value: T; label: string };

/** Une liste de choix où l'on cherche en tapant (un pays, une catégorie parmi beaucoup). */
export function Combobox<T extends string>({ label, value, onChange, options, placeholder = "Choisir", searchPlaceholder = "Rechercher", emptyText = "Aucun résultat" }: {
  label: string; value: T | ""; onChange: (value: T) => void; options: Choice<T>[]; placeholder?: string; searchPlaceholder?: string; emptyText?: string;
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const plain = (text: string) => text.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
  const shown = useMemo(() => options.filter((option) => plain(option.label).includes(plain(query))), [options, query]);
  const choose = (option: Choice<T>) => { onChange(option.value); setOpen(false); setQuery(""); };
  const current = options.find((option) => option.value === value);
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      <Popover.Root open={open} onOpenChange={(next) => { setOpen(next); setActive(0); }}>
        <Popover.Trigger asChild>
          <button id={id} type="button" className="combobox__trigger" aria-haspopup="listbox">
            <span className={current ? "" : "combobox__placeholder"}>{current ? current.label : placeholder}</span>
            <Icon icon={ChevronsUpDown} size={18} />
          </button>
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content className="popover combobox__panel" align="start" sideOffset={6}>
            <input className="combobox__search" autoFocus value={query} placeholder={searchPlaceholder} aria-label={searchPlaceholder}
              onChange={(event) => { setQuery(event.target.value); setActive(0); }}
              onKeyDown={(event) => {
                if (event.key === "ArrowDown") { event.preventDefault(); setActive((index) => Math.min(index + 1, shown.length - 1)); }
                if (event.key === "ArrowUp") { event.preventDefault(); setActive((index) => Math.max(index - 1, 0)); }
                if (event.key === "Enter" && shown[active]) { event.preventDefault(); choose(shown[active]); }
              }} />
            <div className="combobox__list" role="listbox" aria-label={label}>
              {shown.length === 0 && <p className="combobox__empty">{emptyText}</p>}
              {shown.map((option, index) => (
                <button key={option.value} type="button" role="option" aria-selected={option.value === value}
                  className={`combobox__option${index === active ? " is-active" : ""}`} onClick={() => choose(option)} onMouseEnter={() => setActive(index)}>
                  {option.label}{option.value === value && <Icon icon={Check} size={16} />}
                </button>
              ))}
            </div>
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
    </div>
  );
}

/** Le menu « ⋯ » d'une ligne ou d'un écran : quelques gestes, le dangereux en rouge. */
export function Menu({ label, items, icon = MoreHorizontal }: {
  label: string; icon?: LucideIcon; items: { label: string; icon?: LucideIcon; onSelect: () => void; danger?: boolean }[];
}) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button type="button" className="icon-btn" aria-label={label}><Icon icon={icon} /></button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="popover menu" align="end" sideOffset={6}>
          {items.map((item) => (
            <DropdownMenu.Item key={item.label} className={`menu__item${item.danger ? " is-danger" : ""}`} onSelect={item.onSelect}>
              {item.icon && <Icon icon={item.icon} size={18} />}{item.label}
            </DropdownMenu.Item>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/** Une explication courte au survol ou au toucher prolongé (un bouton fait d'une seule icône). */
export function Hint({ text, children }: { text: string; children: ReactNode }) {
  return (
    <Tooltip.Provider delayDuration={300}>
      <Tooltip.Root>
        <Tooltip.Trigger asChild>{children}</Tooltip.Trigger>
        <Tooltip.Portal><Tooltip.Content className="hint" sideOffset={6}>{text}</Tooltip.Content></Tooltip.Portal>
      </Tooltip.Root>
    </Tooltip.Provider>
  );
}

type Column<R> = { key: keyof R & string; label: string; align?: "left" | "right"; format?: (value: R[keyof R], row: R) => ReactNode; sortable?: boolean };

/** Un tableau : colonnes triables d'un toucher sur leur titre, et pages de `pageSize` lignes. */
export function DataTable<R extends Record<string, unknown>>({ columns, rows, rowKey, pageSize = 10, emptyText = "Rien à montrer", label }: {
  columns: Column<R>[]; rows: R[]; rowKey: (row: R) => string; pageSize?: number; emptyText?: string; label: string;
}) {
  const [sort, setSort] = useState<{ key: string; up: boolean } | null>(null);
  const [page, setPage] = useState(0);
  const sorted = useMemo(() => {
    if (!sort) return rows;
    return [...rows].sort((a, b) => {
      const left = a[sort.key];
      const right = b[sort.key];
      const order = typeof left === "number" && typeof right === "number" ? left - right : String(left ?? "").localeCompare(String(right ?? ""), "fr", { numeric: true });
      return sort.up ? order : -order;
    });
  }, [rows, sort]);
  const pages = Math.max(1, Math.ceil(sorted.length / pageSize));
  const shown = sorted.slice(Math.min(page, pages - 1) * pageSize, Math.min(page, pages - 1) * pageSize + pageSize);
  return (
    <div className="table-wrap">
      <table className="table" aria-label={label}>
        <thead>
          <tr>
            {columns.map((column) => {
              const state = sort?.key === column.key ? (sort.up ? "ascending" : "descending") : "none";
              return (
                <th key={column.key} scope="col" aria-sort={column.sortable === false ? undefined : state} className={column.align === "right" ? "is-right" : ""}>
                  {column.sortable === false ? column.label : (
                    <button type="button" onClick={() => setSort(sort?.key === column.key ? { key: column.key, up: !sort.up } : { key: column.key, up: true })}>
                      {column.label}<span aria-hidden="true">{state === "ascending" ? " ↑" : state === "descending" ? " ↓" : ""}</span>
                    </button>
                  )}
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {shown.length === 0 && <tr><td colSpan={columns.length} className="table__empty">{emptyText}</td></tr>}
          {shown.map((row) => (
            <tr key={rowKey(row)}>
              {columns.map((column) => (
                <td key={column.key} className={column.align === "right" ? "is-right" : ""}>{column.format ? column.format(row[column.key], row) : String(row[column.key] ?? "")}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      {pages > 1 && (
        <div className="table__pages">
          <button type="button" className="icon-btn" aria-label="Page précédente" disabled={page <= 0} onClick={() => setPage(page - 1)}><Icon icon={ChevronLeft} /></button>
          <span>Page {Math.min(page, pages - 1) + 1} sur {pages}</span>
          <button type="button" className="icon-btn" aria-label="Page suivante" disabled={page >= pages - 1} onClick={() => setPage(page + 1)}><Icon icon={ChevronRight} /></button>
        </div>
      )}
    </div>
  );
}

/** La place de ce qui se charge : des lignes grises qui battent doucement. */
export function Skeleton({ lines = 3 }: { lines?: number }) {
  return <div className="skeleton" aria-hidden="true">{Array.from({ length: lines }, (_, index) => <span key={index} />)}</div>;
}

/** Un chargement en cours, dit aux lecteurs d'écran. */
export function Spinner({ label = "Chargement" }: { label?: string }) {
  return <span className="spinner" role="status" aria-label={label} />;
}

/** Les étapes d'un parcours (« Panier », « Livraison », « Paiement ») ; `current` commence à 0. */
export function Steps({ steps, current }: { steps: string[]; current: number }) {
  return (
    <ol className="steps">
      {steps.map((step, index) => (
        <li key={step} className={index < current ? "is-done" : index === current ? "is-current" : ""} aria-current={index === current ? "step" : undefined}>
          <span className="steps__dot">{index < current ? <Icon icon={Check} size={14} /> : index + 1}</span>{step}
        </li>
      ))}
    </ol>
  );
}

/** Une quantité, avec − et + (des places, des articles). */
export function QuantityInput({ label, value, onChange, min = 0, max = 99, step = 1 }: {
  label: string; value: number; onChange: (value: number) => void; min?: number; max?: number; step?: number;
}) {
  const set = (next: number) => onChange(Math.min(max, Math.max(min, next)));
  return (
    <div className="field">
      <span className="field__label">{label}</span>
      <div className="quantity" role="group" aria-label={label}>
        <button type="button" aria-label={`Moins (${label})`} disabled={value <= min} onClick={() => set(value - step)}><Icon icon={Minus} size={18} /></button>
        <output aria-live="polite">{value}</output>
        <button type="button" aria-label={`Plus (${label})`} disabled={value >= max} onClick={() => set(value + step)}><Icon icon={Plus} size={18} /></button>
      </div>
    </div>
  );
}

/** Une note en étoiles ; toucher l'étoile déjà choisie remet à 0. */
export function Rating({ label, value, onChange, max = 5 }: { label: string; value: number; onChange?: (value: number) => void; max?: number }) {
  return (
    <div className="rating" role={onChange ? "radiogroup" : "img"} aria-label={`${label} : ${value} sur ${max}`}>
      {Array.from({ length: max }, (_, index) => {
        const number = index + 1;
        const on = number <= value;
        return onChange ? (
          <button key={number} type="button" role="radio" aria-checked={number === value} aria-label={`${number} sur ${max}`} className={on ? "is-on" : ""} onClick={() => onChange(number === value ? 0 : number)}>
            <Icon icon={Star} size={24} />
          </button>
        ) : <span key={number} className={on ? "is-on" : ""}><Icon icon={Star} size={20} /></span>;
      })}
    </div>
  );
}

/** Un curseur (un budget, une distance), avec sa valeur écrite. */
export function Slider({ label, value, onChange, min = 0, max = 100, step = 1, format = String }: {
  label: string; value: number; onChange: (value: number) => void; min?: number; max?: number; step?: number; format?: (value: number) => string;
}) {
  return (
    <div className="field">
      <span className="field__label slider__label">{label}<b>{format(value)}</b></span>
      <RadixSlider.Root className="slider" value={[value]} min={min} max={max} step={step} onValueChange={([next]) => onChange(next)}>
        <RadixSlider.Track className="slider__track"><RadixSlider.Range className="slider__range" /></RadixSlider.Track>
        <RadixSlider.Thumb className="slider__thumb" aria-label={label} />
      </RadixSlider.Root>
    </div>
  );
}

/** Un code reçu par SMS ou e-mail : une case par chiffre, le collage rempli d'un coup. */
export function CodeInput({ label, value, onChange, length = 6 }: { label: string; value: string; onChange: (value: string) => void; length?: number }) {
  const boxes = useRef<(HTMLInputElement | null)[]>([]);
  const put = (index: number, text: string) => {
    const digits = text.replace(/\D/g, "");
    if (!digits) return;
    const next = (value.slice(0, index) + digits).slice(0, length);
    onChange(next);
    boxes.current[Math.min(next.length, length - 1)]?.focus();
  };
  return (
    <div className="field">
      <span className="field__label">{label}</span>
      <div className="code" role="group" aria-label={label}>
        {Array.from({ length }, (_, index) => (
          <input key={index} ref={(node) => { boxes.current[index] = node; }} inputMode="numeric" autoComplete={index === 0 ? "one-time-code" : "off"}
            aria-label={`Chiffre ${index + 1}`} value={value[index] ?? ""}
            onChange={(event) => put(index, event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Backspace") {
                event.preventDefault();
                onChange(value.slice(0, value[index] ? index : Math.max(index - 1, 0)));
                boxes.current[Math.max(value[index] ? index : index - 1, 0)]?.focus();
              }
            }}
            onPaste={(event) => { event.preventDefault(); put(index, event.clipboardData.getData("text")); }} />
        ))}
      </div>
    </div>
  );
}

/** Une zone où déposer des fichiers, ou les choisir d'un toucher (photos, justificatifs). */
export function FileDrop({ label, onFiles, accept, multiple, note }: {
  label: string; onFiles: (files: File[]) => void; accept?: string; multiple?: boolean; note?: string;
}) {
  const [over, setOver] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  return (
    <div className={`file-drop${over ? " is-over" : ""}`} role="button" tabIndex={0} aria-label={label}
      onClick={() => input.current?.click()} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") input.current?.click(); }}
      onDragOver={(event) => { event.preventDefault(); setOver(true); }} onDragLeave={() => setOver(false)}
      onDrop={(event) => { event.preventDefault(); setOver(false); onFiles([...event.dataTransfer.files]); }}>
      <Icon icon={Upload} size={28} />
      <b>{label}</b>
      {note && <small>{note}</small>}
      <input ref={input} type="file" hidden accept={accept} multiple={multiple} onChange={(event) => { onFiles([...(event.target.files ?? [])]); event.target.value = ""; }} />
    </div>
  );
}

/** Le calendrier d'un mois (« 2026-10 ») : choisir un jour, voir les jours marqués (ceux qui ont
 * une dépense, une séance) ; les semaines commencent le lundi. */
export function MonthCalendar({ value, onChange, marked = [], month: shown, onMonth }: {
  value?: string; onChange?: (day: string) => void; marked?: string[]; month?: string; onMonth?: (month: string) => void;
}) {
  const [own, setOwn] = useState((value || today()).slice(0, 7));
  const month = shown ?? own;
  const go = (step: number) => { const next = shiftMonth(month, step); setOwn(next); onMonth?.(next); };
  const [year, number] = month.split("-").map(Number);
  const first = (new Date(year, number - 1, 1).getDay() + 6) % 7;
  const count = new Date(year, number, 0).getDate();
  const days = Array.from({ length: count }, (_, index) => `${month}-${String(index + 1).padStart(2, "0")}`);
  const now = today();
  const set = new Set(marked);
  return (
    <div className="calendar">
      <div className="calendar__head">
        <button type="button" className="icon-btn" aria-label="Mois précédent" onClick={() => go(-1)}><Icon icon={ChevronLeft} /></button>
        <b>{monthName(month)}</b>
        <button type="button" className="icon-btn" aria-label="Mois suivant" onClick={() => go(1)}><Icon icon={ChevronRight} /></button>
      </div>
      <div className="calendar__grid" role="grid" aria-label={monthName(month)}>
        {["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."].map((name) => <span key={name} className="calendar__weekday" aria-hidden="true">{name.slice(0, 1).toUpperCase()}</span>)}
        {Array.from({ length: first }, (_, index) => <span key={`vide-${index}`} />)}
        {days.map((day) => (
          <button key={day} type="button" aria-pressed={day === value} aria-current={day === now ? "date" : undefined}
            className={`calendar__day${day === now ? " is-today" : ""}${set.has(day) ? " is-marked" : ""}`} onClick={() => onChange?.(day)}>
            {Number(day.slice(8))}
          </button>
        ))}
      </div>
    </div>
  );
}
