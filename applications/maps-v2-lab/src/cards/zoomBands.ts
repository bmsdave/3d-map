import { BANDS, expectedBand, FIXED_CENTRE } from "../bands";
import { createMap } from "../sdk";
import { controlRow, el, PENDING, readout, section } from "../ui";
import type { CardSpec } from "./types";

// Непрерывный зум против дискретных полос: слайдер двигает камеру,
// readout сверяет ожидаемую полосу (спецификация) с фактической (SDK).

export const zoomBands: CardSpec = {
  id: "zoom-bands",
  title: "Зум и полосы",
  purpose:
    "Один вьюпорт, слайдер непрерывного зума z0–z17. Проверяет: зум камеры непрерывен, членство классов меняется только на границах полос.",
  group: "Переходы",
  mount(stage, panel) {
    const out = readout([
      { key: "zoom", label: "зум камеры" },
      { key: "band-expected", label: "полоса (спецификация)" },
      { key: "band-actual", label: "полоса (SDK)" },
      { key: "tile-level", label: "уровень тайлов (SDK)" },
      { key: "classes", label: "классы на экране (SDK)" },
    ]);

    const slider = el("input", {
      type: "range",
      min: "0",
      max: "17",
      step: "0.01",
      value: "12",
      "data-testid": "zoom-slider",
    });

    let syncSdk: (zoom: number) => void = () => {};

    const apply = () => {
      const zoom = Number(slider.value);
      stage.setAttribute("data-zoom", zoom.toFixed(2));
      out.set("zoom", zoom.toFixed(2));
      out.set("band-expected", expectedBand(zoom).name);
      syncSdk(zoom);
    };
    slider.addEventListener("input", apply);

    stage.setAttribute(
      "data-centre",
      `${FIXED_CENTRE.lon},${FIXED_CENTRE.lat}`,
    );

    const canvas = el("canvas", { width: "720", height: "480" });
    stage.replaceChildren(canvas);
    createMap(canvas, "ealing")
      .then((map) => {
        map.setCentre(FIXED_CENTRE.lon, FIXED_CENTRE.lat);
        syncSdk = (zoom) => {
          map.setZoom(zoom);
          map.render();
          const state = map.debug();
          out.set("band-actual", state.band);
          out.set("tile-level", String(state.tile_level));
          out.set(
            "classes",
            state.resident_classes.length > 0 ? state.resident_classes.join(", ") : PENDING,
          );
        };
        stage.setAttribute("data-state", "ready");
        apply();
      })
      .catch((error: unknown) => {
        stage.setAttribute("data-state", "error");
        stage.replaceChildren(String(error));
      });
    apply();

    panel.append(
      section("Камера", controlRow("зум", slider)),
      section("Показания", out.root),
      section(
        "Границы полос",
        el(
          "div",
          { class: "mono" },
          [BANDS.map((b) => `${b.name} z${b.entry}`).join(" · ")],
        ),
      ),
    );
  },
};
