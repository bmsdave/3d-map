import { defineConfig } from "@playwright/test";

// Порт лабы — 5178; LAB_PORT переопределяет его, когда параллельные
// worktree-агенты держат по своей лабе и один порт на всех не выходит.
const port = process.env.LAB_PORT ?? "5178";
const origin = `http://localhost:${port}`;

export default defineConfig({
  testDir: "e2e",
  reporter: [["list"]],
  // Замеры производительности живут в своём конфиге: здесь три воркера,
  // а там время меряется в тишине.
  testIgnore: "perf/**",
  timeout: 60_000,
  // Студии больше не рисуют двадцать килобайт синтетики: они тянут
  // настоящие тайлы вырезки, и первый кадр стоит мегабайтов. Пять
  // секунд по умолчанию — это про мгновенную фикстуру, а не про это.
  expect: { timeout: 20_000 },
  // И по той же причине — потолок на параллельность: каждый воркер
  // держит свой Chromium с живым контекстом WebGL и декодирует свои
  // мегабайты.
  workers: 3,
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
