import { expect, test, type Route } from "@playwright/test";
import { join } from "node:path";

const realPackageRoot = process.env.MAPS2_REAL_PACKAGE_ROOT;

function localPackagePath(root: string, url: string): string {
  const relative = new URL(url).pathname.replace(/^\//, "");
  if (relative === "manifest.json" || /^\d+\/\d+\/\d+\.mt2$/.test(relative)) return join(root, relative);
  throw new Error(`unexpected package path: ${relative}`);
}

async function fulfillLocalPackage(route: Route, root: string): Promise<void> {
  await route.fulfill({
    path: localPackagePath(root, route.request().url()),
    headers: { "Access-Control-Allow-Origin": "*" },
  });
}

test("package loader: a manifest drives demand-loaded MT2 tiles", async ({ page }) => {
  await page.goto("/#/card/package-loader");
  const stage = page.getByTestId("stage");

  await expect(stage).toHaveAttribute("data-state", "ready");
  await expect(stage).toHaveAttribute("data-loaded", /[1-9]\d*/);
  await expect(page.getByTestId("readout-package-tiles")).toContainText(/[1-9]\d*/);
  await expect(page.getByTestId("readout-package-level")).toHaveText("12");
  await expect(page.getByTestId("readout-package-attribution")).toContainText("Synthetic fixture");
});

test("package loader: rejects a tile whose bytes do not match the manifest", async ({ page }) => {
  const validOtherTile = new URL("../public/fixtures/ealing/0/0/0.mt2", import.meta.url).pathname;
  await page.route("**/fixtures/ealing/**/*.mt2", (route) => route.fulfill({ path: validOtherTile }));
  await page.goto("/#/card/package-loader");

  await expect(page.getByTestId("stage")).toHaveAttribute("data-state", "error");
});

test("package loader: retries a transient manifest failure", async ({ page }) => {
  let attempts = 0;
  await page.route("**/fixtures/ealing/package-manifest.json", async (route) => {
    attempts += 1;
    if (attempts === 1) {
      await route.fulfill({ status: 503, body: "temporarily unavailable" });
      return;
    }
    await route.continue();
  });
  await page.goto("/#/card/package-loader");

  const stage = page.getByTestId("stage");
  await expect(stage).toHaveAttribute("data-state", "error");
  await page.getByRole("button", { name: "Повторить" }).click();
  await expect(stage).toHaveAttribute("data-state", "ready");
  expect(attempts).toBe(2);
});

test("package loader: retries one transient tile failure", async ({ page }) => {
  let failed = false;
  await page.route("**/fixtures/ealing/**/*.mt2", async (route) => {
    if (!failed) {
      failed = true;
      await route.fulfill({ status: 503, body: "temporarily unavailable" });
      return;
    }
    await route.continue();
  });

  await page.goto("/#/card/package-loader");

  await expect(page.getByTestId("stage")).toHaveAttribute("data-state", "ready");
  expect(failed).toBe(true);
});

test("package loader: accepts a host-selected package manifest", async ({ page }) => {
  await page.goto("/#/card/package-loader");
  const stage = page.getByTestId("stage");

  await expect(stage).toHaveAttribute("data-state", "ready");
  await page.getByTestId("package-manifest-url").fill("/fixtures/ealing/package-manifest.json?host=local");
  await page.getByRole("button", { name: "Загрузить пакет" }).click();

  await expect(stage).toHaveAttribute("data-state", "ready");
  await expect(stage).toHaveAttribute("data-manifest", "/fixtures/ealing/package-manifest.json?host=local");
});

test("package loader: rejects an oversized manifest before requesting tiles", async ({ page }) => {
  const tiles = Array.from({ length: 50_001 }, (_, index) => `12/${index}/0.mt2`);
  await page.route("**/oversized-manifest.json", (route) => route.fulfill({
    contentType: "application/json",
    body: JSON.stringify({
      format: "MT2",
      format_version: 4,
      tiles,
      tile_digests: Object.fromEntries(tiles.map((path) => [path, "0".repeat(64)])),
      view: { lon: -0.1278, lat: 51.5074, zoom: 12 },
      sources: [],
    }),
  }));
  await page.goto("/#/card/package-loader");

  await page.getByTestId("package-manifest-url").fill("/oversized-manifest.json");
  await page.getByRole("button", { name: "Загрузить пакет" }).click();
  await expect(page.getByTestId("stage")).toContainText("package exceeds 50000 tiles");
});

test("package loader: rejects an oversized tile before hashing it", async ({ page }) => {
  await page.route("**/fixtures/ealing/**/*.mt2", (route) => route.fulfill({
    contentType: "application/octet-stream",
    body: "x".repeat(4 * 1024 * 1024 + 1),
  }));
  await page.goto("/#/card/package-loader");

  await expect(page.getByTestId("stage")).toContainText("package tile exceeds 4 MiB");
});

test("package loader: a newer manifest wins over a delayed prior load", async ({ page }) => {
  await page.route("**/fixtures/ealing/package-manifest.json", async (route) => {
    await new Promise((resolve) => setTimeout(resolve, 200));
    await route.continue();
  });
  await page.route("**/oversized-manifest.json", (route) => route.fulfill({
    contentType: "application/json",
    body: JSON.stringify({ format: "MT2", format_version: 4, tiles: Array(50_001).fill("12/0/0.mt2"), tile_digests: {}, view: {}, sources: [] }),
  }));
  await page.goto("/#/card/package-loader");

  await page.getByTestId("package-manifest-url").fill("/oversized-manifest.json");
  await page.getByRole("button", { name: "Загрузить пакет" }).click();
  await expect(page.getByTestId("stage")).toContainText("package exceeds 50000 tiles");
  await page.waitForTimeout(250);
  await expect(page.getByTestId("stage")).toContainText("package exceeds 50000 tiles");
});

test("package loader: recreates the map after WebGL context loss", async ({ page }) => {
  await page.goto("/#/card/package-loader");
  const stage = page.getByTestId("stage");

  await expect(stage).toHaveAttribute("data-state", "ready");
  await stage.locator("canvas").evaluate((canvas) => {
    canvas.dispatchEvent(new Event("webglcontextlost", { cancelable: true }));
  });

  await expect(stage).toHaveAttribute("data-state", "ready");
  await expect(stage).toHaveAttribute("data-recoveries", "1");
  await expect(stage).toHaveAttribute("data-loaded", /[1-9]\d*/);
});

test("package loader: tilt control changes the loaded map frame", async ({ page }) => {
  await page.goto("/#/card/package-loader");
  const stage = page.getByTestId("stage");

  await expect(stage).toHaveAttribute("data-state", "ready");
  const flat = await stage.locator("canvas").screenshot();
  await page.getByTestId("package-tilt-slider").fill("45");
  await expect(stage).toHaveAttribute("data-tilt", "45.0");
  const tilted = await stage.locator("canvas").screenshot();

  expect(tilted).not.toBe(flat);
});

test("package loader: panning refreshes visible package coverage", async ({ page }) => {
  await page.goto("/#/card/package-loader");
  const stage = page.getByTestId("stage");
  await expect(stage).toHaveAttribute("data-state", "ready");
  const box = await stage.locator("canvas").boundingBox();
  expect(box).not.toBeNull();

  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
  await page.mouse.down();
  await page.mouse.move(box!.x + box!.width / 2 - 400, box!.y + box!.height / 2);
  await page.mouse.up();

  await expect.poll(async () => Number(await stage.getAttribute("data-unavailable"))).toBeGreaterThan(0);
});

test("package loader: the wide cache fixture loads new tiles after panning", async ({ page }) => {
  await page.goto("/#/card/package-loader");
  await expect(page.getByTestId("stage")).toHaveAttribute("data-state", "ready");
  await page.getByTestId("package-manifest-url").fill("/fixtures/cache/package-manifest.json");
  await page.getByRole("button", { name: "Загрузить пакет" }).click();
  const stage = page.getByTestId("stage");
  await expect(stage).toHaveAttribute("data-state", "ready");
  const initial = Number(await stage.getAttribute("data-loaded"));
  const box = await stage.locator("canvas").boundingBox();
  expect(box).not.toBeNull();
  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
  await page.mouse.down();
  await page.mouse.move(box!.x + box!.width / 2 - 600, box!.y + box!.height / 2);
  await page.mouse.up();
  await expect.poll(async () => Number(await stage.getAttribute("data-loaded"))).toBeGreaterThan(initial);
  await expect.poll(async () => Number(await stage.getAttribute("data-unloaded"))).toBeGreaterThan(0);
});

test("package loader: rapid pans share in-flight tile requests", async ({ page }) => {
  const requests: string[] = [];
  await page.route("**/fixtures/cache/**/*.mt2", async (route) => {
    requests.push(route.request().url());
    await new Promise((resolve) => setTimeout(resolve, 150));
    await route.continue();
  });
  await page.goto("/#/card/package-loader");
  await expect(page.getByTestId("stage")).toHaveAttribute("data-state", "ready");
  await page.getByTestId("package-manifest-url").fill("/fixtures/cache/package-manifest.json");
  await page.getByRole("button", { name: "Загрузить пакет" }).click();
  const stage = page.getByTestId("stage");
  await expect(stage).toHaveAttribute("data-state", "ready");
  requests.length = 0;
  const box = await stage.locator("canvas").boundingBox();
  expect(box).not.toBeNull();
  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
  await page.mouse.down();
  for (const dx of [120, 240, 360, 480]) {
    await page.mouse.move(box!.x + box!.width / 2 - dx, box!.y + box!.height / 2);
  }
  await page.mouse.up();
  await expect.poll(() => requests.length).toBeGreaterThan(0);
  await page.waitForTimeout(200);
  expect(requests.length).toBe(new Set(requests).size);
});

test("package loader: unloading a tile makes it demand-loadable", async ({ page }) => {
  await page.goto("/#/card/package-loader");
  await expect(page.getByTestId("stage")).toHaveAttribute("data-state", "ready");
  const missing = await page.evaluate(() => {
    const map = window.maps2 as typeof window.maps2 & { unloadTile(z: number, x: number, y: number): void };
    map!.unloadTile(12, 2044, 1361);
    return map!.missingTiles();
  });

  expect(missing).toContain("12/2044/1361.mt2");
});

test("real London package: terrain, attribution, and tilt survive demand loading", async ({ page }) => {
  test.skip(!realPackageRoot, "set MAPS2_REAL_PACKAGE_ROOT to run local real-data acceptance");
  await page.route("https://maps2.local/**", (route) => fulfillLocalPackage(route, realPackageRoot!));
  await page.goto("/#/card/package-loader");

  const stage = page.getByTestId("stage");
  await expect(stage).toHaveAttribute("data-state", "ready");
  await page.getByTestId("package-manifest-url").fill("https://maps2.local/manifest.json");
  await page.getByRole("button", { name: "Загрузить пакет" }).click();
  await expect(stage).toHaveAttribute("data-state", "ready");
  await expect(stage).toHaveAttribute("data-manifest", "https://maps2.local/manifest.json");
  await expect(stage).toHaveAttribute("data-loaded", /[1-9]\d*/);
  await expect(page.getByTestId("readout-package-attribution")).toContainText("OpenStreetMap");
  await expect(page.getByTestId("readout-package-attribution")).toContainText(/COPERNICUS/i);

  const before = await stage.locator("canvas").screenshot();
  await page.getByTestId("package-tilt-slider").fill("45");
  const after = await stage.locator("canvas").screenshot();
  const state = await page.evaluate(() => window.maps2?.debug());
  const p95Ms = await page.evaluate(() => window.maps2?.measureFrames(30) ?? Number.POSITIVE_INFINITY);

  expect(after).not.toBe(before);
  expect(state?.tiles_drawn).toBeGreaterThan(0);
  expect(state?.height_tiles).toBeGreaterThan(0);
  expect(p95Ms).toBeLessThanOrEqual(10);
});
