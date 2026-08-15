import { FIXED_CENTRE } from "../bands";
import { createMap } from "../sdk";
import { controlRow, el, PENDING, readout, section } from "../ui";
import type { CardSpec } from "./types";

// Переход глобус↔плоскость: слайдер z3–z5 поперёк интервала гашения
// Globeness (3.5–4.5). Число — правда камеры (по нему уже есть
// Rust-тесты: обе формы кладут одну точку под центр); отрисовка сферы
// в рендере — отдельная задача после заморозки формата, до неё readout
// формы честно висит в pending.

export const globeTransition: CardSpec = {
  id: "globe-transition",
  title: "Глобус ↔ плоскость",
  purpose:
    "Камера над одной точкой, слайдер z3–z5. Проверяет: globeness гаснет на 3.5–4.5, переход не двигает мир (математика уже под тестами; сфера в рендере — позже).",
  group: "Глобус",
  mount(stage, panel) {
    const out = readout([
      { key: "zoom", label: "зум камеры" },
      { key: "globeness", label: "globeness (SDK)" },
      { key: "shape", label: "форма в рендере (SDK)" },
    ]);
    out.set("shape", PENDING);

    const slider = el("input", {
      type: "range",
      min: "3",
      max: "5",
      step: "0.01",
      value: "4",
      "data-testid": "globeness-slider",
    });

    const canvas = el("canvas", { width: "720", height: "480" });
    stage.replaceChildren(canvas);
    createMap(canvas, "ealing")
      .then((map) => {
        map.setCentre(FIXED_CENTRE.lon, FIXED_CENTRE.lat);
        const apply = () => {
          const zoom = Number(slider.value);
          map.setZoom(zoom);
          map.render();
          const g = map.globeness();
          stage.setAttribute("data-zoom", zoom.toFixed(2));
          stage.setAttribute("data-globeness", g.toFixed(3));
          out.set("zoom", zoom.toFixed(2));
          out.set("globeness", g.toFixed(3));
        };
        slider.addEventListener("input", apply);
        stage.setAttribute("data-state", "ready");
        apply();
      })
      .catch((error: unknown) => {
        stage.setAttribute("data-state", "error");
        stage.replaceChildren(String(error));
      });

    panel.append(
      section("Камера", controlRow("зум", slider)),
      section("Показания", out.root),
    );
  },
};
