// L'argent du mois, prêt à l'emploi : les rentrées et dépenses qui reviennent chaque mois
// (salaire, loyer, forfait), l'import d'un relevé de banque sans doublon, le classement des
// opérations par catégorie d'après leur libellé, et le calendrier des jours de rentrées et de
// dépenses. Vu le 31 octobre 2026 (session #46) : le modèle avait écrit lui-même ces quatre
// choses, et ses rentrées « de chaque mois » se rajoutaient à chaque changement de mois.

import { ChevronLeft, ChevronRight, Pencil, Repeat, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { create } from "zustand";
import { persist } from "zustand/middleware";
import { Alert } from "./extras";
import { Empty, List, Row } from "./blocks";
import { FileDrop } from "./widgets";
import { dayLabel, money, monthName, monthOf, parseAmount, shiftMonth, today } from "./format";
import { Icon, iconFor } from "./icons";
import { useEntries, type Entry } from "./store";
import { toast } from "./toast";

const plain = (text: string) =>
  String(text || "").toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/\s+/g, " ").trim();

// --- Catégories d'après le libellé ---------------------------------------------------------

/** Mots d'un libellé de banque → catégorie. Les règles apprises de l'utilisateur passent avant. */
export const CATEGORY_WORDS: Record<string, string[]> = {
  Courses: ["carrefour", "leclerc", "auchan", "lidl", "aldi", "intermarche", "monoprix", "franprix", "casino", "super u", "hyper u", "picard", "biocoop", "netto"],
  Transport: ["sncf", "ratp", "navigo", "uber", "bolt", "blablacar", "total", "esso", "shell", "bp ", "peage", "parking", "tcl", "tisseo"],
  Sorties: ["restaurant", "resto", "mcdo", "mcdonald", "burger king", "kfc", "starbucks", "deliveroo", "uber eats", "just eat", "bar ", "cafe", "cinema", "ugc", "pathe"],
  Logement: ["loyer", "edf", "engie", "veolia", "eau ", "assurance habitation", "syndic"],
  Abonnements: ["netflix", "spotify", "deezer", "disney", "amazon prime", "canal", "free mobile", "sfr", "orange", "bouygues", "claude", "openai", "apple.com", "google"],
  Santé: ["pharmacie", "docteur", "medecin", "mutuelle", "dentiste", "ameli", "cpam"],
  Shopping: ["amazon", "fnac", "zara", "h&m", "decathlon", "ikea", "cdiscount", "vinted", "leroy merlin"],
  Revenus: ["salaire", "paie", "caf", "virement recu", "remboursement", "pole emploi", "france travail", "mobili"],
};

type RulesState = { learned: Record<string, string>; learn: (label: string, category: string) => void };

/** Les catégories que l'utilisateur a choisies à la main, retenues par libellé. */
export const useCategoryRules = create<RulesState>()(
  persist((set, get) => ({
    learned: {},
    learn: (label, category) => {
      const key = labelKey(label);
      if (key) set({ learned: { ...get().learned, [key]: category } });
    },
  }), { name: "appli-categories" }),
);

/** Le libellé sans ce qui change d'un relevé à l'autre (dates, numéros de carte, références). */
export function labelKey(label: string): string {
  return plain(label)
    .replace(/\b(cb|carte|prlv|prelevement|vir|virement|sepa|paiement|achat|du|le|x?\d{4,})\b/g, " ")
    .replace(/\d{2}[/.-]\d{2}([/.-]\d{2,4})?/g, " ")
    .replace(/[^a-z& ]/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .split(" ")
    .slice(0, 3)
    .join(" ");
}

/** La catégorie d'un libellé : celle que l'utilisateur a choisie pour un libellé pareil, sinon
 * d'après ses mots (CARREFOUR → Courses, SNCF → Transport), sinon `fallback`. */
export function categorize(label: string, fallback = "Autre"): string {
  const learned = useCategoryRules.getState().learned[labelKey(label)];
  if (learned) return learned;
  const text = ` ${plain(label)} `;
  for (const [category, words] of Object.entries(CATEGORY_WORDS)) {
    if (words.some((word) => text.includes(word.endsWith(" ") ? ` ${word}` : word))) return category;
  }
  return fallback;
}

/** Change la catégorie d'une opération et la retient pour les libellés pareils. */
export function recategorize(entry: Entry, category: string): void {
  useEntries.getState().update(entry.id, { category });
  useCategoryRules.getState().learn(entry.label, category);
}

// --- Relevé de banque ------------------------------------------------------------------------

export type BankLine = { date: string; label: string; amount: number };

/** « 28/10/2026 », « 28/10/26 », « 2026-10-28 » → « 2026-10-28 » ; "" si ce n'est pas une date. */
function bankDate(text: string): string {
  const value = text.trim();
  let found = value.match(/^(\d{4})-(\d{1,2})-(\d{1,2})/);
  if (found) return `${found[1]}-${found[2].padStart(2, "0")}-${found[3].padStart(2, "0")}`;
  found = value.match(/^(\d{1,2})[/.-](\d{1,2})[/.-](\d{2,4})$/);
  if (!found) return "";
  const year = found[3].length === 2 ? `20${found[3]}` : found[3];
  return `${year}-${found[2].padStart(2, "0")}-${found[1].padStart(2, "0")}`;
}

/** Les lignes d'un relevé de banque en CSV : points-virgules ou virgules, guillemets, dates
 * JJ/MM/AAAA, montants « -1 234,56 » ou colonnes Débit et Crédit séparées. Les lignes
 * illisibles (titres, totaux) sont laissées de côté. */
export function parseBankCsv(text: string): BankLine[] {
  const body = text.replace(/^﻿/, "");
  const first = body.split(/\r?\n/, 1)[0] ?? "";
  const separator = (first.match(/;/g) || []).length >= (first.match(/,/g) || []).length ? ";" : ",";
  const rows: string[][] = [];
  let cells: string[] = [];
  let value = "";
  let quoted = false;
  for (let index = 0; index < body.length; index += 1) {
    const char = body[index];
    if (quoted) {
      if (char === '"' && body[index + 1] === '"') { value += '"'; index += 1; }
      else if (char === '"') quoted = false;
      else value += char;
    } else if (char === '"') quoted = true;
    else if (char === separator) { cells.push(value); value = ""; }
    else if (char === "\n" || char === "\r") {
      if (char === "\r" && body[index + 1] === "\n") index += 1;
      cells.push(value); rows.push(cells); cells = []; value = "";
    } else value += char;
  }
  if (value || cells.length) { cells.push(value); rows.push(cells); }
  // Les colonnes, d'après la ligne de titres si elle est là.
  const head = rows.find((row) => row.some((cell) => /date/i.test(cell))) ?? [];
  const find = (...words: string[]) => head.findIndex((cell) => words.some((word) => plain(cell).includes(word)));
  let dateAt = find("date");
  let labelAt = find("libelle", "label", "description", "operation", "intitule");
  let amountAt = find("montant", "amount", "somme");
  const debitAt = find("debit");
  const creditAt = find("credit");
  if (dateAt < 0) { dateAt = 0; labelAt = 1; amountAt = 2; }
  if (labelAt < 0) labelAt = dateAt === 0 ? 1 : 0;
  const lines: BankLine[] = [];
  for (const row of rows) {
    const date = bankDate(row[dateAt] ?? "");
    if (!date) continue;
    let amount = NaN;
    if (amountAt >= 0) amount = parseAmount(row[amountAt] ?? "");
    else if (debitAt >= 0 || creditAt >= 0) {
      const debit = parseAmount(row[debitAt] ?? "");
      const credit = parseAmount(row[creditAt] ?? "");
      amount = (Number.isFinite(credit) ? Math.abs(credit) : 0) - (Number.isFinite(debit) ? Math.abs(debit) : 0);
    }
    const label = String(row[labelAt] ?? "").replace(/\s+/g, " ").trim();
    if (!label || !Number.isFinite(amount) || amount === 0) continue;
    lines.push({ date, label, amount: Math.round(amount * 100) / 100 });
  }
  return lines;
}

/** L'identifiant d'une ligne de relevé : le même pour la même opération dans deux relevés qui
 * se chevauchent ; deux opérations pareilles le même jour gardent chacune le leur. */
export function bankIds(lines: BankLine[]): string[] {
  const seen = new Map<string, number>();
  return lines.map((line) => {
    const key = `${line.date}|${line.amount.toFixed(2)}|${labelKey(line.label) || plain(line.label)}`;
    const count = (seen.get(key) ?? 0) + 1;
    seen.set(key, count);
    return `bank:${key}|${count}`;
  });
}

/** Ajoute les opérations d'un relevé (rangées par `categorize`), sans celles déjà là ; rend le
 * nombre d'ajoutées et de déjà là, et propose d'annuler. */
export async function importBankCsv(file: File | string): Promise<{ added: number; known: number; unreadable: boolean }> {
  const text = typeof file === "string" ? file : await file.text();
  const lines = parseBankCsv(text);
  if (!lines.length) {
    toast("Ce fichier ne contient aucune opération lisible (date, libellé, montant).", { tone: "error" });
    return { added: 0, known: 0, unreadable: true };
  }
  const ids = bankIds(lines);
  const before = useEntries.getState().entries;
  const known = new Set(before.map((entry) => entry.id));
  const fresh: Entry[] = lines
    .map((line, index) => ({ id: ids[index], label: line.label, amount: line.amount, date: line.date, category: categorize(line.label, line.amount > 0 ? "Revenus" : "Autre") }))
    .filter((entry) => !known.has(entry.id));
  const result = { added: fresh.length, known: lines.length - fresh.length, unreadable: false };
  if (!fresh.length) {
    toast(`Rien de nouveau : les ${lines.length} opérations de ce relevé sont déjà là.`);
    return result;
  }
  useEntries.setState({ entries: [...before, ...fresh] });
  const newest = fresh.reduce((latest, entry) => (entry.date > latest ? entry.date : latest), fresh[0].date);
  useEntries.getState().setMonth(monthOf(newest));
  toast(`${fresh.length} opération${fresh.length > 1 ? "s" : ""} ajoutée${fresh.length > 1 ? "s" : ""}${result.known ? `, ${result.known} déjà là` : ""}`, {
    action: { label: "Annuler", run: () => useEntries.setState({ entries: before }) },
  });
  return result;
}

/** Le relevé de la banque à reprendre : le fichier touché ou déposé est lu, rangé par
 * catégorie, sans doublon avec les relevés déjà repris ; le résultat est dit dessous. */
export function BankImport({ label = "Reprendre un relevé de banque", note = "Le fichier .csv que donne votre banque" }: { label?: string; note?: string }) {
  const [result, setResult] = useState<{ added: number; known: number; unreadable: boolean } | null>(null);
  return (
    <div className="bank-import">
      <FileDrop label={label} note={note} accept=".csv,text/csv,text/plain" onFiles={(files) => { if (files[0]) void importBankCsv(files[0]).then(setResult); }} />
      {result && (result.unreadable ? (
        <Alert tone="error" title="Fichier illisible">Il faut un fichier .csv avec une date, un libellé et un montant par ligne.</Alert>
      ) : (
        <Alert tone="success" title={result.added ? `${result.added} opération${result.added > 1 ? "s" : ""} ajoutée${result.added > 1 ? "s" : ""}` : "Rien de nouveau"}>
          {result.known ? `${result.known} déjà là, pas comptée${result.known > 1 ? "s" : ""} deux fois.` : "Toutes sont nouvelles."}
        </Alert>
      ))}
    </div>
  );
}

// --- Rentrées et dépenses de chaque mois -----------------------------------------------------

export type Recurring = {
  id: string;
  label: string;
  /** Positif : une rentrée (salaire) ; négatif : une dépense (loyer). */
  amount: number;
  category: string;
  /** Le jour du mois (1 à 28 ; plus tard, le dernier jour du mois). */
  day: number;
  /** Le premier mois où elle compte, « 2026-10 » : jamais les mois d'avant. */
  since: string;
};

type RecurringState = {
  items: Recurring[];
  /** Les opérations déjà posées (`rec:<id>:<mois>`) : une opération supprimée ne revient pas. */
  applied: string[];
  add: (item: Omit<Recurring, "id" | "since"> & { since?: string }) => void;
  update: (id: string, fields: Partial<Omit<Recurring, "id">>) => void;
  remove: (id: string) => void;
};

let recurringCounter = 0;
const recurringEntryId = (item: Recurring, month: string) => `rec:${item.id}:${month}`;

/** Les rentrées et dépenses de chaque mois, gardées sur le téléphone. Chacune devient une vraie
 * opération du mois affiché, une seule fois par mois, même après un rechargement ou un aller-
 * retour entre les mois. */
export const useRecurring = create<RecurringState>()(
  persist((set, get) => ({
    items: [],
    applied: [],
    add: (item) => {
      const created: Recurring = {
        ...item,
        id: `${Date.now().toString(36)}${(recurringCounter++).toString(36)}`,
        amount: Math.round(item.amount * 100) / 100,
        day: Math.min(31, Math.max(1, Math.round(item.day) || 1)),
        since: item.since ?? useEntries.getState().month,
      };
      set({ items: [...get().items, created] });
    },
    update: (id, fields) => {
      set({ items: get().items.map((item) => (item.id === id ? { ...item, ...fields } : item)) });
      // Les opérations déjà posées du mois affiché et des suivants suivent le changement.
      const changed = get().items.find((item) => item.id === id);
      if (!changed) return;
      const month = useEntries.getState().month;
      useEntries.setState({
        entries: useEntries.getState().entries.map((entry) => {
          const [, owner, of] = entry.id.split(":");
          return entry.id.startsWith("rec:") && owner === id && of >= month
            ? { ...entry, label: changed.label, amount: changed.amount, category: changed.category, date: dayOf(of, changed.day) }
            : entry;
        }),
      });
    },
    remove: (id) => {
      set({ items: get().items.filter((item) => item.id !== id) });
      // Les mois à venir perdent l'opération ; les mois passés gardent ce qui a eu lieu.
      const current = monthOf(today());
      useEntries.setState({
        entries: useEntries.getState().entries.filter((entry) => {
          const [, owner, of] = entry.id.split(":");
          return !(entry.id.startsWith("rec:") && owner === id && of > current);
        }),
      });
    },
  }), { name: "appli-recurring" }),
);

/** « 2026-02 », jour 31 → « 2026-02-28 ». */
function dayOf(month: string, day: number): string {
  const [year, number] = month.split("-").map(Number);
  const last = new Date(year, number, 0).getDate();
  return `${month}-${String(Math.min(day, last)).padStart(2, "0")}`;
}

/** Pose les opérations de chaque mois qui manquent au mois `month` ; rien de ce qui a déjà été
 * posé (ou supprimé par l'utilisateur) ne revient. */
export function applyRecurring(month = useEntries.getState().month): void {
  const { items, applied } = useRecurring.getState();
  const done = new Set(applied);
  const due = items.filter((item) => item.since <= month && !done.has(recurringEntryId(item, month)));
  if (!due.length) return;
  const have = new Set(useEntries.getState().entries.map((entry) => entry.id));
  const fresh: Entry[] = due
    .filter((item) => !have.has(recurringEntryId(item, month)))
    .map((item) => ({ id: recurringEntryId(item, month), label: item.label, amount: item.amount, category: item.category, date: dayOf(month, item.day) }));
  useRecurring.setState({ applied: [...applied, ...due.map((item) => recurringEntryId(item, month))] });
  if (fresh.length) useEntries.setState({ entries: [...useEntries.getState().entries, ...fresh] });
}

// Le mois affiché change, une rentrée est ajoutée, la page se recharge : les opérations du mois
// sont posées, une fois.
if (typeof window !== "undefined") {
  let shown = "";
  useEntries.subscribe((state) => {
    if (state.month !== shown) {
      shown = state.month;
      applyRecurring(state.month);
    }
  });
  useRecurring.subscribe((state, previous) => {
    if (state.items !== previous.items) applyRecurring();
  });
  queueMicrotask(() => applyRecurring());
}

/** Total par mois des rentrées (`kind` "in") ou des dépenses ("out") de chaque mois. */
export function recurringTotal(kind: "in" | "out", items = useRecurring.getState().items): number {
  return Math.abs(items.filter((item) => (kind === "in" ? item.amount > 0 : item.amount < 0)).reduce((sum, item) => sum + item.amount, 0));
}

/** La liste à régler des rentrées (`kind` "in" : salaire, CAF) ou des dépenses ("out" : loyer,
 * forfait) de chaque mois : ajouter, modifier, supprimer ; chacune devient une opération de
 * chaque mois à partir de celui-ci. */
export function RecurringList({ kind, categories = [], emptyText }: { kind: "in" | "out"; categories?: string[]; emptyText?: string }) {
  const items = useRecurring((state) => state.items).filter((item) => (kind === "in" ? item.amount > 0 : item.amount < 0));
  const { add, update, remove } = useRecurring();
  const [editing, setEditing] = useState<Recurring | null>(null);
  const [open, setOpen] = useState(false);
  const [label, setLabel] = useState("");
  const [amount, setAmount] = useState("");
  const [day, setDay] = useState("1");
  const [category, setCategory] = useState("");
  const [error, setError] = useState("");
  const start = (item: Recurring | null) => {
    setEditing(item);
    setOpen(true);
    setLabel(item?.label ?? "");
    setAmount(item ? String(Math.abs(item.amount)).replace(".", ",") : "");
    setDay(String(item?.day ?? 1));
    setCategory(item?.category ?? categories[0] ?? (kind === "in" ? "Revenus" : "Logement"));
    setError("");
  };
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const value = parseAmount(amount);
    const when = Number(day);
    if (!label.trim()) return setError("Un nom, comme Loyer ou Salaire.");
    if (!(value > 0)) return setError("Un montant comme 450 ou 1 235,15.");
    if (!(when >= 1 && when <= 31)) return setError("Un jour du mois, de 1 à 31.");
    const fields = { label: label.trim(), amount: kind === "in" ? value : -value, category: category.trim() || categorize(label), day: Math.round(when) };
    if (editing) update(editing.id, fields);
    else add(fields);
    toast(editing ? "Modifié" : `« ${fields.label} » compté chaque mois`);
    setOpen(false);
  };
  return (
    <div className="recurring">
      {items.length ? (
        <List label={kind === "in" ? "Rentrées de chaque mois" : "Dépenses de chaque mois"}>
          {items.map((item) => (
            <Row key={item.id} icon={iconFor(item.category, item.label)} tone={kind === "in" ? "in" : "accent"} title={item.label}
              sub={`le ${item.day === 1 ? "1er" : item.day} du mois${item.category ? ` · ${item.category}` : ""}`} amount={item.amount}
              action={{ icon: Pencil, label: `Modifier « ${item.label} »`, onClick: () => start(item) }} />
          ))}
        </List>
      ) : (
        !open && <Empty icon={Repeat} text={emptyText ?? (kind === "in" ? "Aucune rentrée de chaque mois (salaire, CAF…)." : "Aucune dépense de chaque mois (loyer, forfait…).")} />
      )}
      {open ? (
        <form className="recurring__form" onSubmit={submit} noValidate>
          <div className="field"><label htmlFor={`rec-${kind}-label`}>Nom</label>
            <input id={`rec-${kind}-label`} value={label} placeholder={kind === "in" ? "Salaire" : "Loyer"} autoComplete="off" onChange={(event) => setLabel(event.target.value)} /></div>
          <div className="field-row">
            <div className="field"><label htmlFor={`rec-${kind}-amount`}>Montant</label>
              <input id={`rec-${kind}-amount`} inputMode="decimal" value={amount} placeholder="0,00" onChange={(event) => setAmount(event.target.value)} /></div>
            <div className="field"><label htmlFor={`rec-${kind}-day`}>Jour du mois</label>
              <input id={`rec-${kind}-day`} inputMode="numeric" value={day} onChange={(event) => setDay(event.target.value.replace(/\D/g, "").slice(0, 2))} /></div>
          </div>
          {categories.length > 0 && (
            <div className="field"><label htmlFor={`rec-${kind}-category`}>Catégorie</label>
              <select id={`rec-${kind}-category`} value={category} onChange={(event) => setCategory(event.target.value)}>
                {categories.map((name) => <option key={name} value={name}>{name}</option>)}
              </select></div>
          )}
          {error && <p className="recurring__error" role="alert">{error}</p>}
          <div className="btn-row">
            <button className="btn btn--small" type="button" onClick={() => setOpen(false)}>Annuler</button>
            <button className="btn btn--primary btn--small" type="submit">{editing ? "Enregistrer" : "Ajouter"}</button>
          </div>
          {editing && (
            <button className="btn btn--danger btn--small btn--block" type="button" onClick={() => { remove(editing.id); setOpen(false); toast(`« ${editing.label} » retiré des mois à venir`); }}>
              <Icon icon={Trash2} size={18} />Ne plus compter chaque mois
            </button>
          )}
        </form>
      ) : (
        <button className="btn btn--small btn--block recurring__add" type="button" onClick={() => start(null)}>
          {kind === "in" ? "Ajouter une rentrée de chaque mois" : "Ajouter une dépense de chaque mois"}
        </button>
      )}
    </div>
  );
}

// --- Calendrier des rentrées et des dépenses -------------------------------------------------

/** Le calendrier du mois affiché : un point vert les jours de rentrée, un point rouge les jours de
 * dépense, la dépense du jour en petit ; un jour touché montre ses opérations dessous. */
export function MoneyCalendar({ month: fixed, onMonth }: { month?: string; onMonth?: (month: string) => void }) {
  const entries = useEntries((state) => state.entries);
  const shownMonth = useEntries((state) => state.month);
  const setMonth = useEntries((state) => state.setMonth);
  const month = fixed ?? shownMonth;
  const [picked, setPicked] = useState<string | null>(null);
  useEffect(() => setPicked(null), [month]);
  const byDay = useMemo(() => {
    const days = new Map<string, Entry[]>();
    for (const entry of entries) if (entry.date.startsWith(month)) days.set(entry.date, [...(days.get(entry.date) ?? []), entry]);
    return days;
  }, [entries, month]);
  const [year, number] = month.split("-").map(Number);
  const count = new Date(year, number, 0).getDate();
  const offset = (new Date(year, number - 1, 1).getDay() + 6) % 7;
  const now = today();
  const go = (step: number) => (onMonth ?? setMonth)(shiftMonth(month, step));
  const shown = picked ? byDay.get(picked) ?? [] : [];
  const compact = new Intl.NumberFormat(document.documentElement.lang || "fr", { maximumFractionDigits: 0 });
  return (
    <div className="money-cal">
      <div className="money-cal__head">
        <button className="icon-btn" type="button" aria-label="Mois précédent" onClick={() => go(-1)}><Icon icon={ChevronLeft} /></button>
        <b>{monthName(month)}</b>
        <button className="icon-btn" type="button" aria-label="Mois suivant" onClick={() => go(1)}><Icon icon={ChevronRight} /></button>
      </div>
      <div className="money-cal__grid" role="grid" aria-label={`Rentrées et dépenses, ${monthName(month)}`}>
        {["L", "M", "M", "J", "V", "S", "D"].map((name, index) => <span key={index} className="money-cal__weekday" aria-hidden="true">{name}</span>)}
        {Array.from({ length: offset }, (_, index) => <span key={`vide-${index}`} />)}
        {Array.from({ length: count }, (_, index) => {
          const date = `${month}-${String(index + 1).padStart(2, "0")}`;
          const list = byDay.get(date) ?? [];
          const income = list.filter((entry) => entry.amount > 0).reduce((sum, entry) => sum + entry.amount, 0);
          const spent = Math.abs(list.filter((entry) => entry.amount < 0).reduce((sum, entry) => sum + entry.amount, 0));
          const label = `${dayLabel(date)}${income ? `, rentrées ${money(income)}` : ""}${spent ? `, dépenses ${money(spent)}` : ""}`;
          return (
            <button key={date} type="button" className={`money-cal__day${date === now ? " is-today" : ""}${date === picked ? " is-picked" : ""}`}
              aria-label={label} aria-pressed={date === picked} onClick={() => setPicked(date === picked ? null : date)}>
              <span className="money-cal__number">{index + 1}</span>
              <span className="money-cal__dots" aria-hidden="true">
                {income > 0 && <i className="money-cal__dot money-cal__dot--in" />}
                {spent > 0 && <i className="money-cal__dot money-cal__dot--out" />}
              </span>
              <span className="money-cal__spent" aria-hidden="true">{spent > 0 ? compact.format(spent) : ""}</span>
            </button>
          );
        })}
      </div>
      <p className="money-cal__legend"><span className="chart__key chart__key--in">Rentrée</span><span className="chart__key chart__key--out">Dépense</span></p>
      {picked && (
        shown.length ? (
          <List label={dayLabel(picked)}>
            {shown.map((entry) => (
              <Row key={entry.id} icon={iconFor(entry.category, entry.label)} tone={entry.amount > 0 ? "in" : "accent"} title={entry.label} sub={entry.category} amount={entry.amount} />
            ))}
          </List>
        ) : (
          <p className="money-cal__none">Rien le {dayLabel(picked)}.</p>
        )
      )}
    </div>
  );
}

/** Un montant de chaque mois en une ligne (« 1 438,15 € de rentrées, 589,60 € de dépenses »). */
export function recurringSummary(): string {
  return `${money(recurringTotal("in"))} de rentrées, ${money(recurringTotal("out"))} de dépenses chaque mois`;
}

