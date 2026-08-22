import { expect, test } from "./helpers/coverage";

test("3d-buildings: tilt changes the terrain-clamped building frame", async ({ page }) => {
  await page.goto("/#/card/buildings-3d");
  const stage = page.getByTestId("stage");
  await expect(stage).toHaveAttribute("data-state", "ready");
  const canvas = page.locator("canvas");
  const flat = await canvas.screenshot();

  await page.getByTestId("building-tilt-slider").fill("45");
  await expect(stage).toHaveAttribute("data-tilt", "45.0");
  const tilted = await canvas.screenshot();

  expect(tilted).not.toBe(flat);
});
