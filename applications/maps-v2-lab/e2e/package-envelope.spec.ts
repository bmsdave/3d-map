import fs from "node:fs";
import path from "node:path";
import { expect, test, type Page, type Route } from "@playwright/test";

// Манифест без списка тайлов: пакет на планету не может назвать их
// поимённо — один список был бы гигабайтами JSON до первого кадра, — и
// вместо него несёт конверт: какие уровни есть и какую землю каждый
// покрывает. Клиент считает адрес тайла сам, а 404 читает как «тайла тут
// нет», что для тайлового сервера — обычный ответ, а не поломка.
//
// Вырезка в репозитории список несёт, поэтому здесь он снимается на лету.

const PACKAGE = path.join(process.cwd(), "public", "packages", "trafalgar");
const VIEW_ZOOM = 11;

/** Манифест той же вырезки, но без перечисления тайлов и дайджестов. */
function envelopeManifest(): string {
  const manifest = JSON.parse(fs.readFileSync(path.join(PACKAGE, "manifest.json"), "utf8"));
  const { tiles, tile_digests, ...envelope } = manifest;
  expect(tiles, "вырезка перечисляет тайлы, иначе снимать нечего").toBeTruthy();
  expect(envelope.bounds, "конверт — это bounds по уровням").toBeTruthy();
  return JSON.stringify(envelope);
}

async function serveEnvelope(page: Page, missing: string[] = []): Promise<string[]> {
  const asked: string[] = [];
  await page.route("**/packages/trafalgar/manifest.json", (route: Route) =>
    route.fulfill({ status: 200, contentType: "application/json", body: envelopeManifest() }));
  await page.route("**/packages/trafalgar/**/*.mt2", (route: Route) => {
    const tilePath = route.request().url().split("/packages/trafalgar/")[1]?.split("?")[0] ?? "";
    asked.push(tilePath);
    if (missing.includes(tilePath)) {
      route.fulfill({ status: 404, contentType: "text/plain", body: "no tile" });
      return;
    }
    route.fulfill({
      status: 200,
      contentType: "application/octet-stream",
      body: fs.readFileSync(path.join(PACKAGE, tilePath)),
    });
  });
  return asked;
}

async function openAt(page: Page, zoom: number): Promise<void> {
  await page.goto("/#/card/map-real");
  const stage = page.locator("main.card-page").getByTestId("stage");
  await expect(stage).toHaveAttribute("data-state", "ready", { timeout: 60_000 });
  await page.getByTestId("map-real-zoom").fill(String(zoom));
  const canvas = stage.locator("canvas");
  for (let attempt = 0; attempt < 24; attempt += 1) {
    await canvas.dispatchEvent("pointerdown", { clientX: 1, clientY: 1 });
    await canvas.dispatchEvent("pointermove", { clientX: 2, clientY: 2 });
    await canvas.dispatchEvent("pointerup", { clientX: 2, clientY: 2 });
    if ((await page.evaluate(() => window.maps2?.missingTiles().length)) === 0) break;
  }
}

test("пакет без списка тайлов грузится по вычисленным адресам", async ({ page }) => {
  const asked = await serveEnvelope(page);

  await openAt(page, VIEW_ZOOM);
  const state = await page.evaluate(() => {
    window.maps2?.render();
    return window.maps2?.debug();
  });

  expect(asked.length, "тайлы всё-таки запрашивались").toBeGreaterThan(0);
  expect(state?.tiles_drawn ?? 0, "и доехали до кадра").toBeGreaterThan(0);
  expect(state?.cpu_tiles ?? 0).toBeGreaterThan(0);
});

test("404 внутри конверта — это отсутствие тайла, а не отказ пакета", async ({ page }) => {
  // Дыра ровно там, куда камера смотрит: конверт покрывает эту землю, а
  // сборка тайла не написала.
  const hole = "11/1023/680.mt2";
  const asked = await serveEnvelope(page, [hole]);

  await openAt(page, VIEW_ZOOM);
  const state = await page.evaluate(() => {
    window.maps2?.render();
    return window.maps2?.debug();
  });

  expect(asked, "дыру спросили").toContain(hole);
  // Остальные тайлы кадра приехали: одна дыра не роняет пачку, с которой
  // ехала.
  expect(state?.tiles_drawn ?? 0).toBeGreaterThan(0);
  // И спрашивать её снова не стали: 404 запоминается, иначе каждое
  // движение камеры било бы в тот же пустой адрес.
  const before = asked.filter((tilePath) => tilePath === hole).length;
  await page.evaluate(() => window.maps2?.setCentre(-0.129, 51.509));
  await openAt(page, VIEW_ZOOM);
  expect(asked.filter((tilePath) => tilePath === hole).length).toBe(before);
});
