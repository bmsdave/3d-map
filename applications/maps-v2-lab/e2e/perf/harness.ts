// Общая механика замеров. Тест сам ничего не измеряет: он открывает
// студию, выполняет сценарий и читает то, что записал хост.
//
// Меряется главным образом одно — худший одиночный кусок работы на main
// thread. Слайдер отстаёт от пальца не от суммы за секунду, а от одного
// длинного вызова посреди неё.

import { expect, type Locator, type Page } from "@playwright/test";

import type { Interaction } from "./studies";

/** Отзывчивость: ни один кусок работы не держит поток дольше. */
export const BLOCK_BUDGET_MS = 10;

/** Устоявшийся кадр: p95 рендера на неподвижной камере. */
export const FRAME_BUDGET_MS = 10;

/// Сколько тишины считать концом взаимодействия. Догрузка идёт
/// пачками с паузами на декодирование; полсекунды без единого спана —
/// это уже не пауза внутри прохода, а его конец.
const QUIET_MS = 400;
/// Потолок ожидания, а не срок: студия, которая всё ещё что-то делает
/// через десять секунд после щелчка, уже сказала о себе всё, что
/// требовалось, и держать прогон дальше незачем.
const SETTLE_TIMEOUT_MS = 10_000;

export interface Measurement {
  /** Время от перехода до первого настоящего кадра.
   *
   *  Справка, не бюджет, и грубая: в него попадает свёртывание
   *  предыдущей студии тем же браузером. Замеряемые бюджеты от этого
   *  не страдают — трасса обнуляется уже после того, как студия
   *  открылась и успокоилась. */
  readyMs?: number;
  /** Худший одиночный блокирующий вызов и его фаза. */
  worstBlockMs: number;
  worstPhase: string | null;
  /** Худший долгий кадр по данным браузера; 0 — крупных не было. */
  worstLongFrameMs: number;
  /** Стена от начала сценария до тишины. Не бюджет, а справка. */
  wallMs: number;
  /** Планы резидентности на кадр: больше одного — двойная работа. */
  plansPerFrame: number;
  heldTiles: number;
  heldBytes: number;
  /** Суммарно по фазам: count / total / max / p95. */
  phases: Record<string, { count: number; totalMs: number; maxMs: number; p95Ms: number }>;
}

/// Ждать первого кадра приходится долго и это само по себе показание.
/// Студия, открывающаяся полминуты, — находка, а не сбой стенда, и
/// двадцатисекундный потолок превращал её в невнятную ошибку локатора
/// вместо числа.
const OPEN_TIMEOUT_MS = 90_000;

/** Открывает студию и возвращает время до первого настоящего кадра. */
export async function openStudy(page: Page, id: string): Promise<number> {
  // `?perf=1` до решётки: роутер живёт в хэше, а трасса читает search.
  const started = Date.now();
  await page.goto(`/?perf=1#/card/${id}`);
  await expect(page.getByTestId("stage")).toHaveAttribute("data-state", "ready", {
    timeout: OPEN_TIMEOUT_MS,
  });
  const readyMs = Date.now() - started;
  await settle(page);
  return readyMs;
}

/** Ждёт, пока хост перестанет что-либо делать, и обнуляет трассу. */
export async function armed(page: Page): Promise<void> {
  await settle(page);
  await page.evaluate(() => window.__maps2Perf?.reset());
}

async function settle(page: Page): Promise<void> {
  const deadline = Date.now() + SETTLE_TIMEOUT_MS;
  let seen = -1;
  while (Date.now() < deadline) {
    const count = await page.evaluate(() => window.__maps2Perf?.spans().length ?? 0);
    if (count === seen) return;
    seen = count;
    await page.waitForTimeout(QUIET_MS);
  }
}

/**
 * Выполняет сценарий и возвращает то, что он стоил.
 *
 * Показания снимаются до `debug()`, которым берутся планы и байты:
 * иначе замер хоста попал бы в собственную статистику и каждая
 * студия получила бы лишний спан фазы `debug` от самого теста.
 */
export async function measure(page: Page, action: Interaction): Promise<Measurement> {
  await armed(page);
  const started = Date.now();
  await perform(page, action);
  await settle(page);
  const wallMs = Date.now() - started;

  const trace = await page.evaluate(() => {
    const perf = window.__maps2Perf!;
    const worst = perf.worstBlock();
    return {
      worstBlockMs: worst.ms,
      worstPhase: worst.phase,
      worstLongFrameMs: perf.worstLongFrameMs(),
      phases: perf.summary(),
    };
  });
  const state = await page.evaluate(() => {
    const debug = window.maps2!.debug();
    return { plans: debug.plans, tiles: debug.cpu_tiles, bytes: debug.cpu_bytes };
  });

  return {
    ...trace,
    wallMs,
    plansPerFrame: state.plans,
    heldTiles: state.tiles,
    heldBytes: state.bytes,
  };
}

async function perform(page: Page, action: Interaction): Promise<void> {
  switch (action.kind) {
    case "range":
    case "text": {
      const control = page.getByTestId(action.testId);
      for (const value of action.values) await control.fill(value);
      return;
    }
    case "select": {
      const control = page.getByTestId(action.testId);
      // Пустой список значит «все варианты этого select»: у шести
      // тоглов полос они свои, и выписывать их в реестр значило бы
      // держать имена полос в двух местах.
      const values = action.values.length > 0
        ? action.values
        : await control.locator("option").evaluateAll((options) =>
            options.map((option) => (option as HTMLOptionElement).value));
      for (const value of values) await control.selectOption(value);
      return;
    }
    case "toggle": {
      const control = page.getByTestId(action.testId);
      for (let flip = 0; flip < 4; flip += 1) {
        if (await control.isChecked()) await control.uncheck();
        else await control.check();
      }
      return;
    }
    case "wheel": {
      const canvas = await centreOf(page);
      await page.mouse.move(canvas.x, canvas.y);
      for (let step = 0; step < action.steps; step += 1) {
        await page.mouse.wheel(0, step % 2 === 0 ? -240 : 240);
        await page.waitForTimeout(60);
      }
      return;
    }
    case "drag": {
      const canvas = await centreOf(page);
      await page.mouse.move(canvas.x, canvas.y);
      await page.mouse.down();
      // Драг — это поток событий, а не один прыжок: карта, которую
      // двигают одним переносом, никогда не покажет, как она себя
      // ведёт под настоящим пальцем.
      for (let step = 1; step <= 8; step += 1) {
        await page.mouse.move(
          canvas.x + (action.dx * step) / 8,
          canvas.y + (action.dy * step) / 8,
        );
      }
      await page.mouse.up();
      return;
    }
  }
}

async function centreOf(page: Page): Promise<{ x: number; y: number }> {
  const canvas: Locator = page.getByTestId("stage").locator("canvas");
  const box = await canvas.boundingBox();
  if (!box) throw new Error("сцена без холста");
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}

/**
 * p95 рендера на неподвижной камере — самый тёплый путь, какой есть:
 * ничего не планируется, ничего не грузится, кадр рисуется тридцать
 * раз подряд. Отдельно от `measure`, потому что отвечает на другой
 * вопрос: сколько стоит кадр сам по себе, без движения.
 */
export async function frameMeasurement(page: Page, readyMs: number): Promise<Measurement> {
  const p95 = await page.evaluate(() =>
    window.maps2?.measureFrames(30) ?? Number.POSITIVE_INFINITY);
  const state = await page.evaluate(() => {
    const debug = window.maps2!.debug();
    return { plans: debug.plans, tiles: debug.cpu_tiles, bytes: debug.cpu_bytes };
  });
  const phases = await page.evaluate(() => window.__maps2Perf!.summary());
  return {
    readyMs,
    worstBlockMs: p95,
    worstPhase: "render",
    worstLongFrameMs: 0,
    wallMs: 0,
    plansPerFrame: state.plans,
    heldTiles: state.tiles,
    heldBytes: state.bytes,
    phases,
  };
}

/** Снимок трассы как есть: для страниц, которые не студии. */
export async function traceMeasurement(page: Page, readyMs?: number): Promise<Measurement> {
  const trace = await page.evaluate(() => {
    const perf = window.__maps2Perf!;
    const block = perf.worstBlock();
    return {
      worstBlockMs: block.ms,
      worstPhase: block.phase,
      worstLongFrameMs: perf.worstLongFrameMs(),
      phases: perf.summary(),
    };
  });
  const state = await page.evaluate(() => {
    const map = window.maps2;
    if (!map) return { plans: 0, tiles: 0, bytes: 0 };
    const debug = map.debug();
    return { plans: debug.plans, tiles: debug.cpu_tiles, bytes: debug.cpu_bytes };
  });
  return {
    readyMs,
    ...trace,
    wallMs: 0,
    plansPerFrame: state.plans,
    heldTiles: state.tiles,
    heldBytes: state.bytes,
  };
}

/**
 * Прикрепляет числа к тесту. Репортёр печатает их только там, где они
 * нужны, — в строке провала; всё остальное уходит в JSON.
 */
export function attachMeasurement(
  testInfo: { attach(name: string, options: { body: string; contentType: string }): Promise<void> },
  study: string,
  scenario: string,
  measurement: Measurement,
): Promise<void> {
  return testInfo.attach("maps2-perf", {
    body: JSON.stringify({ study, scenario, ...measurement }),
    contentType: "application/json",
  });
}
