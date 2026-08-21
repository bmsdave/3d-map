// Две страницы, которых нет в реестре студий, потому что они не студии:
// живая доска и анимированная галерея. На доске одновременно живут шесть
// карт, в галерее — четыре, и обе они — то, что человек видит первым.

import { expect, test, type Page } from "@playwright/test";

import { attachMeasurement, BLOCK_BUDGET_MS, traceMeasurement } from "./harness";

const QUIET_MS = 400;

async function armed(page: Page): Promise<void> {
  await page.waitForTimeout(QUIET_MS);
  await page.evaluate(() => window.__maps2Perf?.reset());
}

test("доска: прокрутка не блокирует поток", async ({ page }, testInfo) => {
  await page.goto("/?perf=1#/");
  await expect(page.getByTestId("live-count")).not.toHaveText("0");
  await armed(page);

  // Прокрутка доски поднимает и роняет студии: шесть живых контекстов
  // сменяют друг друга, и каждая смена — это и загрузка, и освобождение.
  for (const y of [600, 1200, 1800, 2400, 1200, 0]) {
    await page.evaluate((to) => window.scrollTo(0, to), y);
    await page.waitForTimeout(700);
  }

  const measurement = await traceMeasurement(page);
  await attachMeasurement(testInfo, "board", "scroll", measurement);
  expect(measurement.worstBlockMs, `доска, худший блок (${measurement.worstPhase})`)
    .toBeLessThanOrEqual(BLOCK_BUDGET_MS);
});

/// Во сколько раз уход с доски может быть медленнее прямого открытия
/// той же студии. Не ноль: доска честно освобождает шесть контекстов
/// WebGL, и это не бесплатно. Но и не втрое — втрое означает, что она
/// продолжает работать после того, как её закрыли.
const BOARD_HANDOVER_FACTOR = 2;

test("доска: уход на студию не тянет за собой работу доски", async ({ page }, testInfo) => {
  // Ровно тот случай, который однажды стоил двенадцати секунд: шесть
  // студий доски доигрывали свой проход декодирования, отнимая поток у
  // той, которую человек только что открыл.
  //
  // Порог здесь не константа, а та же студия, открытая напрямую:
  // абсолютные миллисекунды зависят от машины, а «во сколько раз
  // дороже через доску» — нет.
  await page.goto("/?perf=1#/");
  const first = page.getByTestId("study").first();
  await expect(first).toHaveAttribute("data-live", "true");
  const href = await first.locator("a.study-open").getAttribute("href");
  // Пауза перед щелчком — не формальность: за неё доска успевает
  // поднять свои шесть студий, и именно с ними придётся делить поток.
  await armed(page);

  const started = Date.now();
  await first.locator("a.study-open").click();
  await expect(page.getByTestId("stage")).toHaveAttribute("data-state", "ready", {
    timeout: 60_000,
  });
  const throughBoardMs = Date.now() - started;
  await attachMeasurement(
    testInfo,
    "board",
    "open-study",
    await traceMeasurement(page, throughBoardMs),
  );

  const direct = await page.context().newPage();
  const directStarted = Date.now();
  await direct.goto(`/${href ?? "#/card/zoom-bands"}`);
  await expect(direct.getByTestId("stage")).toHaveAttribute("data-state", "ready", {
    timeout: 60_000,
  });
  const directMs = Date.now() - directStarted;
  await direct.close();

  expect(
    throughBoardMs,
    `с доски ${throughBoardMs}мс против ${directMs}мс напрямую`,
  ).toBeLessThanOrEqual(directMs * BOARD_HANDOVER_FACTOR);
});

test("галерея: анимация не блокирует поток", async ({ page }, testInfo) => {
  await page.goto("/?perf=1#/showcase");
  await expect(page.getByTestId("showcase")).toHaveAttribute("data-playing", "true");
  await page.waitForTimeout(1_500);
  await armed(page);

  // Четыре сцены крутятся сами: зум, курс и наклон меняются каждый кадр.
  // Это самая плотная нагрузка, какую лаба вообще создаёт.
  await page.waitForTimeout(4_000);

  const measurement = await traceMeasurement(page);
  await attachMeasurement(testInfo, "showcase", "animation", measurement);
  expect(measurement.worstBlockMs, `галерея, худший блок (${measurement.worstPhase})`)
    .toBeLessThanOrEqual(BLOCK_BUDGET_MS);
});
