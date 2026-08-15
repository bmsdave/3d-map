// e2e по прямым ссылкам карточек. Читает DOM и data-атрибуты, не
// пиксели. Утверждения формулируются через состав и показания, а не
// «объект X виден» — по правилу роадмапа о видимости как свойстве кадра.

import { expect, test, type Page } from "@playwright/test";

async function openCard(page: Page, id: string): Promise<void> {
  await page.goto(`/#/card/${id}`);
  await expect(page.getByTestId("stage")).toHaveAttribute("data-state", "ready");
}

function classes(page: Page) {
  return page.getByTestId("readout-classes");
}

test("zoom-bands: полосы приходят и уходят с зумом", async ({ page }) => {
  await openCard(page, "zoom-bands");
  const slider = page.getByTestId("zoom-slider");

  await slider.fill("16.8");
  await expect(classes(page)).toContainText("Building");
  await expect(page.getByTestId("readout-band-actual")).toHaveText("Micro");

  // Регрессия v1: после отъезда дома обязаны уйти с экрана.
  await slider.fill("6");
  await expect(classes(page)).not.toContainText("Building");
  await expect(page.getByTestId("readout-tile-level")).toHaveText("5");
});

test("тогл Address↔Micro меняет состав при неподвижной камере", async ({ page }) => {
  await openCard(page, "toggle-address-micro");
  const stage = page.getByTestId("stage");
  const toggle = page.getByTestId("composition-toggle");

  await expect(stage).toHaveAttribute("data-zoom", "16.00");
  await expect(stage).toHaveAttribute("data-composition", "Address");
  await expect(classes(page)).not.toContainText("Building");

  await toggle.selectOption("Micro");
  await expect(stage).toHaveAttribute("data-composition", "Micro");
  await expect(classes(page)).toContainText("Building");

  await toggle.selectOption("Address");
  await expect(classes(page)).not.toContainText("Building");
  // Камера не шевелилась.
  await expect(stage).toHaveAttribute("data-zoom", "16.00");
});

test("тогл Region↔City двигает парк, камера на месте", async ({ page }) => {
  await openCard(page, "toggle-region-city");
  const toggle = page.getByTestId("composition-toggle");

  await expect(classes(page)).not.toContainText("Park");
  await toggle.selectOption("City");
  await expect(classes(page)).toContainText("Park");
});

test("globe-transition: globeness гаснет на 3.5–4.5", async ({ page }) => {
  await openCard(page, "globe-transition");
  const slider = page.getByTestId("globeness-slider");
  const globeness = page.getByTestId("readout-globeness");

  await slider.fill("3");
  await expect(globeness).toHaveText("1.000");
  await slider.fill("4");
  await expect(globeness).toHaveText("0.500");
  await slider.fill("5");
  await expect(globeness).toHaveText("0.000");
});

test("индекс открывает карточку по прямой ссылке", async ({ page }) => {
  await page.goto("/#/card/toggle-district-street");
  await expect(page.locator("main")).toHaveAttribute("data-card", "toggle-district-street");
});
