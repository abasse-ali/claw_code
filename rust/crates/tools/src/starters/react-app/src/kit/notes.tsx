// Les notes : un éditeur de texte mis en forme (titres, gras, listes à puces, listes à cocher,
// citations, annuler), les notes gardées sur le téléphone (useNotes), et leur liste avec
// recherche et épingles. L'éditeur (Tiptap) n'est téléchargé qu'à sa première ouverture.

import { AnimatePresence, motion } from "framer-motion";
import { Pin, PinOff, Plus, Search, StickyNote, Trash2 } from "lucide-react";
import { lazy, Suspense, useMemo, useState } from "react";
import { create } from "zustand";
import { persist } from "zustand/middleware";
import { Icon } from "./icons";
import { toast } from "./toast";
import { Highlight } from "./views";

export type NoteEditorProps = {
  /** Le texte de la note en HTML ("" pour une note vide), tel que onChange le rend. */
  value: string;
  onChange: (html: string) => void;
  placeholder?: string;
  label?: string;
  autofocus?: boolean;
};

const NoteEditorImpl = lazy(() => import("./lazy/note-editor"));

/** L'éditeur d'une note : sa barre (Titre, Gras, Italique, Barré, listes, liste à cocher,
 * citation, Annuler, Rétablir) et la page. `value` et `onChange` : le texte en HTML. Les
 * raccourcis marchent aussi : « - » ou « 1. » en début de ligne, « [ ] » pour une case. */
export function NoteEditor(props: NoteEditorProps) {
  return (
    <Suspense fallback={<div className="note-editor note-editor--loading">Chargement de l'éditeur…</div>}>
      <NoteEditorImpl {...props} />
    </Suspense>
  );
}

/** Le texte d'une note sans sa mise en forme (pour un aperçu, une recherche, un partage). */
export function noteText(html: string): string {
  if (!html) return "";
  const page = new DOMParser().parseFromString(html.replace(/<\/(p|h\d|li|blockquote)>/g, "</$1>\n"), "text/html");
  return (page.body.textContent ?? "").replace(/\n{2,}/g, "\n").trim();
}

/** Le titre d'une note : sa première ligne (« Sans titre » si elle est vide). */
export const noteTitle = (html: string) => noteText(html).split("\n")[0]?.trim().slice(0, 80) || "Sans titre";

export type Note = { id: string; html: string; pinned: boolean; createdAt: string; updatedAt: string };

type NotesState = {
  notes: Note[];
  /** Une nouvelle note (vide par défaut) ; rend son id. */
  add: (html?: string) => string;
  update: (id: string, html: string) => void;
  togglePin: (id: string) => void;
  /** Supprime ; rend de quoi annuler. */
  remove: (id: string) => () => void;
};

/** Les notes, gardées sur le téléphone : notes, add(), update(id, html), togglePin(id),
 * remove(id) (qui rend l'annulation). */
export const useNotes = create<NotesState>()(persist((set, get) => ({
  notes: [],
  add: (html = "") => {
    const now = new Date().toISOString();
    const id = `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
    set({ notes: [{ id, html, pinned: false, createdAt: now, updatedAt: now }, ...get().notes] });
    return id;
  },
  update: (id, html) => set({ notes: get().notes.map((note) => (note.id === id ? { ...note, html, updatedAt: new Date().toISOString() } : note)) }),
  togglePin: (id) => set({ notes: get().notes.map((note) => (note.id === id ? { ...note, pinned: !note.pinned } : note)) }),
  remove: (id) => {
    const before = get().notes;
    set({ notes: before.filter((note) => note.id !== id) });
    return () => set({ notes: before });
  },
}), { name: "appli-notes" }));

const when = (iso: string) => {
  const date = new Date(iso);
  const today = new Date();
  return date.toDateString() === today.toDateString()
    ? new Intl.DateTimeFormat("fr-FR", { hour: "2-digit", minute: "2-digit" }).format(date)
    : new Intl.DateTimeFormat("fr-FR", { day: "numeric", month: "short" }).format(date);
};

/** La liste des notes : recherche (sans accents ni majuscules), épinglées en haut, puis les plus
 * récentes ; « Nouvelle note » ; un toucher ouvre une note (`onOpen(id)`), la corbeille la
 * supprime avec « Annuler ». */
export function NotesList({ onOpen, emptyText = "Aucune note pour l'instant." }: { onOpen: (id: string) => void; emptyText?: string }) {
  const notes = useNotes((state) => state.notes);
  const [query, setQuery] = useState("");
  const plain = (text: string) => text.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
  const shown = useMemo(() => {
    const words = plain(query).split(/\s+/).filter(Boolean);
    return [...notes]
      .filter((note) => words.every((word) => plain(noteText(note.html)).includes(word)))
      .sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.updatedAt.localeCompare(a.updatedAt));
  }, [notes, query]);
  const create = () => onOpen(useNotes.getState().add());
  const remove = (note: Note) => {
    const undo = useNotes.getState().remove(note.id);
    toast(`« ${noteTitle(note.html)} » supprimée`, { action: { label: "Annuler", run: undo } });
  };
  return (
    <div className="notes">
      <div className="notes__top">
        <label className="notes__search">
          <Icon icon={Search} size={18} />
          <span className="visually-hidden">Chercher dans les notes</span>
          <input type="search" placeholder="Chercher" value={query} onChange={(event) => setQuery(event.target.value)} />
        </label>
        <button className="btn btn--primary" type="button" onClick={create}><Icon icon={Plus} size={18} />Nouvelle note</button>
      </div>
      {!shown.length ? (
        <div className="empty"><Icon icon={StickyNote} size={36} /><p>{query ? "Aucune note ne contient ces mots." : emptyText}</p></div>
      ) : (
        <ul className="notes__list">
          <AnimatePresence initial={false}>
            {shown.map((note) => {
              const text = noteText(note.html);
              const [title, ...rest] = text.split("\n");
              return (
                <motion.li key={note.id} layout initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, height: 0 }} className={note.pinned ? "is-pinned" : ""}>
                  <button type="button" className="notes__open" onClick={() => onOpen(note.id)}>
                    <b><Highlight text={title?.trim() || "Sans titre"} query={query} /></b>
                    <span><small>{when(note.updatedAt)}</small> <Highlight text={rest.join(" ").slice(0, 140) || "Pas d'autre texte"} query={query} /></span>
                  </button>
                  <button className="icon-btn" type="button" aria-label={note.pinned ? "Désépingler" : "Épingler"} aria-pressed={note.pinned} onClick={() => useNotes.getState().togglePin(note.id)}>
                    <Icon icon={note.pinned ? PinOff : Pin} size={18} />
                  </button>
                  <button className="icon-btn" type="button" aria-label="Supprimer la note" onClick={() => remove(note)}><Icon icon={Trash2} size={18} /></button>
                </motion.li>
              );
            })}
          </AnimatePresence>
        </ul>
      )}
    </div>
  );
}
