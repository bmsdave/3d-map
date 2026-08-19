import { expect, test, type Route } from "@playwright/test";
import { join } from "node:path";

const worldPackageRoot = process.env.MAPS2_WORLD_PACKAGE_ROOT;
const cityPackageRoot = process.env.MAPS2_REAL_PACKAGE_ROOT;

function localPackagePath(root: string, pathname: string): string {
  const relative = pathname.replace(/^\//, "");
  if (relative === "manifest.json" || /^\d+\/\d+\/\d+\.mt2$/.test(relative)) return join(root, relative);
  throw new Error(`unexpected package path: ${relative}`);
}

async function fulfillPrefixed(route: Route, prefix: string, root: string): Promise<void> {
  const pathname = new URL(route.request().url()).pathname;
  await route.fulfill({
    path: localPackagePath(root, pathname.replace(new RegExp(`^/${prefix}/`), "")),
    headers: { "Access-Control-Allow-Origin": "*" },
  });
}

test("globe-real: a real world package and a real city package compose on one map", async ({ page }) => {
  test.skip(
    !worldPackageRoot || !cityPackageRoot,
    "set MAPS2_WORLD_PACKAGE_ROOT and MAPS2_REAL_PACKAGE_ROOT to run this real-data acceptance test",
  );
  await page.route("https://maps2.local/world/**", (route) => fulfillPrefixed(route, "world", worldPackageRoot!));
  await page.route("https://maps2.local/city/**", (route) => fulfillPrefixed(route, "city", cityPackageRoot!));
  await page.goto("/#/card/globe-real");

  const stage = page.getByTestId("stage");
  await expect(stage).toHaveAttribute("data-state", "idle");
  await page.getByTestId("globe-real-load").click();
  // The world package alone carries 1,048 tiles (real global coastline
  // coverage) - more than the synthetic fixtures other specs load - so
  // the initial fetch can outrun the default timeout under a loaded
  // parallel test run.
  await expect(stage).toHaveAttribute("data-state", "ready", { timeout: 15_000 });
  await expect(stage).toHaveAttribute("data-shape", "globe");
  await expect(page.getByTestId("readout-world-tiles")).toContainText(/[1-9]\d*/);

  const state = await page.evaluate(() => window.maps2?.debug());
  expect(state?.shape).toBe("globe");
  expect(state?.tiles_drawn).toBeGreaterThan(0);

  // Zoom from the globe into the real city package on the same map
  // instance: addSourceLevels must union, not replace, the pyramid.
  await page.evaluate(() => {
    window.maps2?.setCentre(-0.0877, 51.5133);
    window.maps2?.setZoom(15);
    window.maps2?.render();
  });
  const canvas = stage.locator("canvas");
  await canvas.dispatchEvent("pointerdown", { clientX: 1, clientY: 1 });
  await canvas.dispatchEvent("pointermove", { clientX: 2, clientY: 2 });
  await canvas.dispatchEvent("pointerup", { clientX: 2, clientY: 2 });
  await expect(stage).toHaveAttribute("data-tile-level", "15");
  await expect(page.getByTestId("readout-city-tiles")).toContainText(/[1-9]\d*/);

  const cityState = await page.evaluate(() => window.maps2?.debug());
  expect(cityState?.shape).toBe("flat");
  expect(cityState?.tiles_drawn).toBeGreaterThan(0);

  const p95Ms = await page.evaluate(() => window.maps2?.measureFrames(30) ?? Number.POSITIVE_INFINITY);
  expect(p95Ms).toBeLessThanOrEqual(10);
});

test("globe-real: the world package carries labels and roads, not only relief", async ({ page }) => {
  test.skip(
    !worldPackageRoot || !cityPackageRoot,
    "set MAPS2_WORLD_PACKAGE_ROOT and MAPS2_REAL_PACKAGE_ROOT to run this real-data acceptance test",
  );
  await page.route("https://maps2.local/world/**", (route) => fulfillPrefixed(route, "world", worldPackageRoot!));
  await page.route("https://maps2.local/city/**", (route) => fulfillPrefixed(route, "city", cityPackageRoot!));
  await page.goto("/#/card/globe-real");
  const stage = page.getByTestId("stage");
  await page.getByTestId("globe-real-load").click();
  await expect(stage).toHaveAttribute("data-state", "ready", { timeout: 15_000 });
  const canvas = stage.locator("canvas");

  // Every zoom the world package serves has to read as a map, not as a
  // relief model: before Natural Earth was ingested these levels drew
  // hill shading and a coastline and nothing else at all — no place
  // name, no border, no road anywhere on Earth outside one city.
  for (const zoom of [3, 6, 9]) {
    await page.evaluate((z) => { window.maps2?.setCentre(2.0, 49.5); window.maps2?.setZoom(z); }, zoom);
    for (let attempt = 0; attempt < 14; attempt += 1) {
      await canvas.dispatchEvent("pointerdown", { clientX: 1, clientY: 1 });
      await canvas.dispatchEvent("pointermove", { clientX: 2, clientY: 2 });
      await canvas.dispatchEvent("pointerup", { clientX: 2, clientY: 2 });
      if ((await page.evaluate(() => window.maps2?.missingTiles().length)) === 0) break;
    }
    const placed = await page.evaluate(() => {
      window.maps2?.render();
      return window.maps2?.labelDebug().filter((label) => label.state === "placed").length ?? 0;
    });
    expect(placed, `place names at z${zoom}`).toBeGreaterThan(0);
  }
});

test("globe-real: approaching the city through the prefetch band still renders it", async ({ page }) => {
  test.skip(
    !worldPackageRoot || !cityPackageRoot,
    "set MAPS2_WORLD_PACKAGE_ROOT and MAPS2_REAL_PACKAGE_ROOT to run this real-data acceptance test",
  );
  await page.route("https://maps2.local/world/**", (route) => fulfillPrefixed(route, "world", worldPackageRoot!));
  await page.route("https://maps2.local/city/**", (route) => fulfillPrefixed(route, "city", cityPackageRoot!));
  await page.goto("/#/card/globe-real");
  const stage = page.getByTestId("stage");
  await page.getByTestId("globe-real-load").click();
  await expect(stage).toHaveAttribute("data-state", "ready", { timeout: 15_000 });
  const canvas = stage.locator("canvas");
  const settle = async () => {
    for (let attempt = 0; attempt < 12; attempt += 1) {
      await canvas.dispatchEvent("pointerdown", { clientX: 1, clientY: 1 });
      await canvas.dispatchEvent("pointermove", { clientX: 2, clientY: 2 });
      await canvas.dispatchEvent("pointerup", { clientX: 2, clientY: 2 });
      if ((await page.evaluate(() => window.maps2?.missingTiles().length)) === 0) return;
    }
  };

  // Stopping at 11 first is the point: the city's levels are one step
  // deeper, so they prefetch here. Two packages share one map and
  // `evictableTiles` speaks for all of it, so a loader that unloaded
  // paths it did not own would drop these again while the other loader
  // still believed them resident — and the city would never reappear.
  await page.evaluate(() => { window.maps2?.setCentre(-0.1278, 51.5074); window.maps2?.setZoom(11); });
  await settle();
  await page.evaluate(() => window.maps2?.setZoom(12));
  await settle();

  await expect(stage).toHaveAttribute("data-tile-level", "12");
  const state = await page.evaluate(() => window.maps2?.debug());
  expect(state?.tiles_drawn).toBeGreaterThan(1);
  expect(state?.resident_classes).toContain("Water");
  expect(state?.labels_placed).toBeGreaterThan(0);
});

test("globe-real: street zoom outside the city package still draws the world underneath", async ({ page }) => {
  test.skip(
    !worldPackageRoot || !cityPackageRoot,
    "set MAPS2_WORLD_PACKAGE_ROOT and MAPS2_REAL_PACKAGE_ROOT to run this real-data acceptance test",
  );
  await page.route("https://maps2.local/world/**", (route) => fulfillPrefixed(route, "world", worldPackageRoot!));
  await page.route("https://maps2.local/city/**", (route) => fulfillPrefixed(route, "city", cityPackageRoot!));
  await page.goto("/#/card/globe-real");
  const stage = page.getByTestId("stage");
  await page.getByTestId("globe-real-load").click();
  await expect(stage).toHaveAttribute("data-state", "ready", { timeout: 15_000 });

  // Paris: the city package covers London and nothing else, so the
  // target level (13) has no tile here at all. The reported bug was a
  // blank grey canvas — target_level is global, so it demanded z13
  // everywhere and drew nothing where z13 did not exist. The world
  // package's own coverage has to stand in instead.
  await page.evaluate(() => {
    window.maps2?.setCentre(2.3522, 48.8566);
    window.maps2?.setZoom(13);
    window.maps2?.render();
  });
  const canvas = stage.locator("canvas");
  for (let attempt = 0; attempt < 6; attempt += 1) {
    await canvas.dispatchEvent("pointerdown", { clientX: 1, clientY: 1 });
    await canvas.dispatchEvent("pointermove", { clientX: 2, clientY: 2 });
    await canvas.dispatchEvent("pointerup", { clientX: 2, clientY: 2 });
  }
  await expect(stage).toHaveAttribute("data-tile-level", "13");

  const state = await page.evaluate(() => {
    window.maps2?.render();
    return { debug: window.maps2?.debug(), pixel: window.maps2?.samplePixel(360, 240) };
  });
  expect(state.debug?.tiles_drawn).toBeGreaterThan(0);
  expect(state.debug?.height_tiles).toBeGreaterThan(0);
  // Opaque ground, not the cleared canvas showing through.
  expect(state.pixel?.[3]).toBe(255);
});
