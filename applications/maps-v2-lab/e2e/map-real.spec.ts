import { expect, test, type Route } from "@playwright/test";
import { join } from "node:path";

const mapPackageRoot = process.env.MAPS2_MAP_PACKAGE_ROOT;

function localPackagePath(root: string, pathname: string): string {
  const relative = pathname.replace(/^\//, "");
  if (relative === "manifest.json" || /^\d+\/\d+\/\d+\.mt2$/.test(relative)) return join(root, relative);
  throw new Error(`unexpected package path: ${relative}`);
}

async function serveMap(route: Route): Promise<void> {
  const pathname = new URL(route.request().url()).pathname;
  await route.fulfill({
    path: localPackagePath(mapPackageRoot!, pathname.replace(/^\/map\//, "")),
    headers: { "Access-Control-Allow-Origin": "*" },
  });
}

test.describe("map-real", () => {
  test.beforeEach(async ({ page }) => {
    test.skip(!mapPackageRoot, "set MAPS2_MAP_PACKAGE_ROOT to run this real-data acceptance test");
    await page.route("https://maps2.local/map/**", serveMap);
  });

  async function open(page: import("@playwright/test").Page) {
    await page.goto("/#/card/map-real");
    const stage = page.getByTestId("stage");
    await expect(stage).toHaveAttribute("data-state", "idle");
    await page.getByTestId("map-real-load").click();
    await expect(stage).toHaveAttribute("data-state", "ready", { timeout: 20_000 });
    return stage;
  }

  async function settle(page: import("@playwright/test").Page, canvas: ReturnType<typeof page.locator>) {
    for (let attempt = 0; attempt < 16; attempt += 1) {
      await canvas.dispatchEvent("pointerdown", { clientX: 1, clientY: 1 });
      await canvas.dispatchEvent("pointermove", { clientX: 2, clientY: 2 });
      await canvas.dispatchEvent("pointerup", { clientX: 2, clientY: 2 });
      if ((await page.evaluate(() => window.maps2?.missingTiles().length)) === 0) return;
    }
  }

  test("one package answers at every zoom from globe to street", async ({ page }) => {
    const stage = await open(page);
    const canvas = stage.locator("canvas");
    // The two-package build had no tiles at all between z8 and z11:
    // neither package claimed those levels. One builder owns the whole
    // pyramid, so every level has an answer.
    for (const zoom of [3, 6, 8, 10, 11, 12, 14, 16]) {
      await page.evaluate((z) => { window.maps2?.setCentre(-0.1278, 51.5074); window.maps2?.setZoom(z); }, zoom);
      await settle(page, canvas);
      const state = await page.evaluate(() => { window.maps2?.render(); return window.maps2?.debug(); });
      expect(state?.tiles_drawn, `tiles drawn at z${zoom}`).toBeGreaterThan(0);
    }
  });

  test("a city carries one label, not one per source", async ({ page }) => {
    const stage = await open(page);
    const canvas = stage.locator("canvas");
    // Natural Earth's London and OSM's London sit about a kilometre
    // apart. Merged, both arrive; conflated, the stronger source wins
    // and the city is named once.
    await page.evaluate(() => { window.maps2?.setCentre(-0.1278, 51.5074); window.maps2?.setZoom(12); });
    await settle(page, canvas);
    const londons = await page.evaluate(() => {
      window.maps2?.render();
      return (window.maps2?.labelDebug() ?? [])
        .filter((label) => label.state === "placed" && label.text === "London").length;
    });
    expect(londons).toBeLessThanOrEqual(1);
  });

  test("the package declares one continuous pyramid", async ({ page }) => {
    await open(page);
    const levels = await page.evaluate(async () => {
      const manifest = await (await fetch("https://maps2.local/map/manifest.json")).json();
      return [...new Set(manifest.tiles.map((path: string) => Number(path.split("/")[0])))]
        .sort((a, b) => (a as number) - (b as number));
    });
    const expected = Array.from({ length: (levels.at(-1) as number) - (levels[0] as number) + 1 },
      (_, index) => (levels[0] as number) + index);
    expect(levels).toEqual(expected);
  });
});
