// Boutons, choix, champs et réglages.

import { useEffect, useId, useState, type JSX, type ReactNode } from "react";
import { parseAmount } from "./format";
import { Icon, type LucideIcon } from "./icons";

type ButtonProps = {
  children: ReactNode;
  variant?: "primary" | "danger" | "plain";
  small?: boolean;
  block?: boolean;
  icon?: LucideIcon;
  type?: "button" | "submit";
  onClick?: () => void;
  disabled?: boolean;
};

export function Button({ children, variant = "plain", small, block, icon, type = "button", onClick, disabled }: ButtonProps) {
  const classes = ["btn", variant !== "plain" && `btn--${variant}`, small && "btn--small", block && "btn--block"].filter(Boolean).join(" ");
  return (
    <button className={classes} type={type} onClick={onClick} disabled={disabled}>
      {icon && <Icon icon={icon} size={18} />}
      {children}
    </button>
  );
}

/** Des boutons côte à côte, de même largeur. */
export function ButtonRow({ children }: { children: ReactNode }) {
  return <div className="btn-row">{children}</div>;
}

type Option<T extends string> = { value: T; label: string };

/** Un choix entre 2 à 4 options, en pastilles collées (vue, période). */
export function Segmented<T extends string>({ options, value, onChange, label }: { options: Option<T>[]; value: T; onChange: (value: T) => void; label: string }) {
  return (
    <div className="seg" role="group" aria-label={label}>
      {options.map((option) => (
        <button key={option.value} type="button" aria-pressed={option.value === value} onClick={() => onChange(option.value)}>
          {option.label}
        </button>
      ))}
    </div>
  );
}

type ChipsProps<T extends string> = { options: Option<T>[]; label: string };

/** Des filtres en pastilles qui défilent (catégories) ; la valeur "" : tout. Avec une liste pour
 * valeur, plusieurs pastilles se choisissent (pour qui est une dépense) : chacune s'allume ou
 * s'éteint au toucher. */
export function Chips<T extends string>(props: ChipsProps<T> & { value: T[]; onChange: (value: T[]) => void }): JSX.Element;
export function Chips<T extends string>(props: ChipsProps<T> & { value: T; onChange: (value: T) => void }): JSX.Element;
export function Chips<T extends string>({ options, value, onChange, label }: ChipsProps<T> & { value: T | T[]; onChange: ((value: T[]) => void) | ((value: T) => void) }) {
  const pick = (choice: T) => {
    if (Array.isArray(value)) (onChange as (value: T[]) => void)(value.includes(choice) ? value.filter((item) => item !== choice) : [...value, choice]);
    else (onChange as (value: T) => void)(choice);
  };
  return (
    <div className="chips" role="group" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          className="chip"
          type="button"
          aria-pressed={Array.isArray(value) ? value.includes(option.value) : option.value === value}
          onClick={() => pick(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

type TextFieldProps = {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: "text" | "date" | "email" | "tel" | "number";
  placeholder?: string;
  help?: string;
  error?: string;
};

/** Un champ avec son nom au-dessus, une aide et une erreur dessous. `type="number"` : le clavier
 * des chiffres, et la valeur telle qu'elle est tapée, virgule comprise (« 12,50 » ; parseAmount()
 * la lit). Un vrai <input type="number"> refuse la virgule, ou la perd au clavier d'un ordinateur
 * (12,50 devenait 1250 : essai C9 de Claw Web, 30 octobre 2026). */
export function TextField({ label, value, onChange, type = "text", placeholder, help, error }: TextFieldProps) {
  const id = useId();
  const number = type === "number";
  return (
    <div className={`field${error ? " is-invalid" : ""}`}>
      <label htmlFor={id}>{label}</label>
      <input id={id} type={number ? "text" : type} inputMode={number ? "decimal" : undefined} value={value} placeholder={placeholder} autoComplete="off" aria-invalid={Boolean(error)} onChange={(event) => onChange(event.target.value)} />
      {help && <p className="field__help">{help}</p>}
      {error && <p className="field__error">{error}</p>}
    </div>
  );
}

/** Un montant à saisir (« 1 200 », « 12,50 ») ; `onChange` reçoit un nombre valide. */
export function MoneyInput({ value, onChange, label }: { value: number; onChange: (value: number) => void; label: string }) {
  const shown = (amount: number) => (Number.isInteger(amount) ? String(amount) : amount.toFixed(2).replace(".", ","));
  const [text, setText] = useState(shown(value));
  useEffect(() => setText(shown(value)), [value]);
  return (
    <label className="money">
      <input
        inputMode="decimal"
        aria-label={label}
        value={text}
        onChange={(event) => {
          setText(event.target.value);
          const amount = parseAmount(event.target.value);
          if (Number.isFinite(amount) && amount >= 0) onChange(Math.round(amount * 100) / 100);
        }}
        onBlur={() => setText(shown(value))}
      />
      <span aria-hidden="true">€</span>
    </label>
  );
}

/** Un interrupteur (oui ou non) ; `label` est lu à voix haute. */
export function Switch({ checked, onChange, label }: { checked: boolean; onChange: (checked: boolean) => void; label: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className="relative inline-flex h-8 w-13 flex-none items-center rounded-full bg-line p-0.5 transition-colors aria-checked:bg-accent"
    >
      <span className={`size-7 rounded-full bg-white shadow transition-transform duration-200 ${checked ? "translate-x-5" : ""}`} />
    </button>
  );
}

/** Des réglages, un par ligne (des <Setting>). */
export function Settings({ children }: { children: ReactNode }) {
  return <div className="settings">{children}</div>;
}

/** Un réglage : son nom (avec une précision dessous) et sa valeur à droite (<MoneyInput>, interrupteur…). */
export function Setting({ name, note, children }: { name: ReactNode; note?: ReactNode; children: ReactNode }) {
  return (
    <div className="setting">
      <span className="setting__name">{name}{note && <small>{note}</small>}</span>
      {children}
    </div>
  );
}
