import { createApp } from "./app.js";

const port = Number(process.env.PORT) || 3000;
const { server } = createApp();
server.listen(port, () => {
  console.log(`Serveur prêt : http://localhost:${port}`);
});
