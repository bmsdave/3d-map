import { expect, test } from "@playwright/test";

test("showcase presents twenty live animated SDK scenes", async ({ page }) => {
  await page.goto("/#/showcase");
  const showcase = page.getByTestId("showcase");
  await expect(showcase).toHaveAttribute("data-playing", "true");
  await expect(page.getByTestId("showcase-demo")).toHaveCount(20);
  await expect(page.getByTestId("showcase-demo").first()).toHaveAttribute("data-state", "ready");

  await page.getByTestId("showcase-toggle").click();
  await expect(showcase).toHaveAttribute("data-playing", "false");
});
