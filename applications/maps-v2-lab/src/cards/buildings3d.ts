import { FIXED_CENTRE } from "../bands";
import { createMap } from "../sdk";
import { controlRow, el, readout, section } from "../ui";
import type { CardSpec } from "./types";

const ZOOM = 16;

export const buildings3d: CardSpec = {
  id: "buildings-3d",
  title: "Здания: крыши и стены",
  purpose:
    "Высоты из MT2 v4 поднимают замкнутые стены и крыши над terrain; наклон камеры делает вертикаль наблюдаемой.",
  group: "Здания",
  mount(stage, panel) {
    const out = readout([
      { key: "tilt", label: "tilt (SDK)" },
      { key: "draw-calls", label: "draw calls (SDK)" },
      { key: "tiles", label: "тайлов (SDK)" },
    ]);
    const tilt = el("input", {
      type: "range",
      min: "0",
      max: "60",
      step: "1",
      value: "0",
      "data-testid": "building-tilt-slider",
    });
    const canvas = el("canvas", { width: "720", height: "480" });
    stage.replaceChildren(canvas);

    createMap(canvas, "ealing")
      .then((map) => {
        map.setCentre(FIXED_CENTRE.lon, FIXED_CENTRE.lat);
        map.setZoom(ZOOM);
        const apply = () => {
          map.setTilt(Number(tilt.value));
          map.render();
          const state = map.debug();
          stage.setAttribute("data-tilt", state.tilt.toFixed(1));
          out.set("tilt", state.tilt.toFixed(1));
          out.set("draw-calls", String(state.draw_calls));
          out.set("tiles", String(state.tiles_drawn));
        };
        tilt.addEventListener("input", apply);
        panel.append(section("Камера", controlRow("tilt, °", tilt)), section("Показания", out.root));
        stage.setAttribute("data-state", "ready");
        apply();
      })
      .catch((error: unknown) => {
        stage.setAttribute("data-state", "error");
        stage.replaceChildren(String(error));
      });
  },
};
