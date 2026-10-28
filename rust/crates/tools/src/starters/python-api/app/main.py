"""L'API du serveur (FastAPI) et son temps réel (WebSocket /ws) : chaque changement est envoyé
à tous les clients connectés. Lancement : uvicorn app.main:app --reload"""

import os

from fastapi import FastAPI, HTTPException, Response, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel, Field

from .store import Store


class NewItem(BaseModel):
    title: str = Field(min_length=1)


class ItemChange(BaseModel):
    title: str | None = None
    done: bool | None = None


def create_app(store: Store | None = None) -> FastAPI:
    store = store if store is not None else Store(os.environ.get("DATA_FILE", "data/items.json"))
    app = FastAPI(title="Serveur")
    app.add_middleware(CORSMiddleware, allow_origins=["*"], allow_methods=["*"], allow_headers=["*"])
    clients: set[WebSocket] = set()

    async def changed() -> None:
        for client in list(clients):
            try:
                await client.send_json({"items": store.list()})
            except (RuntimeError, WebSocketDisconnect):
                clients.discard(client)

    @app.get("/api/health")
    def health() -> dict:
        return {"ok": True}

    @app.get("/api/items")
    def items() -> list[dict]:
        return store.list()

    @app.post("/api/items", status_code=201)
    async def add(item: NewItem) -> dict:
        title = item.title.strip()
        if not title:
            raise HTTPException(400, "Le titre est obligatoire.")
        added = store.add(title)
        await changed()
        return added

    @app.patch("/api/items/{item_id}")
    async def update(item_id: str, change: ItemChange) -> dict:
        item = store.update(item_id, change.title, change.done)
        if item is None:
            raise HTTPException(404, "Élément introuvable.")
        await changed()
        return item

    @app.delete("/api/items/{item_id}", status_code=204)
    async def remove(item_id: str) -> Response:
        if not store.remove(item_id):
            raise HTTPException(404, "Élément introuvable.")
        await changed()
        return Response(status_code=204)

    @app.websocket("/ws")
    async def live(socket: WebSocket) -> None:
        await socket.accept()
        clients.add(socket)
        await socket.send_json({"items": store.list()})
        try:
            while True:
                await socket.receive_text()
        except WebSocketDisconnect:
            clients.discard(socket)

    return app


app = create_app()
