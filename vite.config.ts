import tailwindcss from "@tailwindcss/vite";
import solid from "vite-plugin-solid";
import { defineConfig } from "vitest/config";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async ({ mode }) => ({
  // `hot: false` under vitest: the HMR runtime's `/@solid-refresh` virtual
  // import cannot be resolved by vitest's module runner and crashes every
  // .tsx test suite. Dev/build are unaffected.
  plugins: [solid({ hot: mode !== "test" }), tailwindcss()],

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

  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    // Excluded from vitest because cargo test covers the Rust side.
    exclude: ["node_modules/**", "dist/**", "src-tauri/**"],
    // Thresholds only apply when coverage is collected (`vitest run
    // --coverage`), so they do not block the default `pnpm test:run`. They turn
    // the soft coverage into a hard gate once `--coverage` is wired into CI.
    coverage: {
      provider: "v8",
      // Files without executable frontend logic must not dilute the metric:
      // JSON locale catalogs, the Rust-shared extension table, the static
      // SVG icon catalog and test plumbing are excluded so the thresholds
      // measure real component/store/util code only.
      exclude: [
        "src/i18n/locales/**",
        "src-tauri/**",
        "src/components/Icons.tsx",
        "src/test/**",
      ],
      thresholds: {
        lines: 70,
        statements: 70,
        functions: 70,
        branches: 60,
      },
    },
  },
}));
