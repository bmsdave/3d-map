import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { expect, test, type Page, type Route } from "@playwright/test";

// Пакет, обрезанный по высотам: у тайлов глубже порога своей растровой
// секции нет, и поверхность под ними рендерер обязан прочитать у
// ближайшего предка. Тайлы правятся на лету, а не лежат фикстурой:
// вырезка в репозитории несёт высоты на каждом уровне, а сто мегабайт
// второй копии ради одного утверждения того не стоят. Раз тайлы
// переписаны, переписан и манифест: загрузчик сверяет sha256 каждого
// тайла и на несовпадении отказывается от пакета целиком.
//
// Порог здесь ниже боевого (`TERRAIN_MAX_Z` = 12), и намеренно: карточка
// гасит рельеф с z12, а проверять нечего там, где затенение выключено.
// Обрезка по z10 и камера на z11 ставят чтение предка ровно под тот
// кадр, где рельеф виден, — и на один уровень, то есть на ту разницу,
// на которой поверхность предка обязана совпасть с настоящей.

const STRIP_ABOVE = 10;
const VIEW_ZOOM = 11;
const RASTER_CLASSES = [0xff00, 0xff01];
const PACKAGE = path.join(process.cwd(), "public", "packages", "trafalgar");

/** Тайл без растровой секции: таблица пересобрана, смещения пересчитаны. */
function stripHeights(bytes: Buffer): Buffer {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const count = view.getUint16(16, true);
  const base = 20 + 10 * count;
  const sections = Array.from({ length: count }, (_, i) => ({
    cls: view.getUint16(20 + 10 * i, true),
    offset: view.getUint32(22 + 10 * i, true),
    length: view.getUint32(26 + 10 * i, true),
  }));
  const keep = sections.filter((section) => !RASTER_CLASSES.includes(section.cls));
  if (keep.length === count) return bytes;

  const head = Buffer.alloc(20 + 10 * keep.length);
  bytes.copy(head, 0, 0, 16);
  head.writeUInt16LE(keep.length, 16);
  head.writeUInt16LE(0, 18);
  const payloads: Buffer[] = [];
  let offset = 0;
  keep.forEach((section, index) => {
    head.writeUInt16LE(section.cls, 20 + 10 * index);
    head.writeUInt32LE(offset, 22 + 10 * index);
    head.writeUInt32LE(section.length, 26 + 10 * index);
    payloads.push(bytes.subarray(base + section.offset, base + section.offset + section.length));
    offset += section.length;
  });
  return Buffer.concat([head, ...payloads]);
}

/** Байты тайла, какими их увидит загрузчик при обрезке выше `above`. */
function tileBytes(tilePath: string, above: number): Buffer {
  const raw = fs.readFileSync(path.join(PACKAGE, tilePath));
  return Number(tilePath.split("/")[0]) > above ? stripHeights(raw) : raw;
}

async function servePackage(page: Page, above: number | null): Promise<void> {
  if (above === null) return;
  const manifest = JSON.parse(fs.readFileSync(path.join(PACKAGE, "manifest.json"), "utf8"));
  const digests: Record<string, string> = {};
  for (const tilePath of manifest.tiles as string[]) {
    digests[tilePath] = createHash("sha256").update(tileBytes(tilePath, above)).digest("hex");
  }
  const served = JSON.stringify({ ...manifest, tile_digests: digests });
  await page.route("**/packages/trafalgar/manifest.json", (route: Route) =>
    route.fulfill({ status: 200, contentType: "application/json", body: served }));
  await page.route("**/packages/trafalgar/**/*.mt2", (route: Route) => {
    const tilePath = route.request().url().split("/packages/trafalgar/")[1]?.split("?")[0] ?? "";
    route.fulfill({
      status: 200,
      contentType: "application/octet-stream",
      body: tileBytes(tilePath, above),
    });
  });
}

interface Shot {
  pixels: number[];
  centreHeight: number | null;
}

/** Кадр и высота под камерой на одном и том же виде. */
async function shoot(page: Page, above: number | null): Promise<Shot> {
  await servePackage(page, above);
  await page.goto("/#/card/map-real");
  const stage = page.locator("main.card-page").getByTestId("stage");
  await expect(stage).toHaveAttribute("data-state", "ready", { timeout: 60_000 });

  // Камеру двигают ручки карточки: догрузку ведёт она, и до её обработчика
  // камера, сдвинутая мимо, не доходит — тайлов под новый кадр не запросят.
  await page.getByTestId("map-real-zoom").fill(String(VIEW_ZOOM));

  const canvas = stage.locator("canvas");
  for (let attempt = 0; attempt < 24; attempt += 1) {
    await canvas.dispatchEvent("pointerdown", { clientX: 1, clientY: 1 });
    await canvas.dispatchEvent("pointermove", { clientX: 2, clientY: 2 });
    await canvas.dispatchEvent("pointerup", { clientX: 2, clientY: 2 });
    // Ждём и запасной уровень: тайлы своего уровня приезжают первыми, а
    // поверхность под ними лежит у предка, и пока он в пути карта читает
    // то, что осталось от прошлой камеры.
    const pending = await page.evaluate(
      () => (window.maps2?.missingTiles().length ?? 1) + (window.maps2?.fallbackTiles().length ?? 1),
    );
    if (pending === 0) break;
  }

  return page.evaluate(() => {
    const map = window.maps2;
    if (!map) return { pixels: [], centreHeight: null };
    // Рендер прямо перед пробой: буфер отрисовки не сохраняется между
    // кадрами, и `samplePixel` без свежего кадра читает пустоту.
    map.render();
    const pixels: number[] = [];
    for (let y = 40; y < 480; y += 40) {
      for (let x = 40; x < 720; x += 40) pixels.push(...map.samplePixel(x, y).slice(0, 3));
    }
    return { pixels, centreHeight: map.debug().centre_height_m };
  });
}

/** Насколько два кадра расходятся, в среднем по каналу. */
function distance(left: number[], right: number[]): number {
  const pairs = Math.min(left.length, right.length);
  if (pairs === 0) return Number.POSITIVE_INFINITY;
  let total = 0;
  for (let i = 0; i < pairs; i += 1) total += Math.abs((left[i] ?? 0) - (right[i] ?? 0));
  return total / pairs;
}

test("тайл без своих высот берёт поверхность у предка, а не плоскость", async ({ browser }) => {
  // Три кадра одной и той же земли: со своими высотами на каждом уровне,
  // с высотами только до z10, и вовсе без высот.
  const shot = async (above: number | null) => {
    const page = await browser.newPage();
    try {
      return await shoot(page, above);
    } finally {
      await page.close();
    }
  };
  const whole = await shot(null);
  const capped = await shot(STRIP_ABOVE);
  const flat = await shot(0);

  expect(whole.pixels.length, "кадр снят").toBeGreaterThan(0);

  // Высота под камерой у предка — та же самая: уровнем выше лежит тот же
  // DEM, пересэмплированный вдвое реже, и посередине родительской ячейки
  // билинейная выборка возвращает ровно его значение.
  expect(capped.centreHeight).toBe(whole.centreHeight);
  expect(flat.centreHeight, "без высот отвечать нечем").toBeNull();

  // Рельеф вообще виден: без этого сравнивать кадры не о чем.
  expect(distance(whole.pixels, flat.pixels)).toBeGreaterThan(1);
  // Кадр по предку — не плоскость: если бы предка не читали, он совпал бы
  // с плоским до пикселя.
  expect(distance(capped.pixels, flat.pixels)).toBeGreaterThan(1);
  // И он заметно ближе к настоящей поверхности, чем к её отсутствию.
  expect(distance(capped.pixels, whole.pixels))
    .toBeLessThan(distance(flat.pixels, whole.pixels) * 0.75);
});
