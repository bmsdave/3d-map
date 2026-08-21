import { FIXED_CENTRE } from "../bands";
import { createPackageMap, type MapHandle } from "../sdk";
import { el, readout, type Readout, section } from "../ui";
import type { CardSpec } from "./types";

// Движения на плоской карте (z ≥ 5, глобус погашен). Карточка ловит сырые
// события ввода, пишет их в лог и отдаёт SDK; показания камеры приходят
// обратно из debug() — e2e шлёт жест и сверяет числа, не пиксели.

const LOG_CAP = 30;
const START_ZOOM = 12;

interface Driver {
  map: MapHandle | null;
  sync(): void;
  /** Гоняет кадры, пока камера едет сама: инерция, анимация зума. */
  coast(): void;
}

// Курсор в пикселях канваса: сцена растянута по колонке, а канвас держит
// свой размер, и жест SDK считает в его пикселях.
function canvasPoint(
  canvas: HTMLCanvasElement,
  event: { clientX: number; clientY: number },
): [number, number] {
  const rect = canvas.getBoundingClientRect();
  return [
    (event.clientX - rect.left) * (canvas.width / rect.width),
    (event.clientY - rect.top) * (canvas.height / rect.height),
  ];
}

function driver(stage: HTMLElement, out: Readout): Driver {
  let running = false;
  const self: Driver = {
    map: null,
    sync() {
      const map = self.map;
      if (!map) return;
      map.render();
      const state = map.debug();
      stage.setAttribute("data-zoom", state.zoom.toFixed(2));
      stage.setAttribute(
        "data-centre",
        `${state.centre_lon.toFixed(6)},${state.centre_lat.toFixed(6)}`,
      );
      stage.setAttribute("data-moving", String(state.moving));
      out.set("centre", `${state.centre_lon.toFixed(4)}, ${state.centre_lat.toFixed(4)}`);
      out.set("zoom", state.zoom.toFixed(2));
      out.set("bearing", `${state.bearing.toFixed(2)}°`);
    },
    coast() {
      const map = self.map;
      if (!map || running) return;
      running = true;
      let previous = performance.now();
      const frame = (now: number) => {
        const dt = Math.max(now - previous, 1);
        previous = now;
        const moving = map.tick(dt);
        self.sync();
        if (moving) requestAnimationFrame(frame);
        else running = false;
      };
      requestAnimationFrame(frame);
    },
  };
  return self;
}

function eventLog(): { root: HTMLElement; push: (kind: string, detail: string) => void } {
  const root = el("ul", { class: "event-log", "data-testid": "event-log" });
  return {
    root,
    push(kind, detail) {
      root.prepend(el("li", {}, [`${kind} ${detail}`]));
      while (root.children.length > LOG_CAP) root.lastChild?.remove();
    },
  };
}

type Push = (kind: string, detail: string) => void;

function attachPointer(
  stage: HTMLElement,
  canvas: HTMLCanvasElement,
  drive: Driver,
  push: Push,
): void {
  let dragging = false;
  let last: [number, number] = [0, 0];
  stage.addEventListener("pointerdown", (e) => {
    dragging = true;
    last = canvasPoint(canvas, e);
    stage.setPointerCapture(e.pointerId);
    drive.map?.pointerDown(last[0], last[1], e.timeStamp);
    push("pointerdown", `${Math.round(last[0])},${Math.round(last[1])}`);
  });
  stage.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const at = canvasPoint(canvas, e);
    push("drag", `Δ${Math.round(at[0] - last[0])},${Math.round(at[1] - last[1])}`);
    last = at;
    drive.map?.pointerMove(at[0], at[1], e.timeStamp);
    drive.sync();
  });
  stage.addEventListener("pointerup", () => {
    dragging = false;
    drive.map?.pointerUp();
    push("pointerup", "");
    drive.coast();
  });
  stage.addEventListener("dblclick", (e) => {
    const [x, y] = canvasPoint(canvas, e);
    drive.map?.doubleClick(x, y);
    push("dblclick", `${Math.round(x)},${Math.round(y)}`);
    drive.coast();
  });
}

function attachWheelAndKeys(
  stage: HTMLElement,
  canvas: HTMLCanvasElement,
  drive: Driver,
  push: Push,
): void {
  stage.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      const [x, y] = canvasPoint(canvas, e);
      // ctrlKey у wheel — так браузер доставляет пинч тачпада.
      drive.map?.wheel(x, y, e.deltaY, e.ctrlKey);
      push(e.ctrlKey ? "pinch" : "wheel", `Δ${e.deltaY.toFixed(1)}`);
      drive.sync();
    },
    { passive: false },
  );
  stage.addEventListener("keydown", (e) => {
    // Что клавиша значит, знает SDK; хост гасит её, только если она его.
    if (drive.map?.key(e.key) !== true) return;
    e.preventDefault();
    push("key", e.key);
    drive.sync();
  });
}

export const inputFlat: CardSpec = {
  id: "input-flat",
  title: "Ввод на плоскости",
  purpose:
    "Сцена на зуме, где мир — только плоскость. Drag, wheel, пинч, dblclick, клавиатура. Проверяет: каждый жест меняет камеру, и ровно так, как заявлено.",
  group: "Ввод",
  mount(stage, panel) {
    stage.setAttribute("data-zoom", START_ZOOM.toFixed(2));

    const out = readout([
      { key: "centre", label: "центр (SDK)" },
      { key: "zoom", label: "зум (SDK)" },
      { key: "bearing", label: "поворот (SDK)" },
    ]);
    const log = eventLog();
    const canvas = el("canvas", { width: "720", height: "480" });
    stage.replaceChildren(canvas);

    const drive = driver(stage, out);
    attachPointer(stage, canvas, drive, log.push);
    attachWheelAndKeys(stage, canvas, drive, log.push);

    createPackageMap(canvas, { zoom: START_ZOOM })
      .then(({ map, onSettled }) => {
        map.setCentre(FIXED_CENTRE.lon, FIXED_CENTRE.lat);
        map.setZoom(START_ZOOM);
        drive.map = map;
        // Показания снимаются с кадра, а тайлы под этой камерой
        // доезжают позже её движения.
        onSettled(() => drive.sync());
        drive.sync();
        stage.setAttribute("data-state", "ready");
      })
      .catch((error: unknown) => {
        stage.setAttribute("data-state", "error");
        stage.replaceChildren(String(error));
      });

    panel.append(section("Камера", out.root), section("Лог событий", log.root));
  },
};
