import { FIXED_CENTRE } from "../bands";
import { overlapCount } from "../labels";
import { createPackageMap } from "../sdk";
import { controlRow, el, readout, section } from "../ui";
import type { CardSpec } from "./types";

// Бюджет — не число объектов, а площадь экрана: одна крупная подпись
// съедает столько же, сколько пяток мелких POI. Ползунок двигает долю
// экрана, отданную подписям; тысяча с лишним кандидатов конкурирует за
// неё по рангу.

export const poiDensity: CardSpec = {
  id: "poi-density",
  title: "Плотность POI и бюджет экрана",
  purpose:
    "Тысяча синтетических POI на z16 и ползунок бюджета занятости. Проверяет: занятая доля экрана не превышает бюджет, меньший бюджет оставляет подмножество большего, отбор идёт по рангу.",
  group: "Подписи",
  mount(stage, panel) {
    const out = readout([
      { key: "candidates", label: "кандидатов (SDK)" },
      { key: "placed", label: "размещено (SDK)" },
      { key: "budget", label: "бюджет (SDK)" },
      { key: "occupancy", label: "занято экрана (SDK)" },
      { key: "worst-rank", label: "худший размещённый ранг" },
      { key: "overlaps", label: "пересечений боксов" },
    ]);

    const budget = el("input", {
      type: "range",
      min: "0",
      max: "30",
      step: "0.5",
      value: "8",
      "data-testid": "budget-slider",
    });

    let apply = () => {};
    const canvas = el("canvas", { width: "720", height: "480" });
    stage.replaceChildren(canvas);
    stage.setAttribute("data-centre", `${FIXED_CENTRE.lon},${FIXED_CENTRE.lat}`);

    createPackageMap(canvas, { zoom: 16 })
      .then(({ map, onSettled }) => {
        map.setCentre(FIXED_CENTRE.lon, FIXED_CENTRE.lat);
        map.setZoom(16);
        apply = () => {
          const share = Number(budget.value) / 100;
          map.setLabelBudget(share);
          map.render();
          const state = map.debug();
          const entries = map.labelDebug();
          const ranks = entries.filter((e) => e.state === "placed").map((e) => e.rank);
          out.set("candidates", String(state.label_candidates));
          out.set("placed", String(state.labels_placed));
          out.set("budget", `${(state.label_budget * 100).toFixed(1)} %`);
          out.set("occupancy", `${(state.label_occupancy * 100).toFixed(1)} %`);
          out.set("worst-rank", ranks.length > 0 ? String(Math.max(...ranks)) : "—");
          out.set("overlaps", String(overlapCount(entries)));
          stage.setAttribute("data-budget", share.toFixed(3));
          stage.setAttribute("data-placed", String(state.labels_placed));
        };
        // Показания снимаются с кадра, а тайлы под этой камерой
        // доезжают позже её движения.
        onSettled(apply);
        stage.setAttribute("data-state", "ready");
        apply();
      })
      .catch((error: unknown) => {
        stage.setAttribute("data-state", "error");
        stage.replaceChildren(String(error));
      });

    budget.addEventListener("input", () => apply());

    panel.append(
      section("Отбор", controlRow("бюджет экрана, %", budget)),
      section("Показания", out.root),
    );
  },
};
