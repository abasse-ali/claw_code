from fastapi.testclient import TestClient

from app.main import create_app
from app.store import Store


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
