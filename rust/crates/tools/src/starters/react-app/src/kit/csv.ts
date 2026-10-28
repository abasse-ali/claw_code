// Une copie des opérations pour un tableur (points-virgules et virgule décimale, comme Excel
// en français), et sa reprise sans doubler ce qui est déjà là.

import { parseAmount, today } from "./format";
import { STORE_KEY, useEntries, type Entry } from "./store";
import { toast } from "./toast";

const cell = (value: unknown) => {
  const text = String(value ?? "");
  return /[";\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
};

/** Enregistre la copie sur le téléphone (« appli-2026-10-28.csv »). */
export function exportCsv(): void {
  const entries = useEntries.getState().entries;
  const lines = entries.map((entry) => [entry.date, entry.label, String(entry.amount).replace(".", ","), entry.category, entry.id].map(cell).join(";"));
  const blob = new Blob([`﻿${["date;libellé;montant;catégorie;id", ...lines].join("\r\n")}`], { type: "text/csv;charset=utf-8" });
  const link = document.createElement("a");
  link.href = URL.createObjectURL(blob);
  link.download = `${STORE_KEY}-${today()}.csv`;
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(link.href), 2000);
  toast(`Copie enregistrée : ${link.download}`);
}

function parse(text: string): string[][] {
  const body = text.replace(/^﻿/, "");
  const first = body.split(/\r?\n/, 1)[0];
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
  return rows.filter((line) => line.some((text) => text.trim()));
}

const names: Record<string, keyof Entry> = { date: "date", libelle: "label", label: "label", montant: "amount", amount: "amount", categorie: "category", category: "category", id: "id" };
const plain = (text: string) => text.toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "").trim();

/** Reprend une copie (un fichier .csv choisi par l'utilisateur) ; rend le nombre d'opérations reprises. */
export async function importCsv(file: File): Promise<number> {
  const [head, ...lines] = parse(await file.text());
  const fields = (head || []).map((title) => names[plain(title)]);
  const found = lines
    .map((cells) => Object.fromEntries(fields.map((field, index) => [field, (cells[index] || "").trim()])) as Record<string, string>)
    .filter((line) => line.label && Number.isFinite(parseAmount(line.amount)))
    .map((line): Entry => ({
      id: line.id || `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`,
      label: line.label,
      amount: Math.round(parseAmount(line.amount) * 100) / 100,
      category: line.category || "",
      date: /^\d{4}-\d{2}-\d{2}$/.test(line.date || "") ? line.date : today(),
    }));
  if (!found.length) {
    toast("Ce fichier ne contient aucune opération lisible.", { tone: "error" });
    return 0;
  }
  const before = useEntries.getState().entries;
  const byId = new Map(before.map((entry) => [entry.id, entry]));
  for (const entry of found) byId.set(entry.id, entry);
  useEntries.setState({ entries: [...byId.values()] });
  toast(`${found.length} ${found.length > 1 ? "opérations reprises" : "opération reprise"}`, {
    action: { label: "Annuler", run: () => useEntries.setState({ entries: before }) },
  });
  return found.length;
}

/** Ouvre le choix d'un fichier, puis reprend la copie choisie. */
export function pickCsv(): void {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = ".csv,text/csv";
  input.addEventListener("change", () => {
    const file = input.files?.[0];
    if (file) void importCsv(file);
  });
  input.click();
}
