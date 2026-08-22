// Рельеф: показания из debug() и golden-скриншот сцены рельефа.
// Утверждения — про состав кадра и параметры стиля, не про «видно гору»:
// саму гору принимает эталон, а «северо-западный склон светлее» проверено
// нативным тестом в maps2-render поверх той же фикстуры.

import { expect, test, type Page } from "./helpers/coverage";

async function openCard(page: Page, id: string): Promise<void> {
  await page.goto(`/#/card/${id}`);
  await expect(page.getByTestId("stage")).toHaveAttribute("data-state", "ready");
}

test("terrain-shade: высоты доехали, стиль — ручка, а не константа", async ({ page }) => {
  await openCard(page, "terrain-shade");
  const stage = page.getByTestId("stage");

  // Растровая секция прочитана: тайлы сцены несут высоты, и под центром
  // камеры стоит измеренная земля, а не «нет данных». Раньше здесь
  // ждали больше километра — там стоял синтетический конус; под
  // Трафальгарской площадью Copernicus меряет единицы метров.
  await expect(page.getByTestId("readout-height-tiles")).not.toHaveText("0");
  const height = await page.getByTestId("readout-height").textContent();
  expect(height).not.toContain("нет данных");
  const metres = Number(height?.replace(/[^\d-]/g, ""));
  expect(metres).toBeGreaterThan(-12000);
  expect(metres).toBeLessThan(9000);
  await expect(page.getByTestId("readout-shape")).toHaveText("flat");

  // Выразительность — параметр стиля: ползунок доезжает до SDK.
  await page.getByTestId("expressiveness-slider").fill("1");
  await expect(stage).toHaveAttribute("data-expressiveness", "1.00");
  await page.getByTestId("expressiveness-slider").fill("0.5");
  await expect(stage).toHaveAttribute("data-expressiveness", "0.50");

  // Затенение выключается, не унося с собой поверхность.
  await page.getByTestId("relief-toggle").uncheck();
  await expect(stage).toHaveAttribute("data-relief", "false");
  await expect(page.getByTestId("readout-height-tiles")).not.toHaveText("0");
  await page.getByTestId("relief-toggle").check();
  await expect(stage).toHaveAttribute("data-relief", "true");
});

test("terrain-shade: сцена рельефа совпадает с эталоном", async ({ page }) => {
  await openCard(page, "terrain-shade");
  await expect(page.getByTestId("readout-height-tiles")).not.toHaveText("0");
  await expect(page.locator("canvas")).toHaveScreenshot("terrain-shade.png", {
    maxDiffPixelRatio: 0.01,
  });
});

test("globe-relief: рельеф остаётся в глобусной проекции", async ({ page }) => {
  await openCard(page, "globe-relief");
  const stage = page.getByTestId("stage");
  await expect(stage).toHaveAttribute("data-shape", "globe");
  await expect(page.getByTestId("readout-height-tiles")).not.toHaveText("0");
  await expect(stage.locator("canvas")).toHaveScreenshot("globe-relief.png", {
    maxDiffPixelRatio: 0.01,
  });
});
