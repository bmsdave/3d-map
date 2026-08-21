import { FIXED_CENTRE } from "../bands";
import { overlapCount, placed, rankInversions } from "../labels";
import { createPackageMap } from "../sdk";
import { controlRow, el, readout, section, switchControl } from "../ui";
import type { CardSpec } from "./types";

// Коллизии и ранг. Карточка показывает то, что обычно невидимо:
// боксы размещённых и отвергнутых. Показания — не «видна ли подпись X»,
// а инварианты кадра: пересечений ноль, инверсий ранга ноль.

export const labelsCollision: CardSpec = {
  id: "labels-collision",
  title: "Коллизии и ранг",
  purpose:
    "Кадр подписей с боксами коллизий. Проверяет: боксы размещённых не пересекаются, отвергнутый не важнее вытеснившего, кадр детерминирован, дубли через границу тайла размещаются один раз.",
  group: "Подписи",
  mount(stage, panel) {
    const out = readout([
      { key: "candidates", label: "кандидатов (SDK)" },
      { key: "placed", label: "размещено (SDK)" },
      { key: "rejected", label: "отвергнуто (SDK)" },
      { key: "collisions", label: "из них по коллизии" },
      { key: "duplicates", label: "дублей через границу" },
      { key: "overlaps", label: "пересечений боксов" },
      { key: "inversions", label: "инверсий ранга" },
      { key: "occupancy", label: "занято экрана" },
    ]);

    const zoom = el("input", {
      type: "range",
      min: "12",
      max: "17",
      step: "0.1",
      value: "16",
      "data-testid": "zoom-slider",
    });

    let apply = () => {};
    const boxes = switchControl(true, "boxes-toggle", () => apply());

    const canvas = el("canvas", { width: "720", height: "480" });
    stage.replaceChildren(canvas);
    stage.setAttribute("data-centre", `${FIXED_CENTRE.lon},${FIXED_CENTRE.lat}`);

    createPackageMap(canvas, { zoom: Number(zoom.value) })
      .then(({ map, onSettled }) => {
        map.setCentre(FIXED_CENTRE.lon, FIXED_CENTRE.lat);
        // Без бюджета: он останавливает кадр раньше, чем начинаются
        // коллизии, и тогда все отказы одинаковы. Бюджет — предмет
        // соседней карточки, здесь мешает смотреть на то, ради чего
        // карточка сделана.
        map.setLabelBudget(1);
        apply = () => {
          const on = (boxes.querySelector("input") as HTMLInputElement).checked;
          map.setCollisionBoxes(on);
          map.setZoom(Number(zoom.value));
          map.render();
          const state = map.debug();
          const entries = map.labelDebug();
          const by = (name: string) => entries.filter((e) => e.state === name).length;
          out.set("candidates", String(state.label_candidates));
          out.set("placed", String(state.labels_placed));
          out.set("rejected", String(state.labels_rejected));
          out.set("collisions", String(by("collision")));
          out.set("duplicates", String(by("duplicate")));
          out.set("overlaps", String(overlapCount(entries)));
          out.set("inversions", String(rankInversions(entries)));
          out.set("occupancy", `${(state.label_occupancy * 100).toFixed(1)} %`);
          stage.setAttribute("data-zoom", Number(zoom.value).toFixed(2));
          stage.setAttribute("data-placed", String(placed(entries).length));
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

    zoom.addEventListener("input", () => apply());

    panel.append(
      section(
        "Кадр",
        el("div", {}, [
          controlRow("зум", zoom),
          controlRow("боксы коллизий", boxes),
        ]),
      ),
      section("Показания", out.root),
    );
  },
};
