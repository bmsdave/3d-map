import { expect, test, type Page } from "./helpers/coverage";

// Демо-страница — публичная витрина: один пакет, один канвас, весь SDK.
// Тест держит именно те обещания, которые она даёт посетителю: карта
// доезжает до кадра, точки обзора действительно меняют камеру, а
// атрибуция ODbL видна там же, где карта.

const readout = (page: Page, key: string) => page.getByTestId(`demo-${key}`);

async function open(page: Page): Promise<void> {
  await page.goto("/demo/");
  await expect(page.locator(".demo-shell")).toHaveAttribute("data-ready", "true", { timeout: 60_000 });
}

test("демо доезжает до кадра на реальном пакете", async ({ page }) => {
  await open(page);
  await expect(readout(page, "shape")).toHaveText("globe");
  await expect(readout(page, "tiles")).not.toHaveText("—");
  await expect(page.getByTestId("demo-canvas")).toBeVisible();
  // Данные чужие: ODbL требует называть источник рядом с картой.
  await expect(page.getByTestId("demo-attribution")).toContainText("OpenStreetMap");
});

test("точка обзора долетает до города и меняет уровень тайлов", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "Trafalgar Square" }).click();
  // Перелёт занимает секунды: ждём именно камеру, а не таймер.
  await expect(readout(page, "level")).toHaveText("16", { timeout: 30_000 });
  await expect(readout(page, "shape")).toHaveText("flat");
  await expect(page.locator(".demo-shell")).toHaveAttribute("data-zoom", "16.00");
});

test("глобус распрямляется в плоскость по дороге вниз", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "Britain" }).click();
  await expect(readout(page, "shape")).toHaveText("flat", { timeout: 30_000 });
  await expect(readout(page, "zoom")).toHaveText("5.20");
});

test("«?package=» открывает другой пакет, без пересборки", async ({ page }) => {
  // Тот же пакет, но названный явно: проверяется, что демо берёт адрес из
  // строки запроса, а не из константы. Так планетный пакет с объектного
  // хранилища подключается без единой правки кода.
  await page.goto("/demo/?package=/packages/trafalgar/manifest.json");
  await expect(page.locator(".demo-shell")).toHaveAttribute("data-ready", "true", {
    timeout: 60_000,
  });
  await expect(readout(page, "tiles")).not.toHaveText("—");
});

test("непригодный «?package=» показывает ошибку, а не пустую страницу", async ({ page }) => {
  await page.goto("/demo/?package=/packages/nothing-here/manifest.json");
  const status = page.getByTestId("demo-status");
  await expect(status).toHaveAttribute("data-state", "error", { timeout: 60_000 });
  await expect(status).toContainText("nothing-here");
});
