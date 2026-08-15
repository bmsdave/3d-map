import { createMap } from "../sdk";
import { controlRow, el, readout, section, switchControl } from "../ui";
import type { CardSpec } from "./types";

// Рельеф на глобусе: вершинное смещение сферы r = R(1 + h·k·g). Множитель
// гаснет вместе с globeness — на плоскости радиуса, который можно раздуть,
// уже нет, и ползунок там ничего не двигает. Затенение на глобусе считается
// с тем же преувеличением, с каким подняты вершины: свет должен описывать
// ту планету, на которую человек смотрит.

const SCENE = { lon: -0.3049, lat: 51.5149, zoom: 2.5 };

export const globeRelief: CardSpec = {
  id: "globe-relief",
  title: "Рельеф: глобус",
  purpose:
    "Сфера с высотами при фиксированной камере. Проверяет: вершинное смещение r = R(1 + h·k·g), преувеличение как ручка, рельеф гаснет вместе с globeness.",
  group: "Рельеф",
  mount(stage, panel) {
    const out = readout([
      { key: "shape", label: "форма в рендере (SDK)" },
      { key: "globeness", label: "globeness (SDK)" },
      { key: "relief", label: "рельеф (SDK)" },
      { key: "exaggeration", label: "преувеличение k (SDK)" },
      { key: "height-tiles", label: "тайлов с высотами (SDK)" },
    ]);

    const slider = el("input", {
      type: "range",
      min: "0",
      max: "120",
      step: "1",
      value: "40",
      "data-testid": "exaggeration-slider",
    });

    const canvas = el("canvas", { width: "720", height: "480" });
    stage.replaceChildren(canvas);

    createMap(canvas, "ridge")
      .then((map) => {
        map.setCentre(SCENE.lon, SCENE.lat);
        map.setZoom(SCENE.zoom);
        map.setRelief(true);
        map.setHypsometric(true);
        const apply = () => {
          map.setReliefExaggeration(Number(slider.value));
          map.render();
          const state = map.debug();
          stage.setAttribute("data-shape", state.shape);
          stage.setAttribute("data-exaggeration", state.exaggeration.toFixed(0));
          out.set("shape", state.shape);
          out.set("globeness", state.globeness.toFixed(3));
          out.set("relief", state.relief ? "есть" : "нет");
          out.set("exaggeration", `×${state.exaggeration.toFixed(0)}`);
          out.set("height-tiles", String(state.height_tiles));
        };
        slider.addEventListener("input", apply);

        panel.append(
          section(
            "Стиль",
            el("div", {}, [
              controlRow(
                "рельеф",
                switchControl(true, "relief-toggle", (on) => {
                  map.setRelief(on);
                  apply();
                }),
              ),
              controlRow("преувеличение", slider),
            ]),
          ),
          section("Показания", out.root),
        );
        stage.setAttribute("data-state", "ready");
        apply();
      })
      .catch((error: unknown) => {
        stage.setAttribute("data-state", "error");
        stage.replaceChildren(String(error));
      });
  },
};
