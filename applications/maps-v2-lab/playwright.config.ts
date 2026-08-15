import { defineConfig } from "@playwright/test";

// Порт лабы — 5178; LAB_PORT переопределяет его, когда параллельные
// worktree-агенты держат по своей лабе и один порт на всех не выходит.
const port = process.env.LAB_PORT ?? "5178";
const origin = `http://localhost:${port}`;

export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  use: {
    baseURL: origin,
  },
  webServer: {
    command: `npm run dev -- --port ${port} --strictPort`,
    url: origin,
    reuseExistingServer: true,
    timeout: 120_000,
  },
});
