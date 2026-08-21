import { PerfOverlay } from "../perfOverlay";
import { createMap, createTilePackageLoader, type MapHandle, type TilePackageLoader } from "../sdk";
import { controlRow, el, readout, section } from "../ui";
import type { CardSpec } from "./types";

const MAP_MANIFEST = "https://maps2.local/map/manifest.json";

/// Below this the ground is a generalised world and relief is the only
/// thing that makes it visible; at and above it a real city extract owns
/// the ground, where hypsometric tint would read as countryside and
/// shading a 30 m DEM speckles the streets.
const CITY_ENTRY_ZOOM = 12;

function canvasPoint(canvas: HTMLCanvasElement, event: PointerEvent | WheelEvent): [number, number] {
  const rect = canvas.getBoundingClientRect();
  return [(event.clientX - rect.left) * canvas.width / rect.width, (event.clientY - rect.top) * canvas.height / rect.height];
}

function attachNavigation(canvas: HTMLCanvasElement, map: MapHandle, refresh: () => void): void {
  let dragging = false;
  canvas.addEventListener("pointerdown", (event) => {
    dragging = true;
    canvas.setPointerCapture(event.pointerId);
    map.pointerDown(...canvasPoint(canvas, event), event.timeStamp);
  });
  canvas.addEventListener("pointermove", (event) => {
    if (!dragging) return;
    map.pointerMove(...canvasPoint(canvas, event), event.timeStamp);
    map.render();
    refresh();
  });
  canvas.addEventListener("pointerup", () => { dragging = false; map.pointerUp(); });
  canvas.addEventListener("wheel", (event) => {
    event.preventDefault();
    map.wheel(...canvasPoint(canvas, event), event.deltaY, event.ctrlKey);
    map.render();
    refresh();
  }, { passive: false });
}

/**
 * One package, built from every source at once.
 *
 * The globe-real card composes two packages in the browser, which is a
 * real SDK capability but the wrong place to decide which source owns a
 * piece of ground: by tile time a feature no longer knows where it came
 * from, so the same city arrives twice at two coordinates and the same
 * motorway as two geometries. `maps2-ingest build-map` settles that once,
 * against a build plan, and hands the browser a single pyramid.
 */
export const mapReal: CardSpec = {
  id: "map-real",
  title: "Карта: один пакет из всех источников",
  purpose:
    "Один пакет, собранный build-map из плана: мир (береговые линии, рельеф GEBCO, границы, города, магистрали) и реальный город в одной пирамиде без разрывов. Пересечения источников разрешены на сборке, а не в браузере.",
  group: "Глобус",
  mount(stage, panel) {
    const out = readout([
      { key: "shape", label: "форма (SDK)" },
      { key: "zoom", label: "зум камеры" },
      { key: "tile-level", label: "уровень тайлов (SDK)" },
      { key: "levels", label: "уровни пакета" },
      { key: "tiles", label: "загружено тайлов" },
      { key: "attribution", label: "атрибуция" },
    ]);
    const urlInput = el("input", {
      type: "url", value: MAP_MANIFEST, "data-testid": "map-real-url", "aria-label": "URL manifest пакета",
    });
    const load = el("button", { type: "button", "data-testid": "map-real-load" }, ["Загрузить пакет"]);
    const tilt = el("input", {
      type: "range", min: "0", max: "60", step: "1", value: "0",
      "data-testid": "map-real-tilt", "aria-label": "Наклон карты",
    });
    const zoomSlider = el("input", {
      type: "range", min: "0", max: "18", step: "0.1", value: "2",
      "data-testid": "map-real-zoom", "aria-label": "Зум карты",
    });
    const perfToggle = el("input", {
      type: "checkbox", "data-testid": "map-real-perf", "aria-label": "Показывать профиль кадра",
    });
    const source = section("Источник", el("div", {}, [
      controlRow("Пакет", urlInput),
      load,
      controlRow("Наклон", tilt),
      controlRow("Зум", zoomSlider),
      controlRow("Профиль кадра", perfToggle),
    ]));

    let generation = 0;
    let activeMap: MapHandle | null = null;
    let activeRefresh: (() => void) | null = null;
    const applyControls = () => {
      if (!activeMap) return;
      activeMap.setTilt(Number(tilt.value));
      activeMap.setZoom(Number(zoomSlider.value));
      activeMap.render();
      activeRefresh?.();
    };
    const showError = (error: unknown, manifestUrl: string) => {
      const retry = el("button", { type: "button" }, ["Повторить"]);
      retry.addEventListener("click", () => void loadPackage());
      stage.setAttribute("data-state", "error");
      stage.replaceChildren(
        `Не удалось загрузить ${manifestUrl}: ${String(error)}. Проверьте URL — по умолчанию это заглушка, а не рабочий сервер.`,
        retry,
      );
    };
    const showIdle = () => {
      stage.setAttribute("data-state", "idle");
      stage.replaceChildren(
        "Укажите URL manifest пакета, собранного `maps2-ingest build-map`, и нажмите «Загрузить пакет». " +
          "По умолчанию поле указывает на заглушку — она никогда не отвечает.",
      );
    };

    const loadPackage = async () => {
      const request = ++generation;
      const canvas = el("canvas", { width: "720", height: "480" });
      stage.setAttribute("data-state", "loading");
      stage.removeAttribute("data-loaded");
      stage.replaceChildren(canvas);
      activeMap = null;
      activeRefresh = null;
      const manifestUrl = urlInput.value.trim();
      const overlay = new PerfOverlay();
      overlay.root.hidden = !perfToggle.checked;
      perfToggle.onchange = () => { overlay.root.hidden = !perfToggle.checked; };
      stage.style.position = "relative";
      stage.append(overlay.root);
      try {
        const map = await createMap(canvas, null);
        const loader: TilePackageLoader = await createTilePackageLoader(map, manifestUrl);
        if (request !== generation) return;
        map.setReliefExpressiveness(0.22);
        map.setCentre(loader.manifest.view.lon, loader.manifest.view.lat);
        map.setZoom(Number(zoomSlider.value));
        const levels = [...new Set(loader.manifest.tiles.map((path) => Number(path.split("/")[0])))]
          .sort((a, b) => a - b);
        out.set("levels", `${levels[0]}–${levels[levels.length - 1]}`);
        let loaded = 0;
        let terrainShown: boolean | null = null;
        const applyTerrainForZoom = (): boolean => {
          const wanted = map.debug().zoom < CITY_ENTRY_ZOOM;
          if (wanted === terrainShown) return false;
          terrainShown = wanted;
          map.setRelief(wanted);
          map.setHypsometric(wanted);
          return true;
        };
        // One refresh at a time. Pointer moves arrive far faster than a
        // load can finish, and starting a fresh one per event piled up
        // concurrent passes over the same camera; the last camera is the
        // only one worth answering, so a request that arrives mid-flight
        // just marks the answer stale.
        let refreshing = false;
        let stale = false;
        const refresh = async () => {
          if (refreshing) { stale = true; return; }
          refreshing = true;
          try {
            do {
              stale = false;
              await runRefresh();
              if (request !== generation) return;
            } while (stale);
          } finally {
            refreshing = false;
          }
        };
        const runRefresh = async () => {
          overlay.beginHostWork();
          if (applyTerrainForZoom()) map.render();
          const startedLoad = performance.now();
          const result = await loader.loadVisible();
          overlay.recordLoad(result.blockedMs, performance.now() - startedLoad);
          if (request !== generation) return;
          loaded += result.loaded;
          out.set("tiles", String(loaded));
          stage.setAttribute("data-loaded", String(loaded));
          const state = map.debug();
          out.set("shape", state.shape);
          out.set("zoom", state.zoom.toFixed(2));
          out.set("tile-level", String(state.tile_level));
          stage.setAttribute("data-shape", state.shape);
          stage.setAttribute("data-tile-level", String(state.tile_level));
          overlay.endHostWork(map);
        };
        await refresh();
        if (request !== generation) return;
        activeMap = map;
        activeRefresh = () => void refresh();
        applyControls();
        out.set(
          "attribution",
          Array.from(new Set(loader.manifest.sources.map((entry) => entry.attribution))).join(" · "),
        );
        panel.replaceChildren(source, section("Показания", out.root));
        stage.setAttribute("data-state", "ready");
        attachNavigation(canvas, map, () => void refresh());
      } catch (error) {
        if (request === generation) showError(error, manifestUrl);
      }
    };

    load.addEventListener("click", () => void loadPackage());
    tilt.addEventListener("input", applyControls);
    zoomSlider.addEventListener("input", applyControls);
    panel.append(source);
    showIdle();
  },
};
