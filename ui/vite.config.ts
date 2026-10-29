import { defineConfig } from "vite";
import preact from "@preact/preset-vite";
import { readFileSync } from "node:fs";

const { version } = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));

const geko = process.env.GEKO_URL ?? "http://localhost:8080";

export default defineConfig({
  plugins: [preact()],
  define: { __APP_VERSION__: JSON.stringify(version) },
  // MapLibre alone is about 1 MB; it loads apart from the shell.
  build: { chunkSizeWarningLimit: 1200 },
  server: {
    proxy: { "/admin": geko, "/geocoder": geko, "/tiles": geko, "/health": geko, "/v1": geko },
  },
});
