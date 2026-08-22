// e2e подписей. Ни одно утверждение здесь не имеет формы «подпись X
// видна на z14»: видимость подписи — свойство кадра, а не фичи, и такое
// утверждение флаки по своей природе (ROADMAP-rebuild, «Подписи —
// полный движок»). Проверяются инварианты кадра, которые SDK отдаёт
// списком: непересечение боксов, порядок рангов, детерминизм,
// доля переразмещений при малом сдвиге, потолок бюджета.

import { expect, test, type Locator, type Page } from "./helpers/coverage";

async function openCard(page: Page, id: string): Promise<Locator> {
  await page.goto(`/#/card/${id}`);
  const stage = page.getByTestId("stage");
  await expect(stage).toHaveAttribute("data-state", "ready");
  return stage;
}

function readout(page: Page, key: string): Locator {
  return page.getByTestId(`readout-${key}`);
}

async function number(page: Page, key: string): Promise<number> {
  return Number.parseFloat((await readout(page, key).textContent()) ?? "");
}

// Допуск golden: кадр рисует WebGL, и сглаживание разнится от машины к
// машине. Порог ловит смену набора и раскладки, а не последний пиксель.
// Проверен на дискриминацию: сдвиг образца на 36 px даёт 4% различных
// пикселей — на два порядка выше допуска.
const GOLDEN = { maxDiffPixelRatio: 0.002 } as const;

test.describe("подписи", () => {
  // Эталон и инвариант — одно утверждение: если повёрнутая и
  // наклонённая камера даёт тот же кадр, значит текст не в меше мира.
  test("type-specimen: bearing и tilt не трогают набор", async ({ page }) => {
    const stage = await openCard(page, "type-specimen");
    await expect(readout(page, "bearing")).toHaveText("0.0");
    await expect(readout(page, "tilt")).toHaveText("0.0");
    await expect(stage).toHaveScreenshot("type-specimen.png", GOLDEN);

    await page.getByTestId("bearing-slider").fill("37");
    await page.getByTestId("tilt-slider").fill("55");
    await expect(readout(page, "bearing")).toHaveText("37.0");
    await expect(readout(page, "tilt")).toHaveText("55.0");
    await expect(stage).toHaveScreenshot("type-specimen.png", GOLDEN);
  });

  test("type-specimen: halo и строка меняют кадр", async ({ page }) => {
    const stage = await openCard(page, "type-specimen");
    const plain = await stage.screenshot();
    await page.getByTestId("halo-slider").fill("0.14");
    await expect(readout(page, "halo")).toHaveText("0.14");
    expect(await stage.screenshot()).not.toEqual(plain);

    await page.getByTestId("specimen-text").fill("Northfields 42");
    await expect(readout(page, "text")).toHaveText("Northfields 42");
    expect(await stage.screenshot()).not.toEqual(plain);
  });

  test("labels-collision: размещённые боксы не пересекаются", async ({ page }) => {
    const stage = await openCard(page, "labels-collision");
    // Кандидатов должно быть много больше, чем помещается: именно это
    // заставляет этап отбирать. Считаются те, что кадр действительно
    // взвесил — подписи за пределами экрана отсеиваются до подсчёта,
    // потому что шейпинг каждой из них и был самым дорогим в кадре.
    const candidates = await number(page, "candidates");
    const placed = await number(page, "placed");
    expect(candidates).toBeGreaterThan(placed);
    expect(placed).toBeGreaterThan(0);
    expect(await number(page, "collisions")).toBeGreaterThan(0);
    // Два инварианта этапа, посчитанные хостом из label_debug().
    await expect(readout(page, "overlaps")).toHaveText("0");
    await expect(readout(page, "inversions")).toHaveText("0");
    await expect(stage).toHaveAttribute("data-zoom", "16.00");
  });

  test("labels-collision: дубль через границу тайла размещается один раз", async ({
    page,
  }) => {
    await openCard(page, "labels-collision");
    // На z14 фикстурное место сидит на общем углу четырёх тайлов.
    await page.getByTestId("zoom-slider").fill("14");
    expect(await number(page, "duplicates")).toBeGreaterThan(0);
    await expect(readout(page, "overlaps")).toHaveText("0");
    await expect(readout(page, "inversions")).toHaveText("0");
  });

  test("labels-collision: тогл боксов меняет кадр, показания — нет", async ({ page }) => {
    const stage = await openCard(page, "labels-collision");
    const withBoxes = await stage.screenshot();
    const placed = await number(page, "placed");
    await page.getByTestId("boxes-toggle").uncheck();
    expect(await stage.screenshot()).not.toEqual(withBoxes);
    // Отладочная визуализация ничего не решает — отбор тот же.
    expect(await number(page, "placed")).toBe(placed);
  });

  test("labels-collision: golden", async ({ page }) => {
    const stage = await openCard(page, "labels-collision");
    await expect(stage).toHaveScreenshot("labels-collision.png", GOLDEN);
  });

  test("poi-density: занятость не превышает бюджет и растёт вместе с ним", async ({
    page,
  }) => {
    await openCard(page, "poi-density");
    // Плотное поле кандидатов — предпосылка карточки. Число считает
    // только те, что кадр взвесил: заэкранные отсеиваются раньше.
    expect(await number(page, "candidates")).toBeGreaterThan(500);

    const seen: number[] = [];
    for (const percent of ["1", "3", "8", "15", "25"]) {
      await page.getByTestId("budget-slider").fill(percent);
      const budget = await number(page, "budget");
      const occupancy = await number(page, "occupancy");
      expect(budget).toBeCloseTo(Number(percent), 1);
      expect(occupancy).toBeLessThanOrEqual(budget + 1e-6);
      await expect(readout(page, "overlaps")).toHaveText("0");
      seen.push(await number(page, "placed"));
    }
    // Больший бюджет — не меньше подписей: отбор идёт по рангу и
    // просто останавливается позже.
    for (let i = 1; i < seen.length; i += 1) {
      expect(seen[i]!).toBeGreaterThan(seen[i - 1]!);
    }
  });

  test("viewport-stability: сдвиг до 20 px переразмещает меньше десятой части", async ({
    page,
  }) => {
    const stage = await openCard(page, "viewport-stability");
    for (const pixels of ["1", "5", "12", "20"]) {
      await page.getByTestId("shift-slider").fill(pixels);
      await expect(stage).toHaveAttribute("data-shift", pixels);
      const churn = Number(await stage.getAttribute("data-churn"));
      expect(churn).toBeLessThan(0.1);
      // И тот же кадр при возврате камеры на место.
      await expect(stage).toHaveAttribute("data-deterministic", "true");
    }
  });
});
