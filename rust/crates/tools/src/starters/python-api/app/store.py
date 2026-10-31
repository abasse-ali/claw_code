"""Les données du serveur. Trois façons de les garder, choisies au démarrage par open_store() :
- une base SQL quand DATABASE_URL est donnée, avec SQLAlchemy : MySQL
  (mysql+pymysql://utilisateur:mot-de-passe@hôte:3306/base) ou SQLite (sqlite:///data/app.db) ;
  la table `items` est créée au besoin ;
- Redis quand REDIS_URL est donnée (redis://localhost:6379) : rapide, partagé par plusieurs
  serveurs ;
- sinon un fichier JSON (data/items.json), qui reste après un redémarrage.
`Store(None)` garde tout en mémoire seulement (pour les tests)."""

import json
import os
import uuid
from datetime import datetime, timezone
from pathlib import Path


def _new_item(title: str) -> dict:
    return {"id": str(uuid.uuid4()), "title": title, "done": False,
            "createdAt": datetime.now(timezone.utc).isoformat()}


def _apply(item: dict, title: str | None, done: bool | None) -> dict:
    if title is not None and title.strip():
        item["title"] = title.strip()
    if done is not None:
        item["done"] = done
    return item


class Store:
    """Un fichier JSON ; `None` : en mémoire seulement."""

    def __init__(self, file: str | None):
        self.file = Path(file) if file else None
        self.items: list[dict] = []
        if self.file and self.file.is_file():
            try:
                self.items = json.loads(self.file.read_text(encoding="utf-8"))
            except ValueError:
                self.items = []

    def list(self) -> list[dict]:
        return list(self.items)

    def add(self, title: str) -> dict:
        item = _new_item(title)
        self.items.append(item)
        self._save()
        return item

    def update(self, item_id: str, title: str | None = None, done: bool | None = None) -> dict | None:
        item = next((known for known in self.items if known["id"] == item_id), None)
        if item is None:
            return None
        _apply(item, title, done)
        self._save()
        return item

    def remove(self, item_id: str) -> bool:
        before = len(self.items)
        self.items = [item for item in self.items if item["id"] != item_id]
        self._save()
        return len(self.items) < before

    def _save(self) -> None:
        if self.file:
            self.file.parent.mkdir(parents=True, exist_ok=True)
            self.file.write_text(json.dumps(self.items, ensure_ascii=False, indent=2), encoding="utf-8")


class SqlStore(Store):
    """Une base SQL (MySQL, SQLite…) par SQLAlchemy : la table `items`."""

    def __init__(self, url: str):
        from sqlalchemy import (Boolean, Column, DateTime, Integer, MetaData, String, Table,
                                create_engine)

        if url.startswith("sqlite:///"):
            Path(url.removeprefix("sqlite:///")).parent.mkdir(parents=True, exist_ok=True)
        self.engine = create_engine(url, pool_pre_ping=True)
        meta = MetaData()
        self.table = Table(
            "items", meta,
            Column("seq", Integer, primary_key=True, autoincrement=True),
            Column("id", String(36), unique=True, nullable=False),
            Column("title", String(500), nullable=False),
            Column("done", Boolean, nullable=False, default=False),
            Column("created_at", DateTime(timezone=True), nullable=False),
            mysql_charset="utf8mb4",
        )
        meta.create_all(self.engine)

    @staticmethod
    def _item(row) -> dict:
        created = row.created_at if row.created_at.tzinfo else row.created_at.replace(tzinfo=timezone.utc)
        return {"id": row.id, "title": row.title, "done": bool(row.done), "createdAt": created.isoformat()}

    def list(self) -> list[dict]:
        with self.engine.connect() as db:
            return [self._item(row) for row in db.execute(self.table.select().order_by(self.table.c.seq))]

    def add(self, title: str) -> dict:
        item = _new_item(title)
        with self.engine.begin() as db:
            db.execute(self.table.insert().values(id=item["id"], title=title, done=False,
                                                  created_at=datetime.fromisoformat(item["createdAt"])))
        return item

    def update(self, item_id: str, title: str | None = None, done: bool | None = None) -> dict | None:
        with self.engine.begin() as db:
            row = db.execute(self.table.select().where(self.table.c.id == item_id)).first()
            if row is None:
                return None
            item = _apply(self._item(row), title, done)
            db.execute(self.table.update().where(self.table.c.id == item_id)
                       .values(title=item["title"], done=item["done"]))
        return item

    def remove(self, item_id: str) -> bool:
        with self.engine.begin() as db:
            return db.execute(self.table.delete().where(self.table.c.id == item_id)).rowcount > 0


class RedisStore(Store):
    """Redis : chaque élément en JSON dans le hachage `items`, l'ordre dans la liste `items:order`."""

    def __init__(self, url: str, key: str = "items"):
        import redis

        self.redis = redis.Redis.from_url(url, decode_responses=True)
        self.redis.ping()
        self.key = key

    def list(self) -> list[dict]:
        ids = self.redis.lrange(f"{self.key}:order", 0, -1)
        texts = self.redis.hmget(self.key, ids) if ids else []
        return [json.loads(text) for text in texts if text]

    def add(self, title: str) -> dict:
        item = _new_item(title)
        self.redis.pipeline().hset(self.key, item["id"], json.dumps(item)).rpush(f"{self.key}:order", item["id"]).execute()
        return item

    def update(self, item_id: str, title: str | None = None, done: bool | None = None) -> dict | None:
        text = self.redis.hget(self.key, item_id)
        if text is None:
            return None
        item = _apply(json.loads(text), title, done)
        self.redis.hset(self.key, item_id, json.dumps(item))
        return item

    def remove(self, item_id: str) -> bool:
        removed, _ = self.redis.pipeline().hdel(self.key, item_id).lrem(f"{self.key}:order", 0, item_id).execute()
        return removed > 0


def open_store(env=os.environ) -> Store:
    """La façon de garder les données que l'environnement demande (voir en haut)."""
    if env.get("DATABASE_URL"):
        return SqlStore(env["DATABASE_URL"])
    if env.get("REDIS_URL"):
        return RedisStore(env["REDIS_URL"])
    return Store(env.get("DATA_FILE", "data/items.json"))
