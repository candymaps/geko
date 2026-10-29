import { defineConfig, mergeConfig } from "vite";
import base from "./vite.config";

const modules = JSON.stringify([
  { name: "geocoder", version: "0.1.0", unavailable: null },
  { name: "tiles", version: "0.1.0", unavailable: null },
]);

export default mergeConfig(
  base,
  defineConfig({
    plugins: [
      {
        name: "mock-modules",
        configureServer(server) {
          server.middlewares.use("/admin/api/modules", (_req, res) => {
            res.setHeader("content-type", "application/json");
            res.end(modules);
          });
        },
      },
    ],
    server: { port: 5173 },
  }),
);
