// L'API du serveur (Express) et son temps réel (Socket.IO) : chaque changement est envoyé
// à tous les clients connectés (événement « items »).

import cors from "cors";
import express from "express";
import { existsSync } from "node:fs";
import { createServer } from "node:http";
import { Server } from "socket.io";
import { Store } from "./store.js";

export function createApp(store = new Store(process.env.DATA_FILE ?? "data/items.json")) {
  const app = express();
  app.use(cors());
  app.use(express.json());
  // Une page web servie avec l'API (le dossier public/, s'il existe).
  if (existsSync("public")) app.use(express.static("public"));

  const server = createServer(app);
  const io = new Server(server, { cors: { origin: "*" } });
  const changed = () => io.emit("items", store.list());

  app.get("/api/health", (_request, response) => {
    response.json({ ok: true });
  });

  app.get("/api/items", (_request, response) => {
    response.json(store.list());
  });

  app.post("/api/items", (request, response) => {
    const title = String(request.body?.title ?? "").trim();
    if (!title) {
      response.status(400).json({ error: "Le titre est obligatoire." });
      return;
    }
    const item = store.add(title);
    changed();
    response.status(201).json(item);
  });

  app.patch("/api/items/:id", (request, response) => {
    const item = store.update(request.params.id, request.body ?? {});
    if (!item) {
      response.status(404).json({ error: "Élément introuvable." });
      return;
    }
    changed();
    response.json(item);
  });

  app.delete("/api/items/:id", (request, response) => {
    if (!store.remove(request.params.id)) {
      response.status(404).json({ error: "Élément introuvable." });
      return;
    }
    changed();
    response.status(204).end();
  });

  io.on("connection", (socket) => {
    socket.emit("items", store.list());
  });

  return { app, server, io, store };
}
