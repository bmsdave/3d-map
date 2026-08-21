import { createPackageMap, TRAFALGAR } from "../sdk";
import { controlRow, el, readout, section, switchControl } from "../ui";
import type { CardSpec } from "./types";

// Рельеф на плоскости: настоящий рельеф — Copernicus над сушей, GEBCO
// под водой, — свет с северо-запада.
// Тоглы разделяют две независимые вещи — затенение (форма) и гипсометрию
// (высота цветом); ползунок выразительности задаёт вертикальное
// преувеличение, с которым считается нормаль: на честном масштабе склон
// в 200 м на 30 км — пятая часть градуса, и тихий холст его не показывает.

/** Та же точка, что у всех карточек, но с высоты, с которой рельеф
 *  вообще есть: Лондон плоский, а z6 вокруг него — это Британские
 *  острова, Ла-Манш и шельф, то есть настоящий разброс высот. Глубже
 *  пятого зума карточка ещё и заведомо плоская: выше начинается
 *  переход в глобус, а это предмет соседней студии. */
const SCENE = { ...TRAFALGAR, zoom: 6 };

export const terrainShade: CardSpec = {
  id: "terrain-shade",
  title: "Рельеф: затенение",
  purpose:
    "Настоящий рельеф на плоскости. Проверяет: нормаль из градиента высоты, свет с северо-запада, гипсометрия из стиля, выразительность — параметр, а не константа.",
  group: "Рельеф",
  mount(stage, panel) {
    const out = readout([
      { key: "shape", label: "форма в рендере (SDK)" },
      { key: "relief", label: "затенение (SDK)" },
      { key: "hypsometric", label: "гипсометрия (SDK)" },
      { key: "expressiveness", label: "выразительность (SDK)" },
      { key: "height", label: "высота под центром (SDK)" },
      { key: "height-tiles", label: "тайлов с высотами (SDK)" },
    ]);

    const slider = el("input", {
      type: "range",
      min: "0",
      max: "1",
      step: "0.01",
      value: "0.5",
      "data-testid": "expressiveness-slider",
    });

    const canvas = el("canvas", { width: "720", height: "480" });
    stage.replaceChildren(canvas);

    createPackageMap(canvas, { zoom: SCENE.zoom, centre: SCENE })
      .then(({ map, onSettled }) => {
        map.setCentre(SCENE.lon, SCENE.lat);
        map.setZoom(SCENE.zoom);
        map.setRelief(true);
        map.setHypsometric(true);
        const apply = () => {
          map.setReliefExpressiveness(Number(slider.value));
          map.render();
          const state = map.debug();
          stage.setAttribute("data-relief", String(state.relief));
          stage.setAttribute("data-expressiveness", state.expressiveness.toFixed(2));
          out.set("shape", state.shape);
          out.set("relief", state.relief ? "есть" : "нет");
          out.set("hypsometric", state.hypsometric ? "есть" : "нет");
          out.set("expressiveness", state.expressiveness.toFixed(2));
          out.set(
            "height",
            state.centre_height_m === null ? "нет данных" : `${state.centre_height_m} м`,
          );
          out.set("height-tiles", String(state.height_tiles));
        };
        slider.addEventListener("input", apply);

        panel.append(
          section(
            "Стиль",
            el("div", {}, [
              controlRow(
                "затенение",
                switchControl(true, "relief-toggle", (on) => {
                  map.setRelief(on);
                  apply();
                }),
              ),
              controlRow(
                "гипсометрия",
                switchControl(true, "hypsometric-toggle", (on) => {
                  map.setHypsometric(on);
                  apply();
                }),
              ),
              controlRow("выразительность", slider),
            ]),
          ),
          section("Показания", out.root),
        );
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
  },
};
