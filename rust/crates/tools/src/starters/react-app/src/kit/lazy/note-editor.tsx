// L'éditeur lui-même (Tiptap). Chargé seulement quand un NoteEditor s'affiche : une appli
// sans notes ne le télécharge pas. Passez par NoteEditor.

import { TaskItem } from "@tiptap/extension-task-item";
import { TaskList } from "@tiptap/extension-task-list";
import { Placeholder } from "@tiptap/extensions";
import { EditorContent, useEditor, useEditorState } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { Bold, Heading2, Italic, List, ListChecks, ListOrdered, Quote, Redo2, Strikethrough, Undo2 } from "lucide-react";
import { useEffect } from "react";
import { Icon, type LucideIcon } from "../icons";
import type { NoteEditorProps } from "../notes";

export default function NoteEditorImpl({ value, onChange, placeholder = "Écrivez ici…", label = "Note", autofocus }: NoteEditorProps) {
  const editor = useEditor({
    extensions: [
      StarterKit.configure({ heading: { levels: [2, 3] }, link: { openOnClick: true, autolink: true } }),
      TaskList,
      TaskItem.configure({ nested: true, a11y: { checkboxLabel: (node, checked) => `${checked ? "Fait" : "À faire"} : ${node.textContent || "tâche vide"}` } }),
      Placeholder.configure({ placeholder }),
    ],
    content: value || "",
    autofocus: autofocus ? "end" : false,
    editorProps: { attributes: { class: "note-editor__page", "aria-label": label, role: "textbox", "aria-multiline": "true" } },
    onUpdate: ({ editor: current }) => onChange(current.isEmpty ? "" : current.getHTML()),
  });

  // Une autre note ouverte dans le même éditeur : son texte remplace l'ancien.
  useEffect(() => {
    if (editor && value !== (editor.isEmpty ? "" : editor.getHTML())) editor.commands.setContent(value || "", { emitUpdate: false });
  }, [editor, value]);

  const state = useEditorState({
    editor,
    selector: ({ editor: current }) => current ? {
      bold: current.isActive("bold"), italic: current.isActive("italic"), strike: current.isActive("strike"),
      heading: current.isActive("heading", { level: 2 }), bullet: current.isActive("bulletList"), ordered: current.isActive("orderedList"),
      task: current.isActive("taskList"), quote: current.isActive("blockquote"), undo: current.can().undo(), redo: current.can().redo(),
    } : null,
  });

  if (!editor) return <div className="note-editor note-editor--loading">Chargement de l'éditeur…</div>;
  const tool = (icon: LucideIcon, name: string, on: boolean | undefined, run: () => void, disabled = false) => (
    <button key={name} type="button" className={`note-editor__tool${on ? " is-on" : ""}`} aria-label={name} aria-pressed={on} disabled={disabled}
      onMouseDown={(event) => event.preventDefault()} onClick={run}>
      <Icon icon={icon} size={18} />
    </button>
  );
  const chain = () => editor.chain().focus();
  return (
    <div className="note-editor">
      <div className="note-editor__bar" role="toolbar" aria-label="Mise en forme">
        {tool(Heading2, "Titre", state?.heading, () => chain().toggleHeading({ level: 2 }).run())}
        {tool(Bold, "Gras", state?.bold, () => chain().toggleBold().run())}
        {tool(Italic, "Italique", state?.italic, () => chain().toggleItalic().run())}
        {tool(Strikethrough, "Barré", state?.strike, () => chain().toggleStrike().run())}
        <span className="note-editor__sep" aria-hidden="true" />
        {tool(List, "Liste à puces", state?.bullet, () => chain().toggleBulletList().run())}
        {tool(ListOrdered, "Liste numérotée", state?.ordered, () => chain().toggleOrderedList().run())}
        {tool(ListChecks, "Liste à cocher", state?.task, () => chain().toggleTaskList().run())}
        {tool(Quote, "Citation", state?.quote, () => chain().toggleBlockquote().run())}
        <span className="note-editor__sep" aria-hidden="true" />
        {tool(Undo2, "Annuler", false, () => chain().undo().run(), !state?.undo)}
        {tool(Redo2, "Rétablir", false, () => chain().redo().run(), !state?.redo)}
      </div>
      <EditorContent editor={editor} />
    </div>
  );
}
