import { expect, test } from "./helpers/coverage";

test("showcase presents twenty animated studies in GPU-safe reels", async ({ page }) => {
  await page.goto("/#/showcase");
  const showcase = page.getByTestId("showcase");
  await expect(showcase).toHaveAttribute("data-playing", "true");
  await expect(showcase).toHaveAttribute("data-demo-count", "20");
  await expect(page.getByTestId("showcase-demo")).toHaveCount(4);
  await expect(page.getByTestId("showcase-demo").first()).toHaveAttribute("data-state", "ready");

  await page.getByTestId("showcase-next").click();
  await expect(showcase).toHaveAttribute("data-reel", "2");
  await expect(page.getByTestId("showcase-demo").first()).toContainText("Long shadow");

  await page.getByTestId("showcase-toggle").click();
  await expect(showcase).toHaveAttribute("data-playing", "false");
});
