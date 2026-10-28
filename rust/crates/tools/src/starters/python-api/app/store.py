"""Les données du serveur, gardées dans un fichier JSON (data/items.json) : elles restent après
un redémarrage. `Store(None)` les garde en mémoire seulement (pour les tests)."""

import json
import uuid
from datetime import datetime, timezone
from pathlib import Path


class Store:
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
        item = {"id": str(uuid.uuid4()), "title": title, "done": False,
                "createdAt": datetime.now(timezone.utc).isoformat()}
        self.items.append(item)
        self._save()
        return item

    def update(self, item_id: str, title: str | None = None, done: bool | None = None) -> dict | None:
        item = next((known for known in self.items if known["id"] == item_id), None)
        if item is None:
            return None
        if title is not None and title.strip():
            item["title"] = title.strip()
        if done is not None:
            item["done"] = done
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
