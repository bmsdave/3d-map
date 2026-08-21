import { FIXED_CENTRE } from "../bands";
import { churn, lonPerPixel } from "../labels";
import { createPackageMap } from "../sdk";
import { controlRow, el, readout, section } from "../ui";
import type { CardSpec } from "./types";

// Сдвиг камеры на несколько пикселей законно меняет набор подписей —
// так ведёт себя и Google Maps. Незаконно — менять его целиком.
// Карточка меряет ровно эту долю: сдвигает центр на N экранных
// пикселей и сверяет два кадра по id размещённых.

const LIMIT = 0.1;

export const viewportStability: CardSpec = {
  id: "viewport-stability",
  title: "Устойчивость к сдвигу вьюпорта",
  purpose:
    "Сдвиг камеры на 1–20 px и доля переразмещённых подписей. Проверяет: набор меняется только по краям — переразмещено меньше десятой части, и кадр воспроизводится при возврате камеры.",
  group: "Подписи",
  mount(stage, panel) {
    const out = readout([
      { key: "shift", label: "сдвиг, px" },
      { key: "before", label: "размещено до" },
      { key: "after", label: "размещено после" },
      { key: "churn", label: "переразмещено" },
      { key: "limit", label: "порог" },
      { key: "deterministic", label: "кадр воспроизводится" },
    ]);

    const shift = el("input", {
      type: "range",
      min: "1",
      max: "20",
      step: "1",
      value: "8",
      "data-testid": "shift-slider",
    });

    let apply = () => {};
    const canvas = el("canvas", { width: "720", height: "480" });
    stage.replaceChildren(canvas);
    stage.setAttribute("data-centre", `${FIXED_CENTRE.lon},${FIXED_CENTRE.lat}`);

    createPackageMap(canvas, { zoom: 16 })
      .then(({ map, onSettled }) => {
        const zoom = 16;
        map.setZoom(zoom);
        map.setLabelBudget(0.12);
        const frameAt = (lon: number) => {
          map.setCentre(lon, FIXED_CENTRE.lat);
          map.render();
          return map.labelDebug();
        };
        apply = () => {
          const pixels = Number(shift.value);
          const before = frameAt(FIXED_CENTRE.lon);
          const after = frameAt(FIXED_CENTRE.lon + pixels * lonPerPixel(zoom));
          const again = frameAt(FIXED_CENTRE.lon);
          const moved = churn(before, after);
          const same = churn(before, again) === 0;
          out.set("shift", String(pixels));
          out.set("before", String(before.filter((e) => e.state === "placed").length));
          out.set("after", String(after.filter((e) => e.state === "placed").length));
          out.set("churn", `${(moved * 100).toFixed(1)} %`);
          out.set("limit", `${LIMIT * 100} %`);
          out.set("deterministic", same ? "да" : "нет");
          stage.setAttribute("data-shift", String(pixels));
          stage.setAttribute("data-churn", moved.toFixed(4));
          stage.setAttribute("data-deterministic", String(same));
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

    shift.addEventListener("input", () => apply());

    panel.append(
      section("Камера", controlRow("сдвиг, px", shift)),
      section("Показания", out.root),
    );
  },
};
