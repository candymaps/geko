import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

const geko = process.env.GEKO_URL ?? "http://localhost:8080";

export default defineConfig({
  plugins: [preact()],
  // MapLibre alone is about 1 MB; it loads apart from the shell.
  build: { chunkSizeWarningLimit: 1200 },
  server: {
    proxy: { "/admin": geko, "/geocoder": geko, "/tiles": geko, "/health": geko, "/v1": geko },
  },
});
