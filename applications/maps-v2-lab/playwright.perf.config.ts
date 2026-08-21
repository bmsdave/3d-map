import { defineConfig } from "@playwright/test";

// Замеры производительности отдельно от функционального прогона, и не
// из вкусовщины: они меряют время, а время меряется только в тишине.
// Три воркера, которыми идёт test:e2e, — это три хромиума с живыми
// контекстами WebGL, дерущиеся за то самое CPU, чей бюджет здесь
// проверяется.

const port = process.env.LAB_PORT ?? "5178";
const origin = `http://localhost:${port}`;

export default defineConfig({
  testDir: "e2e/perf",
  timeout: 120_000,
  expect: { timeout: 20_000 },
  // Один. Ровно по той причине, ради которой всё это написано.
  workers: 1,
  fullyParallel: false,
  // Повтор упавшего замера показал бы «иногда укладывается», что не
  // ответ на вопрос «держим ли мы бюджет».
  retries: 0,
  reporter: [["./e2e/perf/reporter.ts"]],
  use: {
    baseURL: origin,
    // Одинаковая сцена у всех студий: размер холста входит в стоимость
    // кадра, и мерить его на разных окнах — мерить окно.
    viewport: { width: 1280, height: 900 },
  },
  webServer: {
    command: `npm run dev -- --port ${port} --strictPort`,
    url: origin,
    reuseExistingServer: true,
    timeout: 120_000,
  },
});
