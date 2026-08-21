import { FIXED_CENTRE } from "../bands";
import { createPackageMap } from "../sdk";
import { controlRow, el, readout, section } from "../ui";
import type { CardSpec } from "./types";

// Образец гарнитуры: одна строка на рампе кеглей, поле знаковых
// расстояний в фрагментном шейдере. Проверяемое глазами — резкость на
// любом кегле и halo; проверяемое числами — что поворот и наклон
// камеры не трогают текст: он не в меше мира, а отдельным проходом.

const DEFAULT_TEXT = "Ealing Broadway 1863";

export const typeSpecimen: CardSpec = {
  id: "type-specimen",
  title: "Образец гарнитуры",
  purpose:
    "Одна строка на шести кеглях, SDF в фрагментном шейдере. Проверяет: резкость на любом размере, halo, и что bearing и tilt не вращают и не наклоняют текст.",
  group: "Подписи",
  mount(stage, panel) {
    const out = readout([
      { key: "text", label: "строка" },
      { key: "halo", label: "halo, em" },
      { key: "bearing", label: "bearing (SDK)" },
      { key: "tilt", label: "tilt (SDK)" },
      { key: "draw-calls", label: "draw calls (SDK)" },
    ]);

    const input = el("input", {
      type: "text",
      value: DEFAULT_TEXT,
      "data-testid": "specimen-text",
    });
    const halo = slider("halo-slider", "0", "0.14", "0.01", "0.07");
    const bearing = slider("bearing-slider", "0", "360", "1", "0");
    const tilt = slider("tilt-slider", "0", "60", "1", "0");

    let apply = () => {};
    const canvas = el("canvas", { width: "720", height: "480" });
    stage.replaceChildren(canvas);
    stage.setAttribute("data-centre", `${FIXED_CENTRE.lon},${FIXED_CENTRE.lat}`);

    createPackageMap(canvas, { zoom: 14 })
      .then(({ map, onSettled }) => {
        map.setCentre(FIXED_CENTRE.lon, FIXED_CENTRE.lat);
        map.setZoom(14);
        apply = () => {
          map.setSpecimen(input.value);
          map.setHaloEm(Number(halo.value));
          map.setBearing(Number(bearing.value));
          map.setTilt(Number(tilt.value));
          map.render();
          const state = map.debug();
          out.set("text", input.value);
          out.set("halo", Number(halo.value).toFixed(2));
          out.set("bearing", state.bearing.toFixed(1));
          out.set("tilt", state.tilt.toFixed(1));
          out.set("draw-calls", String(state.draw_calls));
          stage.setAttribute("data-bearing", state.bearing.toFixed(1));
          stage.setAttribute("data-tilt", state.tilt.toFixed(1));
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

    for (const control of [input, halo, bearing, tilt]) {
      control.addEventListener("input", () => apply());
    }

    panel.append(
      section(
        "Набор",
        el("div", {}, [
          controlRow("строка", input),
          controlRow("halo, em", halo),
        ]),
      ),
      section(
        "Камера",
        el("div", {}, [
          controlRow("bearing, °", bearing),
          controlRow("tilt, °", tilt),
        ]),
      ),
      section("Показания", out.root),
    );
  },
};

function slider(
  testId: string,
  min: string,
  max: string,
  step: string,
  value: string,
): HTMLInputElement {
  return el("input", { type: "range", min, max, step, value, "data-testid": testId });
}
