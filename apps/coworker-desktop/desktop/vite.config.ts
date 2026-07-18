import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (id.includes("@tauri-apps")) return "tauri";
          if (id.includes("lucide-react")) return "icons";
          if (
            /[\\/]node_modules[\\/](react-markdown|remark-|rehype-|unified|katex|micromark|mdast-|hast-|unist-|vfile|ccount|comma-separated-tokens|decode-named-character-reference|devlop|escape-string-regexp|html-url-attributes|is-plain-obj|markdown-table|property-information|space-separated-tokens|trim-lines|trough|zwitch)/.test(
              id,
            )
          ) {
            return "markdown";
          }
          if (/[\\/]node_modules[\\/](react|react-dom|scheduler)[\\/]/.test(id)) return "react";
          return undefined;
        },
      },
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    globals: true,
  },
});
