import { expect, test } from "@playwright/test";

test("package loader: a manifest drives demand-loaded MT2 tiles", async ({ page }) => {
  await page.goto("/#/card/package-loader");
  const stage = page.getByTestId("stage");

  await expect(stage).toHaveAttribute("data-state", "ready");
  await expect(stage).toHaveAttribute("data-loaded", /[1-9]\d*/);
  await expect(page.getByTestId("readout-package-tiles")).toContainText(/[1-9]\d*/);
  await expect(page.getByTestId("readout-package-level")).toHaveText("12");
});

test("package loader: rejects a tile whose bytes do not match the manifest", async ({ page }) => {
  const validOtherTile = new URL("../public/fixtures/ealing/0/0/0.mt2", import.meta.url).pathname;
  await page.route("**/fixtures/ealing/**/*.mt2", (route) => route.fulfill({ path: validOtherTile }));
  await page.goto("/#/card/package-loader");

  await expect(page.getByTestId("stage")).toHaveAttribute("data-state", "error");
});
