// L'API du serveur (Express) et son temps réel (Socket.IO) : chaque changement est envoyé
// à tous les clients connectés (événement « items »). Les données : src/store.ts (fichier
// JSON, MySQL avec DATABASE_URL ou Redis avec REDIS_URL).

import cors from "cors";
import express from "express";
import { existsSync } from "node:fs";
import { createServer } from "node:http";
import { Server } from "socket.io";
import { JsonStore, type Store } from "./store.js";

export function createApp(store: Store = new JsonStore(process.env.DATA_FILE ?? "data/items.json")) {
  const app = express();
  app.use(cors());
  app.use(express.json());
  // Une page web servie avec l'API (le dossier public/, s'il existe).
  if (existsSync("public")) app.use(express.static("public"));

  const server = createServer(app);
  const io = new Server(server, { cors: { origin: "*" } });
  const changed = async () => io.emit("items", await store.list());

  app.get("/api/health", (_request, response) => {
    response.json({ ok: true });
  });

  app.get("/api/items", async (_request, response) => {
    response.json(await store.list());
  });

  app.post("/api/items", async (request, response) => {
    const title = String(request.body?.title ?? "").trim();
    if (!title) {
      response.status(400).json({ error: "Le titre est obligatoire." });
      return;
    }
    const item = await store.add(title);
    await changed();
    response.status(201).json(item);
  });

  app.patch("/api/items/:id", async (request, response) => {
    const item = await store.update(request.params.id, request.body ?? {});
    if (!item) {
      response.status(404).json({ error: "Élément introuvable." });
      return;
    }
    await changed();
    response.json(item);
  });

  app.delete("/api/items/:id", async (request, response) => {
    if (!(await store.remove(request.params.id))) {
      response.status(404).json({ error: "Élément introuvable." });
      return;
    }
    await changed();
    response.status(204).end();
  });

  io.on("connection", async (socket) => {
    socket.emit("items", await store.list());
  });

  // Une erreur de la base (MySQL ou Redis injoignable) : une réponse claire, pas un plantage.
  app.use((error: unknown, _request: express.Request, response: express.Response, _next: express.NextFunction) => {
    console.error(error);
    response.status(503).json({ error: "Les données sont injoignables pour le moment." });
  });

  return { app, server, io, store };
}
