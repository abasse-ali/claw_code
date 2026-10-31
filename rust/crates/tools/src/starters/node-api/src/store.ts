// Les données du serveur. Trois façons de les garder, choisies au démarrage par openStore() :
// - MySQL quand DATABASE_URL est donnée (mysql://utilisateur:mot-de-passe@hôte:3306/base) : la
//   table `items` est créée au besoin ;
// - Redis quand REDIS_URL est donnée (redis://localhost:6379) : une liste rapide, partagée par
//   plusieurs serveurs ;
// - sinon un fichier JSON (data/items.json), qui reste après un redémarrage.
// `new JsonStore(null)` garde tout en mémoire seulement (pour les tests). Toutes les méthodes
// rendent une promesse : la base peut être loin.

import { randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

export type Item = { id: string; title: string; done: boolean; createdAt: string };
export type ItemChange = Partial<Pick<Item, "title" | "done">>;

export interface Store {
  list(): Promise<Item[]>;
  add(title: string): Promise<Item>;
  update(id: string, fields: ItemChange): Promise<Item | null>;
  remove(id: string): Promise<boolean>;
  close(): Promise<void>;
}

const newItem = (title: string): Item => ({ id: randomUUID(), title, done: false, createdAt: new Date().toISOString() });

/** Applique un changement (un titre vide ne remplace rien) ; rend l'élément changé. */
function apply(item: Item, fields: ItemChange): Item {
  if (typeof fields.title === "string" && fields.title.trim()) item.title = fields.title.trim();
  if (typeof fields.done === "boolean") item.done = fields.done;
  return item;
}

/** Un fichier JSON ; `null` : en mémoire seulement. */
export class JsonStore implements Store {
  private items: Item[] = [];

  constructor(private readonly file: string | null) {
    if (!file) return;
    try {
      this.items = JSON.parse(readFileSync(file, "utf8")) as Item[];
    } catch {
      this.items = [];
    }
  }

  async list() {
    return [...this.items];
  }

  async add(title: string) {
    const item = newItem(title);
    this.items.push(item);
    this.save();
    return item;
  }

  async update(id: string, fields: ItemChange) {
    const item = this.items.find((known) => known.id === id);
    if (!item) return null;
    apply(item, fields);
    this.save();
    return item;
  }

  async remove(id: string) {
    const before = this.items.length;
    this.items = this.items.filter((item) => item.id !== id);
    this.save();
    return this.items.length < before;
  }

  async close() {}

  private save(): void {
    if (!this.file) return;
    mkdirSync(dirname(this.file), { recursive: true });
    writeFileSync(this.file, JSON.stringify(this.items, null, 2));
  }
}

/** Redis : chaque élément est un texte JSON dans le hachage `items`, l'ordre dans la liste
 * `items:order`. */
export class RedisStore implements Store {
  private constructor(private readonly redis: import("ioredis").Redis, private readonly key: string) {}

  static async open(url: string, key = "items"): Promise<RedisStore> {
    const { Redis } = await import("ioredis");
    const redis = new Redis(url, { lazyConnect: true, maxRetriesPerRequest: 2 });
    redis.on("error", (error: Error) => console.error("Redis :", error.message));
    await redis.connect();
    return new RedisStore(redis, key);
  }

  async list() {
    const ids = await this.redis.lrange(`${this.key}:order`, 0, -1);
    if (!ids.length) return [];
    const texts = await this.redis.hmget(this.key, ...ids);
    return texts.filter((text): text is string => Boolean(text)).map((text) => JSON.parse(text) as Item);
  }

  async add(title: string) {
    const item = newItem(title);
    await this.redis.multi().hset(this.key, item.id, JSON.stringify(item)).rpush(`${this.key}:order`, item.id).exec();
    return item;
  }

  async update(id: string, fields: ItemChange) {
    const text = await this.redis.hget(this.key, id);
    if (!text) return null;
    const item = apply(JSON.parse(text) as Item, fields);
    await this.redis.hset(this.key, id, JSON.stringify(item));
    return item;
  }

  async remove(id: string) {
    const [[, removed] = [null, 0]] = (await this.redis.multi().hdel(this.key, id).lrem(`${this.key}:order`, 0, id).exec()) ?? [];
    return Number(removed) > 0;
  }

  async close() {
    await this.redis.quit();
  }
}

type Row = { id: string; title: string; done: number | boolean; created_at: Date | string };

/** MySQL (ou MariaDB) : la table `items`, créée au premier démarrage. */
export class MySqlStore implements Store {
  private constructor(private readonly pool: import("mysql2/promise").Pool) {}

  static async open(url: string): Promise<MySqlStore> {
    const mysql = await import("mysql2/promise");
    const pool = mysql.createPool({ uri: url, connectionLimit: 5 });
    await pool.query(`CREATE TABLE IF NOT EXISTS items (
      seq BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
      id CHAR(36) NOT NULL UNIQUE,
      title VARCHAR(500) NOT NULL,
      done BOOLEAN NOT NULL DEFAULT FALSE,
      created_at DATETIME(3) NOT NULL
    ) CHARACTER SET utf8mb4`);
    return new MySqlStore(pool);
  }

  private static item(row: Row): Item {
    return { id: row.id, title: row.title, done: Boolean(row.done), createdAt: new Date(row.created_at).toISOString() };
  }

  async list() {
    const [rows] = await this.pool.query("SELECT id, title, done, created_at FROM items ORDER BY seq");
    return (rows as Row[]).map(MySqlStore.item);
  }

  async add(title: string) {
    const item = newItem(title);
    await this.pool.execute("INSERT INTO items (id, title, done, created_at) VALUES (?, ?, ?, ?)", [item.id, item.title, item.done, new Date(item.createdAt)]);
    return item;
  }

  async update(id: string, fields: ItemChange) {
    const [rows] = await this.pool.execute("SELECT id, title, done, created_at FROM items WHERE id = ?", [id]);
    const row = (rows as Row[])[0];
    if (!row) return null;
    const item = apply(MySqlStore.item(row), fields);
    await this.pool.execute("UPDATE items SET title = ?, done = ? WHERE id = ?", [item.title, item.done, id]);
    return item;
  }

  async remove(id: string) {
    const [result] = await this.pool.execute("DELETE FROM items WHERE id = ?", [id]);
    return (result as { affectedRows: number }).affectedRows > 0;
  }

  async close() {
    await this.pool.end();
  }
}

/** La façon de garder les données que l'environnement demande (voir en haut). */
export async function openStore(env = process.env): Promise<Store> {
  if (env.DATABASE_URL) return MySqlStore.open(env.DATABASE_URL);
  if (env.REDIS_URL) return RedisStore.open(env.REDIS_URL);
  return new JsonStore(env.DATA_FILE ?? "data/items.json");
}
