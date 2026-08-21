// Бюджет отзывчивости на всех двадцати студиях. Тесты не написаны по
// одному на карточку, а порождены из реестра: карточка, добавленная без
// строки в studies.ts, останется без замеров, и это видно сразу — счёт
// студий здесь сверяется с реестром самой лабы.

import { expect, test } from "@playwright/test";

import { CARDS } from "../../src/cards";
import {
  attachMeasurement,
  BLOCK_BUDGET_MS,
  FRAME_BUDGET_MS,
  frameMeasurement,
  measure,
  openStudy,
} from "./harness";
import { STUDIES } from "./studies";

test("реестр замеров покрывает все студии лаборатории", () => {
  const measured = new Set(STUDIES.map((study) => study.id));
  const missing = CARDS.map((card) => card.id).filter((id) => !measured.has(id));
  expect(missing, "студии без замеров производительности").toEqual([]);
});

for (const study of STUDIES) {
  test.describe(study.id, () => {
    test("устоявшийся кадр укладывается в бюджет", async ({ page }, testInfo) => {
      const readyMs = await openStudy(page, study.id);
      const measurement = await frameMeasurement(page, readyMs);
      await attachMeasurement(testInfo, study.id, "frame", measurement);
      expect(measurement.worstBlockMs, `${study.id} p95 кадра`)
        .toBeLessThanOrEqual(FRAME_BUDGET_MS);
    });

    test("главное взаимодействие не блокирует поток", async ({ page }, testInfo) => {
      const readyMs = await openStudy(page, study.id);
      const measurement = await measure(page, study.primary);
      await attachMeasurement(testInfo, study.id, "primary", { ...measurement, readyMs });
      expect(
        measurement.worstBlockMs,
        `${study.id} худший блок (${measurement.worstPhase})`,
      ).toBeLessThanOrEqual(BLOCK_BUDGET_MS);
    });

    test("второе взаимодействие не блокирует поток", async ({ page }, testInfo) => {
      const readyMs = await openStudy(page, study.id);
      const measurement = await measure(page, study.secondary);
      await attachMeasurement(testInfo, study.id, "secondary", { ...measurement, readyMs });
      expect(
        measurement.worstBlockMs,
        `${study.id} худший блок (${measurement.worstPhase})`,
      ).toBeLessThanOrEqual(BLOCK_BUDGET_MS);
    });

    test("повтор того же не копит работу", async ({ page }, testInfo) => {
      const readyMs = await openStudy(page, study.id);
      // Первый проход прогревает: тайлы под этой камерой уже приехали.
      const first = await measure(page, study.secondary);
      const second = await measure(page, study.secondary);
      await attachMeasurement(testInfo, study.id, "repeat", { ...second, readyMs });

      // Один план резидентности на кадр. Больше — значит один и тот же
      // вопрос задан дважды за кадр, и это ровно та работа, которой
      // никто не заказывал.
      expect(second.plansPerFrame, `${study.id} планов на кадр`).toBeLessThanOrEqual(1);

      // Повтор того же движения по прогретой карте не должен ничего
      // добавлять: рост здесь — это удержание тайлов, которые никто
      // больше не покажет.
      expect(
        second.heldBytes,
        `${study.id} держит байтов после повтора (было ${first.heldBytes})`,
      ).toBeLessThanOrEqual(Math.max(first.heldBytes, 1) * 1.05);
    });
  });
}
