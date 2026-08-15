// Жесты на карточке input-flat. Тест шлёт настоящее событие браузера и
// сверяет числа камеры из debug(), а не пиксели: сдвиг центра в градусах,
// зум, неподвижность точки под курсором.

import { expect, test, type Page } from "@playwright/test";

function stage(page: Page) {
  return page.getByTestId("stage");
}

async function openCard(page: Page): Promise<void> {
  await page.goto("/#/card/input-flat");
  await expect(stage(page)).toHaveAttribute("data-state", "ready");
}

async function centre(page: Page): Promise<{ lon: number; lat: number }> {
  const value = await stage(page).getAttribute("data-centre");
  const [lon, lat] = (value ?? "").split(",").map(Number);
  return { lon: lon ?? Number.NaN, lat: lat ?? Number.NaN };
}

async function zoom(page: Page): Promise<number> {
  return Number(await stage(page).getAttribute("data-zoom"));
}

// Сцена растянута по колонке, а канвас держит свой размер: столько
// пикселей канваса приходится на один CSS-пиксель курсора.
async function canvasScale(page: Page): Promise<number> {
  return page.evaluate(() => {
    const canvas = document.querySelector("canvas");
    if (!canvas) throw new Error("на карточке нет канваса");
    return canvas.width / canvas.getBoundingClientRect().width;
  });
}

/** Градусов долготы в одном пикселе канваса на этом зуме. */
function degreesPerPixel(zoomLevel: number): number {
  return 360 / (256 * 2 ** zoomLevel);
}

async function stageBox(page: Page) {
  const box = await stage(page).boundingBox();
  if (!box) throw new Error("сцена не на экране");
  return box;
}

test("drag сдвигает центр ровно на пиксели жеста", async ({ page }) => {
  await openCard(page);
  const box = await stageBox(page);
  const scale = await canvasScale(page);
  const before = await centre(page);
  const level = await zoom(page);

  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x - 120, y, { steps: 6 });

  // Кнопка ещё нажата: инерции нет, и число сверяется точно.
  const after = await centre(page);
  const expected = before.lon + 120 * scale * degreesPerPixel(level);
  const halfPixel = 0.5 * scale * degreesPerPixel(level);
  expect(Math.abs(after.lon - expected)).toBeLessThan(halfPixel);
  expect(Math.abs(after.lat - before.lat)).toBeLessThan(1e-5);
  await page.mouse.up();
});

test("wheel зумит к курсору: точка под курсором остаётся на месте", async ({ page }) => {
  await openCard(page);
  const box = await stageBox(page);
  const scale = await canvasScale(page);
  const before = await centre(page);
  const level = await zoom(page);

  // Курсор правее центра сцены: смещение в пикселях канваса.
  const offsetCss = box.width * 0.3;
  await page.mouse.move(box.x + box.width / 2 + offsetCss, box.y + box.height / 2);
  await page.mouse.wheel(0, -100);

  await expect.poll(() => zoom(page)).toBeCloseTo(level + 0.4, 2);
  const after = await centre(page);
  const offsetPx = offsetCss * scale;
  const groundBefore = before.lon + offsetPx * degreesPerPixel(level);
  const groundAfter = after.lon + offsetPx * degreesPerPixel(await zoom(page));
  const halfPixel = 0.5 * scale * degreesPerPixel(level);
  expect(Math.abs(groundAfter - groundBefore)).toBeLessThan(halfPixel);
  expect(after.lon).toBeGreaterThan(before.lon);
});

test("пинч тачпада берёт круче колеса на той же дельте", async ({ page }) => {
  await openCard(page);
  const level = await zoom(page);
  // Пинч тачпада приезжает как wheel с ctrlKey и куда меньшими дельтами;
  // мышь такое событие не шлёт, поэтому оно собирается руками.
  await page.getByTestId("stage").evaluate((stage) => {
    stage.dispatchEvent(
      new WheelEvent("wheel", { deltaY: -20, ctrlKey: true, bubbles: true, cancelable: true }),
    );
  });
  const pinched = await zoom(page);
  expect(pinched).toBeGreaterThan(level);
  // Колесо на той же дельте дало бы 20/250 = 0.08 уровня, пинч — 0.5.
  expect(pinched - level).toBeCloseTo(0.5, 2);
});

test("dblclick добавляет ровно один уровень", async ({ page }) => {
  await openCard(page);
  const level = await zoom(page);
  await stage(page).dblclick();
  await expect.poll(() => zoom(page)).toBeCloseTo(level + 1, 2);
  await expect(stage(page)).toHaveAttribute("data-moving", "false");
});

test("стрелки и знаки шагают на заявленные величины", async ({ page }) => {
  await openCard(page);
  await stage(page).focus();
  const before = await centre(page);
  const level = await zoom(page);

  await page.keyboard.press("ArrowRight");
  const east = await centre(page);
  const step = 80 * degreesPerPixel(level);
  expect(Math.abs(east.lon - (before.lon + step))).toBeLessThan(1e-4);

  await page.keyboard.press("ArrowUp");
  expect((await centre(page)).lat).toBeGreaterThan(east.lat);

  await page.keyboard.press("=");
  expect(await zoom(page)).toBeCloseTo(level + 1, 2);
  await page.keyboard.press("-");
  expect(await zoom(page)).toBeCloseTo(level, 2);
});

test("после отпускания карта доезжает по инерции и останавливается", async ({ page }) => {
  await openCard(page);
  const box = await stageBox(page);
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;

  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x - 150, y, { steps: 10 });
  await page.mouse.up();

  const released = await centre(page);
  await expect(stage(page)).toHaveAttribute("data-moving", "false");
  const rest = await centre(page);
  expect(rest.lon).toBeGreaterThan(released.lon);
});

test("показания камеры приходят из SDK, а не из карточки", async ({ page }) => {
  await openCard(page);
  await expect(page.getByTestId("readout-zoom")).not.toHaveAttribute("data-pending", "");
  await expect(page.getByTestId("readout-centre")).toContainText("51.5");
  await expect(page.getByTestId("readout-bearing")).toHaveText("0.00°");
});
