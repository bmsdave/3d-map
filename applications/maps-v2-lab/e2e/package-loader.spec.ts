import { expect, test } from "@playwright/test";

test("package loader: a manifest drives demand-loaded MT2 tiles", async ({ page }) => {
  await page.goto("/#/card/package-loader");
  const stage = page.getByTestId("stage");

  await expect(stage).toHaveAttribute("data-state", "ready");
  await expect(stage).toHaveAttribute("data-loaded", /[1-9]\d*/);
  await expect(page.getByTestId("readout-package-tiles")).toContainText(/[1-9]\d*/);
  await expect(page.getByTestId("readout-package-level")).toHaveText("12");
});
