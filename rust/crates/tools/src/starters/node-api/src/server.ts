import { createApp } from "./app.js";
import { openStore, type Store } from "./store.js";

const port = Number(process.env.PORT) || 3000;
let store: Store;
try {
  store = await openStore();
} catch (error) {
  console.error("Les données sont injoignables (DATABASE_URL ou REDIS_URL) :", error instanceof Error ? error.message : error);
  process.exit(1);
}
const { server } = createApp(store);
server.listen(port, () => {
  console.log(`Serveur prêt : http://localhost:${port}`);
});
