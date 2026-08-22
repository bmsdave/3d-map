import { expect, test } from "./helpers/coverage";

test("sdk-business: clamp and levelBox fallback", async ({ page }) => {
  await page.goto("/");
  const r = await page.evaluate(async () => {
    const { createPackageMap, levelBox } = await import("/src/sdk.ts");
    const m = await fetch("/packages/trafalgar/manifest.json").then((x) => x.json());
    const a = levelBox(m, 16);
    const b = levelBox(m, 18);
    const c = levelBox({ format: "MT2", format_version: 5, tiles: [], tile_digests: {}, view: { lon: 0, lat: 0, zoom: 0 }, sources: [] } as never, 5);
    const canvas = document.createElement("canvas");
    canvas.width = 720; canvas.height = 480;
    document.body.appendChild(canvas);
    const { map } = await createPackageMap(canvas, { zoom: 16, centre: { lon: 0, lat: 0 } });
    map.setCentre(0, 0);
    await new Promise((done) => setTimeout(done, 900));
    const s = map.debug();
    const box = b!;
    const eps = 1e-5;
    const clamped = s.centre_lon >= box.west - eps && s.centre_lon <= box.east + eps && s.centre_lat >= box.south - eps && s.centre_lat <= box.north + eps;
    const drew = s.tiles_drawn > 0;
    document.body.removeChild(canvas);
    return { fallback: JSON.stringify(a) === JSON.stringify(b), nullOk: c === null, clamped, drew };
  });
  expect(r.fallback).toBe(true);
  expect(r.nullOk).toBe(true);
  expect(r.clamped).toBe(true);
  expect(r.drew).toBe(true);
});

test("sdk-business: hasTooManyTiles rejects oversized manifest", async ({ page }) => {
  const tiles = Array.from({ length: 50001 }, () => "0/0/0.mt2");
  await page.route("**/oversized-manifest.json", (route) => route.fulfill({
    contentType: "application/json",
    body: JSON.stringify({ format: "MT2", format_version: 5, tiles, tile_digests: Object.fromEntries(tiles.map((p) => [p, "0".repeat(64)])), view: { lon: -0.1281, lat: 51.508, zoom: 16 }, sources: [] }),
  }));
  await page.goto("/#/card/package-loader");
  await page.getByTestId("package-manifest-url").fill("/oversized-manifest.json");
  await page.getByRole("button", { name: "Загрузить пакет" }).click();
  await expect(page.getByTestId("stage")).toContainText("exceeds 50000");
});

test("sdk-business: tile retry on 429 succeeds", async ({ page }) => {
  let first = true;
  await page.route("**/*.mt2", async (route) => {
    if (first) { first = false; await route.fulfill({ status: 429, body: "retry" }); return; }
    await route.continue();
  });
  await page.goto("/#/card/package-loader");
  await expect(page.getByTestId("stage")).toHaveAttribute("data-state", "ready");
  expect(first).toBe(false);
});
