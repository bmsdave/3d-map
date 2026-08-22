// roads-micro: реальный перекрёсток на живом SDK. Утверждения — про
// стыки, казинг и ширины из debug(), а не про пиксели; пиксели
// проверяет единственный golden-скриншот в конце.
//
// Точных чисел здесь больше нет: синтетическая сцена держала ровно по
// одному стыку каждого вида, город даёт их тысячами. Проверяется то,
// что и проверялось, — правило, а не его старый счёт.

import { expect, test, type Page } from "./helpers/coverage";

async function openRoads(page: Page) {
  await page.goto("/#/card/roads-micro");
  const stage = page.getByTestId("stage");
  await expect(stage).toHaveAttribute("data-state", "ready");
  return stage;
}

function joins(page: Page) {
  return page.getByTestId("readout-joins");
}

test("сцена собирается со стыками обоих видов", async ({ page }) => {
  const stage = await openRoads(page);
  // Плавные повороты митрятся, острые углы бевелятся: на настоящей
  // сети есть и те, и другие.
  await expect(joins(page)).toContainText(/митра [1-9]\d*/);
  await expect(joins(page)).toContainText(/бевел [1-9]\d*/);
  const candidates = await stage.getAttribute("data-label-candidates");
  expect(Number(candidates)).toBeGreaterThan(0);
});

test("предел митры превращает бевелы в митры", async ({ page }) => {
  const stage = await openRoads(page);
  const bevels = async () => {
    const text = (await joins(page).textContent()) ?? "";
    return Number(/бевел (\d+)/.exec(text)?.[1] ?? -1);
  };

  // Предел решает, какой угол ещё можно смитрить. Поднимая его, мы
  // забираем углы у бевела и отдаём митре — счёт бевелов обязан падать,
  // и ни один угол не может стать бевелом от роста предела.
  await page.getByTestId("miter-limit").selectOption("1.5");
  await expect(stage).toHaveAttribute("data-miter-limit", "1.5");
  const tight = await bevels();
  expect(tight).toBeGreaterThan(0);

  await page.getByTestId("miter-limit").selectOption("2");
  const middle = await bevels();
  expect(middle).toBeLessThanOrEqual(tight);

  await page.getByTestId("miter-limit").selectOption("4");
  await expect(stage).toHaveAttribute("data-miter-limit", "4.0");
  const loose = await bevels();
  expect(loose).toBeLessThanOrEqual(middle);
  expect(loose).toBeLessThan(tight);
});

test("тогл казинга снимает нижний проход", async ({ page }) => {
  const stage = await openRoads(page);
  await expect(stage).toHaveAttribute("data-casing", "true");
  await expect(page.getByTestId("readout-casing")).toHaveText("включён");

  await page.getByTestId("casing-toggle").uncheck();
  await expect(stage).toHaveAttribute("data-casing", "false");
  await expect(page.getByTestId("readout-casing")).toHaveText("выключен");
});

test("ширина дороги задаётся в экранных пикселях", async ({ page }) => {
  await openRoads(page);
  const widths = page.getByTestId("readout-widths");
  // Рампа стиля на z17: магистраль заметно шире улицы.
  await expect(widths).toContainText("магистраль 12.0");
  await expect(widths).toContainText("улица 4.5");

  await page.getByTestId("width-motorway").fill("20");
  await page.getByTestId("width-motorway").blur();
  await expect(widths).toContainText("магистраль 20.0");
});

// Столбец пикселей поперёк дороги: земля → казинг → заливка → казинг →
// земля. Проба — хук вне горячего пути, кадр за неё не платит.
// x=480 попадает на проезжую часть у Northumberland Avenue, к востоку
// от центра сцены; строка y=230..270 пересекает её поперёк.
async function columnAcrossMotorway(page: Page, x = 480): Promise<number[]> {
  return page.evaluate((sampleX) => {
    const map = (
      window as unknown as {
        maps2: { render(): void; samplePixel(x: number, y: number): number[] };
      }
    ).maps2;
    map.render();
    const brightness: number[] = [];
    for (let y = 230; y <= 270; y++) {
      const [r, g, b] = map.samplePixel(sampleX, y);
      brightness.push((r ?? 0) + (g ?? 0) + (b ?? 0));
    }
    return brightness;
  }, x);
}

const CENTRE_INDEX = 249 - 230;

test("казинг темнее заливки, центр ленты — цвет заливки", async ({ page }) => {
  await openRoads(page);
  const column = await columnAcrossMotorway(page);
  const land = column[0] ?? 0;
  const centre = column[CENTRE_INDEX] ?? 0;

  expect(centre).toBeGreaterThan(land);
  const rim = Math.min(...column);
  expect(rim).toBeLessThan(land);

  // Тёмная кромка обязана быть по обе стороны от центра, иначе это не
  // казинг, а случайно попавший в пробу сосед.
  const dark = column.flatMap((v, i) => (v < land ? [i] : []));
  expect(Math.min(...dark)).toBeLessThan(CENTRE_INDEX);
  expect(Math.max(...dark)).toBeGreaterThan(CENTRE_INDEX);
});

test("снятый казинг убирает тёмную кромку с ленты", async ({ page }) => {
  await openRoads(page);
  await page.getByTestId("casing-toggle").uncheck();
  const column = await columnAcrossMotorway(page);
  const land = column[0] ?? 0;
  expect(column[CENTRE_INDEX]).toBeGreaterThan(land);
  expect(Math.min(...column)).toBeGreaterThanOrEqual(land);
});

test("roads-micro: эталонный кадр", async ({ page }) => {
  const stage = await openRoads(page);
  await expect(stage.locator("canvas")).toHaveScreenshot("roads-micro.png", {
    maxDiffPixelRatio: 0.002,
  });
});
