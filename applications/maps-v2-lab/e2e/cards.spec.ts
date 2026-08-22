// e2e по прямым ссылкам карточек. Читает DOM и data-атрибуты, не
// пиксели. Утверждения формулируются через состав и показания, а не
// «объект X виден» — по правилу роадмапа о видимости как свойстве кадра.

import { expect, test, type Page } from "./helpers/coverage";

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
  // Уровень тайлов — это зум камеры, а не то, что нашлось в пакете:
  // у фикстуры на z6 покрытия не было и рендерер откатывался на пятый.
  await expect(page.getByTestId("readout-tile-level")).toHaveText("6");
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

test("главная показывает живые студии сразу, без перехода по ссылке", async ({ page }) => {
  await page.goto("/#/");
  const quickStart = page.getByTestId("quick-start");
  await expect(quickStart).toBeVisible();
  await expect(quickStart.locator("code")).toContainText("createPackageMap");
  await expect(quickStart.locator("code")).toContainText("Trafalgar Square");

  // Герой рисует до любого клика — страница открывается уже картой.
  await expect(page.getByTestId("hero-stage")).toHaveAttribute("data-state", "ready");

  const studies = page.getByTestId("study");
  await expect(studies).toHaveCount(20);
  // Первые студии смонтированы сами; остальные ждут своей очереди за
  // бюджетом контекстов, а не клика.
  const first = studies.first();
  await expect(first.getByTestId("stage")).toHaveAttribute("data-state", "ready");
  await expect(first).toHaveAttribute("data-live", "true");
  const live = Number(await page.getByTestId("live-count").textContent());
  expect(live).toBeGreaterThan(0);
  expect(live).toBeLessThanOrEqual(Number(await page.getByTestId("home").getAttribute("data-live-budget")));

  // Фильтр — единственный способ спрятать студию с доски.
  const shown = page.locator('[data-testid="study"]:not([hidden])');
  await page.getByTestId("study-filter").fill("roads-micro");
  await expect(shown).toHaveCount(1);
  await expect(shown).toHaveAttribute("data-card", "roads-micro");
});

test("студия открывается отдельной страницей по своей ссылке", async ({ page }) => {
  await page.goto("/#/");
  const first = page.getByTestId("study").first();
  // Дождаться, пока доска отдаст свои контексты, а не пересекаться с ней
  // за GPU в момент перехода.
  await expect(first).toHaveAttribute("data-live", "true");
  const open = first.locator("a.study-open");
  const href = await open.getAttribute("href");
  await open.click();
  await expect(page).toHaveURL(new RegExp(`${href}$`));
  // Сцену ищем внутри страницы карточки, а не по всей странице. URL меняется
  // сразу по клику, а перерисовка приходит следующим тактом, на hashchange —
  // и в этом окне на странице ещё доска, где сцен двадцать. Локатор без рамки
  // ловит их все и падает на strict mode, а это не «ещё не готово», это
  // ошибка: ждать Playwright после неё уже не станет. Рамка совпадает с нулём
  // элементов, пока доска не сменилась, и потому честно ждёт.
  const stage = page.locator("main.card-page").getByTestId("stage");
  await expect(stage).toHaveAttribute("data-state", "ready");
});
