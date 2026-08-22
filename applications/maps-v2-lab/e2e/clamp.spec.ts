// Вырезка конечна, и у неё есть край. Рендерер за краем честно
// показал бы пустоту — возвращает камеру хост: это его пакет и его
// решение, куда можно смотреть. Проверяется там, где камерой двигает
// человек: свободный ввод и непрерывный зум.
//
// Оракул здесь — границы из manifest вырезки, а не missingTiles():
// пирамида map-v1 сама по себе не сплошная (мировые слои кончаются на
// z7, город начинается с z8), и отсутствующий тайл там — норма, на
// которую рендерер отвечает откатом к более грубому уровню.

import { expect, test, type Page } from "./helpers/coverage";

interface Box { west: number; south: number; east: number; north: number }

async function boundsFor(page: Page, level: number): Promise<Box> {
  return page.evaluate(async (want) => {
    const manifest = await (await fetch("/packages/trafalgar/manifest.json")).json();
    for (let level = want; level >= 0; level -= 1) {
      const box = manifest.bounds[String(level)];
      if (box) return box as Box;
    }
    throw new Error(`no bounds at or above z${want}`);
  }, level);
}

async function camera(page: Page) {
  return page.evaluate(() => {
    const state = window.maps2!.debug();
    return { lon: state.centre_lon, lat: state.centre_lat, level: state.tile_level, drawn: state.tiles_drawn };
  });
}

/** Камера возвращается через f32 рендерера: на краю вырезки это
 *  расхождение в восьмом знаке, то есть доли миллиметра на земле.
 *  Тайл на z16 — пять тысячных градуса, так что запас безопасен. */
const EDGE_EPSILON = 1e-5;

async function expectOnPack(page: Page, note: string): Promise<void> {
  const view = await camera(page);
  const box = await boundsFor(page, view.level);
  expect(view.lon, `${note}: долгота внутри вырезки`).toBeGreaterThanOrEqual(box.west - EDGE_EPSILON);
  expect(view.lon, `${note}: долгота внутри вырезки`).toBeLessThanOrEqual(box.east + EDGE_EPSILON);
  expect(view.lat, `${note}: широта внутри вырезки`).toBeGreaterThanOrEqual(box.south - EDGE_EPSILON);
  expect(view.lat, `${note}: широта внутри вырезки`).toBeLessThanOrEqual(box.north + EDGE_EPSILON);
  expect(view.drawn, `${note}: кадр не пустой`).toBeGreaterThan(0);
}

test("input-flat: перетаскивание не уводит камеру за край вырезки", async ({ page }) => {
  await page.goto("/#/card/input-flat");
  await expect(page.getByTestId("stage")).toHaveAttribute("data-state", "ready");
  const box = (await page.getByTestId("stage").locator("canvas").boundingBox())!;
  const centre = { x: box.x + box.width / 2, y: box.y + box.height / 2 };

  // Десять бросков через весь холст в одну сторону — много дальше, чем
  // простирается покрытие вырезки на этом уровне.
  for (const [dx, dy] of [[-1, 0], [1, 0], [0, -1], [0, 1]]) {
    for (let pull = 0; pull < 10; pull += 1) {
      await page.mouse.move(centre.x, centre.y);
      await page.mouse.down();
      await page.mouse.move(centre.x + dx * 600, centre.y + dy * 400);
      await page.mouse.up();
    }
    await page.waitForTimeout(500);
    await expectOnPack(page, `бросок (${dx}, ${dy})`);
  }
});

test("zoom-bands: непрерывный зум остаётся на земле вырезки", async ({ page }) => {
  await page.goto("/#/card/zoom-bands");
  await expect(page.getByTestId("stage")).toHaveAttribute("data-state", "ready");

  for (const zoom of ["0", "4", "8", "11", "13", "15", "17"]) {
    await page.getByTestId("zoom-slider").fill(zoom);
    await page.waitForTimeout(400);
    await expectOnPack(page, `z${zoom}`);
  }
});
