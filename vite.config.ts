import { resolve } from "node:path";
import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "es2022",
    outDir: "dist",
    rollupOptions: {
      input: {
        popup: resolve(__dirname, "index.html"),
        settings: resolve(__dirname, "settings.html"),
        wizard: resolve(__dirname, "wizard.html"),
      },
    },
  },
});
