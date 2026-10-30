// Fenêtres natives (<dialog>) : un volet qui monte du bas sur un téléphone (Sheet), une carte
// au centre (Modal), la confirmation d'un geste qui efface (ConfirmModal), et le volet d'ajout
// et de modification d'une opération (EntrySheet, ouvert par openEntrySheet()).

import { X } from "lucide-react";
import { useEffect, useId, useRef, useState, type FormEvent, type ReactNode } from "react";
import { create } from "zustand";
import { money, monthOf, parseAmount, today } from "./format";
import { Icon, type LucideIcon } from "./icons";
import { IconButton } from "./layout";
import { useEntries, type Entry } from "./store";
import { toast } from "./toast";

/** Ouvre ou ferme le <dialog> avec `open` ; Échap et un appui sur le fond appellent `onClose`. */
function useDialog(open: boolean, onClose: () => void) {
  const ref = useRef<HTMLDialogElement>(null);
  const pressed = useRef<EventTarget | null>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);
  return {
    ref,
    onClose,
    onPointerDown: (event: { target: EventTarget }) => { pressed.current = event.target; },
    onClick: (event: { target: EventTarget }) => {
      if (event.target === ref.current && pressed.current === ref.current) onClose();
    },
  };
}

type WindowProps = { open: boolean; onClose: () => void; title: ReactNode; children: ReactNode };

/** Un volet : il monte du bas sur un téléphone, au centre ailleurs. */
export function Sheet({ open, onClose, title, children }: WindowProps) {
  const dialog = useDialog(open, onClose);
  const id = useId();
  return (
    <dialog className="sheet" aria-labelledby={id} {...dialog}>
      <div className="sheet__body">
        <div className="sheet__grip" aria-hidden="true" />
        <div className="sheet__head">
          <h2 className="sheet__title" id={id}>{title}</h2>
          <IconButton icon={X} label="Fermer" onClick={onClose} />
        </div>
        {children}
      </div>
    </dialog>
  );
}

/** Une petite fenêtre au centre. */
export function Modal({ open, onClose, title, children }: WindowProps) {
  const dialog = useDialog(open, onClose);
  const id = useId();
  return (
    <dialog className="modal" aria-labelledby={id} {...dialog}>
      <div className="modal__body">
        <div className="modal__head"><h2 className="modal__title" id={id}>{title}</h2></div>
        {children}
      </div>
    </dialog>
  );
}

type ConfirmProps = {
  open: boolean;
  onClose: () => void;
  title: string;
  text: ReactNode;
  confirmLabel: string;
  cancelLabel?: string;
  onConfirm: () => void;
};

/** « Tout effacer ? » : garder, ou faire le geste (en rouge). */
export function ConfirmModal({ open, onClose, title, text, confirmLabel, cancelLabel = "Garder", onConfirm }: ConfirmProps) {
  return (
    <Modal open={open} onClose={onClose} title={title}>
      <p>{text}</p>
      <div className="btn-row">
        <button className="btn" type="button" onClick={onClose}>{cancelLabel}</button>
        <button className="btn btn--danger" type="button" onClick={() => { onClose(); onConfirm(); }}>{confirmLabel}</button>
      </div>
    </Modal>
  );
}

const useEntrySheetState = create<{ open: boolean; entry: Entry | null }>(() => ({ open: false, entry: null }));

/** Ouvre le volet d'ajout ; avec une opération, le volet la modifie (et peut la supprimer). */
export function openEntrySheet(entry: Entry | null = null): void {
  useEntrySheetState.setState({ open: true, entry });
}

export type Category = { name: string; icon: LucideIcon };

type EntrySheetProps = {
  /** 4 à 8 catégories, chacune avec son icône. */
  categories: Category[];
  /** Les deux genres : dépense (montant négatif) et rentrée. */
  kinds?: { out: string; in: string };
  /** Les montants à ajouter d'un appui. */
  quick?: number[];
  title?: string;
  submitLabel?: string;
};

/** Le volet d'ajout d'une opération : genre, montant, montants rapides, catégorie, libellé, date. */
export function EntrySheet({ categories, kinds = { out: "Dépense", in: "Rentrée" }, quick = [5, 10, 20, 50], title = "Nouvelle opération", submitLabel = "Ajouter" }: EntrySheetProps) {
  const { open, entry } = useEntrySheetState();
  const { add, update, remove, month, setMonth } = useEntries();
  const close = () => useEntrySheetState.setState({ open: false });
  const [kind, setKind] = useState<"out" | "in">("out");
  const [amount, setAmount] = useState("");
  const [category, setCategory] = useState(categories[0]?.name ?? "");
  const [label, setLabel] = useState("");
  const [date, setDate] = useState(today());
  const [invalid, setInvalid] = useState(false);
  const amountInput = useRef<HTMLInputElement>(null);
  const ids = useId();

  useEffect(() => {
    if (!open) return;
    const value = entry ? Math.abs(entry.amount) : 0;
    setKind(entry && entry.amount > 0 ? "in" : "out");
    setAmount(entry ? (Number.isInteger(value) ? String(value) : value.toFixed(2).replace(".", ",")) : "");
    setCategory(entry?.category || categories[0]?.name || "");
    setLabel(entry?.label ?? "");
    setDate(entry?.date ?? today());
    setInvalid(false);
    if (!entry) setTimeout(() => amountInput.current?.focus(), 60);
  }, [open, entry, categories]);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const value = parseAmount(amount);
    if (!(value > 0)) {
      setInvalid(true);
      amountInput.current?.focus();
      return;
    }
    const fields = { label: label.trim() || category, amount: kind === "in" ? value : -value, category, date: date || today() };
    close();
    if (entry) {
      update(entry.id, fields);
      toast("Opération modifiée");
      return;
    }
    const added = add(fields);
    // Une opération d'un autre mois : l'appli montre ce mois.
    if (monthOf(added.date) !== month) setMonth(monthOf(added.date));
    toast(`${kind === "in" ? kinds.in : kinds.out} de ${money(value)} enregistrée`, {
      action: { label: "Annuler", run: () => remove(added.id) },
    });
  };

  const removeEntry = () => {
    if (!entry) return;
    close();
    const undo = remove(entry.id);
    if (undo) toast(`« ${entry.label} » supprimé`, { action: { label: "Annuler", run: undo } });
  };

  return (
    <Sheet open={open} onClose={close} title={entry ? "Modifier l'opération" : title}>
      <form className="form" onSubmit={submit} noValidate>
        <fieldset className="seg seg--choice">
          <legend className="visually-hidden">Genre</legend>
          {(["out", "in"] as const).map((value) => (
            <label key={value}>
              <input type="radio" name="kind" value={value} checked={kind === value} onChange={() => setKind(value)} />
              <span>{kinds[value]}</span>
            </label>
          ))}
        </fieldset>
        <label className="visually-hidden" htmlFor={`${ids}-montant`}>Montant en euros</label>
        <div className={`sheet__amount${invalid ? " is-invalid" : ""}`}>
          <input
            ref={amountInput}
            id={`${ids}-montant`}
            inputMode="decimal"
            autoComplete="off"
            placeholder="0"
            value={amount}
            style={{ width: `${Math.max(1, amount.length) + 0.4}ch` }}
            aria-invalid={invalid}
            onChange={(event) => {
              setAmount(event.target.value);
              if (invalid) setInvalid(!(parseAmount(event.target.value) > 0));
            }}
          />
          <span aria-hidden="true">€</span>
        </div>
        <p className="sheet__error">Un montant comme 12,50.</p>
        <div className="sheet__quick">
          {quick.map((step) => (
            <button
              key={step}
              type="button"
              onClick={() => {
                const current = parseAmount(amount);
                const next = (Number.isFinite(current) ? current : 0) + step;
                setAmount(String(Math.round(next * 100) / 100).replace(".", ","));
                setInvalid(false);
              }}
            >
              +{step} €
            </button>
          ))}
        </div>
        <fieldset className="sheet__cats">
          <legend>Catégorie</legend>
          {categories.map((item) => (
            <label className="sheet__cat" key={item.name}>
              <input type="radio" name="category" value={item.name} checked={category === item.name} onChange={() => setCategory(item.name)} />
              <span><Icon icon={item.icon} size={22} /><b>{item.name}</b></span>
            </label>
          ))}
        </fieldset>
        <div className="field-row">
          <div className="field">
            <label htmlFor={`${ids}-libelle`}>Libellé</label>
            <input id={`${ids}-libelle`} autoComplete="off" placeholder={category} value={label} onChange={(event) => setLabel(event.target.value)} />
          </div>
          <div className="field">
            <label htmlFor={`${ids}-date`}>Date</label>
            <input id={`${ids}-date`} type="date" value={date} onChange={(event) => setDate(event.target.value)} />
          </div>
        </div>
        <button className="btn btn--primary btn--block" type="submit">{entry ? "Enregistrer" : submitLabel}</button>
        {entry && <button className="btn btn--danger btn--block" type="button" onClick={removeEntry}>Supprimer l'opération</button>}
      </form>
    </Sheet>
  );
}
