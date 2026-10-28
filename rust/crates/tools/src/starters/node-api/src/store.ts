// Les données du serveur, gardées dans un fichier JSON (data/items.json) : elles restent après
// un redémarrage. `new Store(null)` les garde en mémoire seulement (pour les tests).

import { randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

export type Item = { id: string; title: string; done: boolean; createdAt: string };

export class Store {
  private items: Item[] = [];

  constructor(private readonly file: string | null) {
    if (!file) return;
    try {
      this.items = JSON.parse(readFileSync(file, "utf8")) as Item[];
    } catch {
      this.items = [];
    }
  }

  list(): Item[] {
    return [...this.items];
  }

  add(title: string): Item {
    const item: Item = { id: randomUUID(), title, done: false, createdAt: new Date().toISOString() };
    this.items.push(item);
    this.save();
    return item;
  }

  update(id: string, fields: Partial<Pick<Item, "title" | "done">>): Item | null {
    const item = this.items.find((known) => known.id === id);
    if (!item) return null;
    if (typeof fields.title === "string" && fields.title.trim()) item.title = fields.title.trim();
    if (typeof fields.done === "boolean") item.done = fields.done;
    this.save();
    return item;
  }

  remove(id: string): boolean {
    const before = this.items.length;
    this.items = this.items.filter((item) => item.id !== id);
    this.save();
    return this.items.length < before;
  }

  private save(): void {
    if (!this.file) return;
    mkdirSync(dirname(this.file), { recursive: true });
    writeFileSync(this.file, JSON.stringify(this.items, null, 2));
  }
}
