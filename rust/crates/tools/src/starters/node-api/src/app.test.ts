import assert from "node:assert/strict";
import type { AddressInfo } from "node:net";
import { after, before, test } from "node:test";
import { io as connect, type Socket } from "socket.io-client";
import { createApp } from "./app.js";
import { Store, type Item } from "./store.js";

const { server, io } = createApp(new Store(null));
let base = "";

before(async () => {
  await new Promise<void>((done) => server.listen(0, done));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

after(() => {
  io.close();
  server.close();
});

const call = (path: string, method = "GET", body?: unknown) =>
  fetch(`${base}${path}`, {
    method,
    headers: body === undefined ? {} : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });

test("an item is added, changed and deleted", async () => {
  const created = await call("/api/items", "POST", { title: "Acheter du pain" });
  assert.equal(created.status, 201);
  const item = (await created.json()) as Item;
  assert.equal(item.title, "Acheter du pain");

  const changed = await call(`/api/items/${item.id}`, "PATCH", { done: true });
  assert.equal(((await changed.json()) as Item).done, true);

  assert.equal((await call(`/api/items/${item.id}`, "DELETE")).status, 204);
  assert.deepEqual(await (await call("/api/items")).json(), []);
});

test("an item without a title is refused", async () => {
  const response = await call("/api/items", "POST", { title: "  " });
  assert.equal(response.status, 400);
});

test("connected clients receive the list when it changes", async () => {
  const client: Socket = connect(base, { transports: ["websocket"] });
  const lists: Item[][] = [];
  const second = new Promise<void>((done) => {
    client.on("items", (items: Item[]) => {
      lists.push(items);
      if (lists.length === 2) done();
    });
  });
  await new Promise<void>((done) => client.on("connect", () => done()));
  await call("/api/items", "POST", { title: "Appeler Léa" });
  await second;
  client.close();
  assert.equal(lists[1].at(-1)?.title, "Appeler Léa");
});
