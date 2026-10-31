from fastapi.testclient import TestClient

from app.main import create_app
import os
import time

import pytest

from app.store import RedisStore, SqlStore, Store


def client() -> TestClient:
    return TestClient(create_app(Store(None)))


def test_an_item_is_added_changed_and_deleted():
    api = client()
    created = api.post("/api/items", json={"title": "Acheter du pain"})
    assert created.status_code == 201
    item = created.json()
    assert item["title"] == "Acheter du pain"
    assert api.patch(f"/api/items/{item['id']}", json={"done": True}).json()["done"] is True
    assert api.delete(f"/api/items/{item['id']}").status_code == 204
    assert api.get("/api/items").json() == []


def test_an_item_without_a_title_is_refused():
    assert client().post("/api/items", json={"title": "  "}).status_code == 400


def test_connected_clients_receive_the_list_when_it_changes():
    api = client()
    with api.websocket_connect("/ws") as socket:
        assert socket.receive_json() == {"items": []}
        api.post("/api/items", json={"title": "Appeler Léa"})
        assert socket.receive_json()["items"][-1]["title"] == "Appeler Léa"


def store_story(store: Store):
    first = store.add("Pain")
    store.add("Lait")
    assert [item["title"] for item in store.list()] == ["Pain", "Lait"]
    assert store.update(first["id"], done=True)["done"] is True
    assert store.update("inconnu", done=True) is None
    assert store.remove(first["id"]) is True
    assert store.remove(first["id"]) is False
    assert [item["title"] for item in store.list()] == ["Lait"]


def test_the_sqlite_store_keeps_items_in_order(tmp_path):
    store_story(SqlStore(f"sqlite:///{tmp_path}/app.db"))


@pytest.mark.skipif(not os.environ.get("REDIS_URL"), reason="pas de serveur Redis (REDIS_URL)")
def test_the_redis_store_keeps_items_in_order():
    store_story(RedisStore(os.environ["REDIS_URL"], key=f"test:{time.time()}"))


@pytest.mark.skipif(not os.environ.get("MYSQL_URL"), reason="pas de serveur MySQL (MYSQL_URL)")
def test_the_mysql_store_keeps_items_in_order():
    store = SqlStore(os.environ["MYSQL_URL"])
    for item in store.list():
        store.remove(item["id"])
    store_story(store)
