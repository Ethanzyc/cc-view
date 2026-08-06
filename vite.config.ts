import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import path from "path";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// 截图 mock：CC_VIEW_MOCK=1 时把 @tauri-apps/* 指向 src/mock/*（浏览器渲染 + mock 数据，供 gstack 截图）
const mockAlias = process.env.CC_VIEW_MOCK
  ? {
      "@tauri-apps/api/core": path.resolve(__dirname, "src/mock/api.ts"),
      "@tauri-apps/api/event": path.resolve(__dirname, "src/mock/api.ts"),
      "@tauri-apps/api/webviewWindow": path.resolve(__dirname, "src/mock/api.ts"),
      "@tauri-apps/api/app": path.resolve(__dirname, "src/mock/api.ts"),
      "@tauri-apps/plugin-updater": path.resolve(__dirname, "src/mock/plugins.ts"),
      "@tauri-apps/plugin-process": path.resolve(__dirname, "src/mock/plugins.ts"),
    }
  : {};

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [vue()],
  resolve: { alias: mockAlias },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
