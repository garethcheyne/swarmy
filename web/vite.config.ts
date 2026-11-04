import { resolve } from "node:path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

// The Rust daemon serves the built app from web/dist. During development,
// run the daemon on :8080 and `pnpm dev`; API and WebSocket calls are proxied.
const daemon = process.env.RIPTIDE_URL ?? "http://127.0.0.1:8080"

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": resolve(import.meta.dirname, "./src"),
    },
  },
  server: {
    proxy: {
      "/api": { target: daemon, ws: true, changeOrigin: true },
    },
  },
})
