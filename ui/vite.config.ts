import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

const geko = process.env.GEKO_URL ?? "http://localhost:8080";

export default defineConfig({
  plugins: [preact()],
  server: {
    proxy: { "/admin": geko, "/geocoder": geko, "/tiles": geko, "/health": geko },
  },
});
