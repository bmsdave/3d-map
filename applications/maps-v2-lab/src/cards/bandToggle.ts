import { FIXED_CENTRE, type Transition } from "../bands";
import { createPackageMap, renderUntilSettled } from "../sdk";
import { controlRow, el, PENDING, readout, section } from "../ui";
import type { CardSpec } from "./types";

// Тогл-карточка перехода между полосами. Камера зафиксирована — и позиция,
// и зум; тогл переключает только состав классов через override полосы в
// SDK. Это третий из трёх независимых переходов, дёрнутый в изоляции.

export function bandToggleCard(transition: Transition): CardSpec {
  const { lower, upper } = transition;
  const id = `toggle-${lower.name.toLowerCase()}-${upper.name.toLowerCase()}`;
  // Камера стоит на границе полос: на этом зуме обе композиции читаемы.
  const cameraZoom = upper.entry;

  return {
    id,
    title: `${lower.name} ↔ ${upper.name}`,
    purpose:
      `Камера неподвижна (z${cameraZoom}, одно место). Тогл меняет состав классов: полоса ${lower.name} или ${upper.name}. Видно передачу эстафеты без скролла и без глобуса.`,
    group: "Тоглы переходов",
    mount(stage, panel) {
      stage.setAttribute("data-zoom", cameraZoom.toFixed(2));
      stage.setAttribute(
        "data-centre",
        `${FIXED_CENTRE.lon},${FIXED_CENTRE.lat}`,
      );

      const out = readout([
        { key: "camera", label: "камера (фиксирована)" },
        { key: "composition", label: "активная полоса" },
        { key: "classes", label: "классы на экране (SDK)" },
      ]);
      out.set("camera", `z${cameraZoom} · Трафальгарская площадь`);

      const bandSelect = el("select", { "data-testid": "composition-toggle" });
      for (const band of [lower, upper]) {
        bandSelect.append(
          el("option", { value: band.name }, [`${band.name} (z${band.entry})`]),
        );
      }
      bandSelect.value = lower.name;

      const mode = el("select", { "data-testid": "transition-mode" });
      mode.append(
        el("option", { value: "instant" }, ["мгновенно"]),
        el("option", { value: "animated" }, ["с анимацией"]),
      );
      stage.setAttribute("data-transition-mode", "instant");

      const canvas = el("canvas", { width: "720", height: "480" });
      stage.replaceChildren(canvas);
      createPackageMap(canvas, { zoom: cameraZoom })
        .then(({ map, onSettled }) => {
          map.setCentre(FIXED_CENTRE.lon, FIXED_CENTRE.lat);
          map.setZoom(cameraZoom);
          const sync = () => {
            const state = map.debug();
            stage.setAttribute("data-composition", state.composition);
            out.set("composition", state.composition);
            out.set(
              "classes",
              state.resident_classes.length > 0
                ? state.resident_classes.join(", ")
                : PENDING,
            );
          };
          const apply = () => {
            map.setBandOverride(bandSelect.value);
            renderUntilSettled(map, sync);
          };
          bandSelect.addEventListener("change", apply);
          mode.addEventListener("change", () => {
            stage.setAttribute("data-transition-mode", mode.value);
            map.setTransitionAnimated(mode.value === "animated");
          });
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

      panel.append(
        section(
          "Состав классов",
          el("div", {}, [
            controlRow("полоса", bandSelect),
            controlRow("переход", mode),
          ]),
        ),
        section("Показания", out.root),
      );
    },
  };
}
