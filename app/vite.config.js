import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    fs: { allow: [".."] },
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    license: { fileName: "licences/frontend.txt" },
  },
});
