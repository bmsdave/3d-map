import { createMap, loadPackCentre, type MapHandle } from "../sdk";
import { controlRow, el, readout, section } from "../ui";
import type { CardSpec } from "./types";

// Дороги на самом детальном зуме: фиксированная сцена патологий —
// острый угол, Y-развязка, круг, дублёр, мост и тоннель. Ручки — ровно
// то, что описано в плане §4.2–4.4: казинг, предел митры, ширина в
// экранных пикселях. Все показания читаются из debug() SDK.

const MOTORWAY = "RoadMotorway";
const STREET = "RoadResidential";

export const roadsMicro: CardSpec = {
  id: "roads-micro",
  title: "Дороги на z17",
  purpose:
    "Фиксированная сцена z17: все классы дорог, развязка, острый угол. Проверяет: ширину в экранных пикселях, митру с пределом, казинг, порядок по классу.",
  group: "Отрисовка",
  mount(stage, panel) {
    const out = readout([
      { key: "joins", label: "соединения (SDK)" },
      { key: "casing", label: "казинг (SDK)" },
      { key: "widths", label: "ширины, px (SDK)" },
    ]);

    const casing = el("input", {
      type: "checkbox",
      "data-testid": "casing-toggle",
    });
    const miter = el("select", { "data-testid": "miter-limit" });
    for (const v of ["1.5", "2", "4"]) {
      miter.append(el("option", { value: v }, [v]));
    }
    miter.value = "2";

    const widthInput = (testId: string) =>
      el("input", {
        type: "number",
        min: "1",
        max: "24",
        step: "0.5",
        "data-testid": testId,
      });
    const motorway = widthInput("width-motorway");
    const street = widthInput("width-street");

    const canvas = el("canvas", { width: "720", height: "480" });
    stage.replaceChildren(canvas);

    const start = async (): Promise<void> => {
      const centre = await loadPackCentre("roads");
      const map = await createMap(canvas, "roads");
      map.setCentre(centre.lon, centre.lat);
      map.setZoom(centre.zoom);
      stage.setAttribute("data-zoom", centre.zoom.toFixed(2));
      stage.setAttribute("data-centre", `${centre.lon},${centre.lat}`);
      wire(map, { casing, miter, motorway, street }, stage, out);
      stage.setAttribute("data-state", "ready");
    };

    start().catch((error: unknown) => {
      stage.setAttribute("data-state", "error");
      stage.replaceChildren(String(error));
    });

    panel.append(
      section(
        "Стиль линий",
        el("div", {}, [
          controlRow(
            "казинг",
            el("label", { class: "switch" }, [
              casing,
              el("span", { class: "track" }),
            ]),
          ),
          controlRow("предел митры", miter),
          controlRow("магистраль, px", motorway),
          controlRow("улица, px", street),
        ]),
      ),
      section("Показания", out.root),
    );
  },
};

interface Controls {
  casing: HTMLInputElement;
  miter: HTMLSelectElement;
  motorway: HTMLInputElement;
  street: HTMLInputElement;
}

function wire(
  map: MapHandle,
  controls: Controls,
  stage: HTMLElement,
  out: ReturnType<typeof readout>,
): void {
  const sync = () => {
    map.render();
    const state = map.debug();
    out.set("joins", `митра ${state.joins.miter} · бевел ${state.joins.bevel}`);
    out.set("casing", state.casing ? "включён" : "выключен");
    const width = (name: string) => (state.road_widths[name] ?? 0).toFixed(1);
    out.set("widths", `магистраль ${width(MOTORWAY)} · улица ${width(STREET)}`);
    stage.setAttribute("data-casing", String(state.casing));
    stage.setAttribute("data-miter-limit", state.miter_limit.toFixed(1));
    stage.setAttribute("data-label-candidates", String(state.label_candidates));
  };

  // Ручки ширины стартуют с того, что даёт рампа стиля на этом зуме:
  // сначала показываем рампу, изменение — уже override.
  const widths = map.debug().road_widths;
  controls.motorway.value = String(widths[MOTORWAY] ?? 6);
  controls.street.value = String(widths[STREET] ?? 2);
  controls.casing.checked = map.debug().casing;

  controls.casing.addEventListener("change", () => {
    map.setRoadCasing(controls.casing.checked);
    sync();
  });
  controls.miter.addEventListener("change", () => {
    map.setMiterLimit(Number(controls.miter.value));
    sync();
  });
  const bindWidth = (input: HTMLInputElement, className: string) => {
    input.addEventListener("change", () => {
      map.setRoadWidthPx(className, Number(input.value));
      sync();
    });
  };
  bindWidth(controls.motorway, MOTORWAY);
  bindWidth(controls.street, STREET);
  sync();
}
