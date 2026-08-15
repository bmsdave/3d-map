import { expect, test, type Page } from "@playwright/test";

const RENDERING_CARDS = [
  "zoom-bands",
  "roads-micro",
  "labels-collision",
  "terrain-shade",
  "globe-relief",
] as const;

async function measureFrames(page: Page, id: string): Promise<number> {
  await page.goto(`/#/card/${id}`);
  await expect(page.getByTestId("stage")).toHaveAttribute("data-state", "ready");
  return page.evaluate(() => window.maps2?.measureFrames(30) ?? Number.POSITIVE_INFINITY);
}

test("rendering cards keep the p95 frame time within 10 ms", async ({ page }) => {
  for (const id of RENDERING_CARDS) {
    const p95Ms = await measureFrames(page, id);
    expect(p95Ms, `${id} p95 frame time`).toBeLessThanOrEqual(10);
  }
});
