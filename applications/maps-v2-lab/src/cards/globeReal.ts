import { createMap, createTilePackageLoader, type MapHandle, type TilePackageLoader } from "../sdk";
import { controlRow, el, readout, section } from "../ui";
import type { CardSpec } from "./types";

const WORLD_MANIFEST = "https://maps2.local/world/manifest.json";
const CITY_MANIFEST = "https://maps2.local/city/manifest.json";

/// The zoom a street-level city package conventionally starts at, and so
/// the zoom this demo hands the ground over from world terrain to real
/// city geometry.
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
 * The globe-to-city demo: a real, world-wide low-zoom package (real
 * coastlines, real ocean, no planet-scale OSM parsing required — see
 * world_water in maps2-ingest) composed additively with a real
 * high-zoom regional package (e.g. the local London build). Spin the
 * globe at low zoom, zoom in, and the same map keeps rendering without
 * a reload once the regional package's levels take over.
 */
export const globeReal: CardSpec = {
  id: "globe-real",
  title: "Глобус: мир + реальный город",
  purpose:
    "Два реальных пакета на одной карте: мир (мелкие зумы, реальные береговые линии и рельеф GEBCO) снизу, город (крупные зумы) поверх — addSourceLevels объединяет уровни без замены. Там, где у города нет покрытия, подкладывается ближайший мировой тайл, а не пустота.",
  group: "Глобус",
  mount(stage, panel) {
    const out = readout([
      { key: "shape", label: "форма (SDK)" },
      { key: "zoom", label: "зум камеры" },
      { key: "tile-level", label: "уровень тайлов (SDK)" },
      { key: "world-tiles", label: "загружено из мира" },
      { key: "city-tiles", label: "загружено из города" },
      { key: "attribution", label: "атрибуция" },
    ]);
    const worldInput = el("input", {
      type: "url", value: WORLD_MANIFEST, "data-testid": "globe-real-world-url", "aria-label": "URL мирового manifest",
    });
    const cityInput = el("input", {
      type: "url", value: CITY_MANIFEST, "data-testid": "globe-real-city-url", "aria-label": "URL городского manifest",
    });
    const load = el("button", { type: "button", "data-testid": "globe-real-load" }, ["Загрузить оба пакета"]);
    const tilt = el("input", {
      type: "range", min: "0", max: "60", step: "1", value: "0",
      "data-testid": "globe-real-tilt", "aria-label": "Наклон карты",
    });
    const zoomSlider = el("input", {
      type: "range", min: "0", max: "18", step: "0.1", value: "2",
      "data-testid": "globe-real-zoom", "aria-label": "Зум карты",
    });
    const source = section("Источники", el("div", {}, [
      controlRow("Мир (мелкие зумы)", worldInput),
      controlRow("Город (крупные зумы)", cityInput),
      load,
      controlRow("Наклон", tilt),
      controlRow("Зум", zoomSlider),
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
    const showError = (error: unknown, manifestUrl?: string) => {
      const retry = el("button", { type: "button" }, ["Повторить"]);
      retry.addEventListener("click", () => void loadBoth());
      const detail = manifestUrl
        ? `Не удалось загрузить ${manifestUrl}: ${String(error)}. Проверьте URL — по умолчанию это заглушка, а не рабочий сервер.`
        : String(error);
      stage.setAttribute("data-state", "error");
      stage.replaceChildren(detail, retry);
    };
    const showIdle = () => {
      stage.setAttribute("data-state", "idle");
      stage.replaceChildren(
        "Введите URL двух реальных manifest-пакетов (мир и город) и нажмите «Загрузить оба пакета». " +
          "По умолчанию поля указывают на заглушку — она никогда не отвечает.",
      );
    };

    const loadBoth = async () => {
      const request = ++generation;
      const canvas = el("canvas", { width: "720", height: "480" });
      stage.setAttribute("data-state", "loading");
      stage.removeAttribute("data-world-loaded");
      stage.removeAttribute("data-city-loaded");
      stage.replaceChildren(canvas);
      activeMap = null;
      activeRefresh = null;
      let attemptedUrl = worldInput.value.trim();
      try {
        const map = await createMap(canvas, null);
        // World first, plain (replace) so its shallow pyramid is the
        // base; city second, additive so its deep levels join rather
        // than wipe the world's — see addSourceLevels's doc comment.
        const worldLoader: TilePackageLoader = await createTilePackageLoader(map, attemptedUrl);
        attemptedUrl = cityInput.value.trim();
        const cityLoader: TilePackageLoader = await createTilePackageLoader(
          map, attemptedUrl, { additive: true },
        );
        if (request !== generation) return;
        // Relief is the backdrop, not the subject: at the default
        // expressiveness GEBCO's slopes come out loud enough to bury the
        // roads, borders and place names drawn over them.
        map.setReliefExpressiveness(0.22);
        map.setCentre(worldLoader.manifest.view.lon, worldLoader.manifest.view.lat);
        map.setZoom(Number(zoomSlider.value));
        let worldLoaded = 0;
        let cityLoaded = 0;
        let terrainShown: boolean | null = null;
        // Terrain is what makes the *world* package visible at all: its
        // land carries no vector features, only water polygons and a
        // height raster, so with relief off every continent renders as
        // bare background. Inside the city package the same setting
        // works against the map — hypsometric green over streets reads
        // as countryside, and shading a 30 m DEM speckles them — so the
        // relief belongs to the zooms the world package actually serves.
        const applyTerrainForZoom = (): boolean => {
          const wanted = map.debug().zoom < CITY_ENTRY_ZOOM;
          if (wanted === terrainShown) return false;
          terrainShown = wanted;
          map.setRelief(wanted);
          map.setHypsometric(wanted);
          return true;
        };
        const refresh = async () => {
          if (applyTerrainForZoom()) map.render();
          const [worldResult, cityResult] = await Promise.all([worldLoader.loadVisible(), cityLoader.loadVisible()]);
          if (request !== generation) return;
          worldLoaded += worldResult.loaded;
          cityLoaded += cityResult.loaded;
          out.set("world-tiles", String(worldLoaded));
          out.set("city-tiles", String(cityLoaded));
          stage.setAttribute("data-world-loaded", String(worldLoaded));
          stage.setAttribute("data-city-loaded", String(cityLoaded));
          const state = map.debug();
          out.set("shape", state.shape);
          out.set("zoom", state.zoom.toFixed(2));
          out.set("tile-level", String(state.tile_level));
          stage.setAttribute("data-shape", state.shape);
          stage.setAttribute("data-tile-level", String(state.tile_level));
        };
        await refresh();
        if (request !== generation) return;
        activeMap = map;
        activeRefresh = () => void refresh();
        applyControls();
        const attributions = [...worldLoader.manifest.sources, ...cityLoader.manifest.sources]
          .map((entry) => entry.attribution);
        out.set("attribution", Array.from(new Set(attributions)).join(" · "));
        panel.replaceChildren(source, section("Показания", out.root));
        stage.setAttribute("data-state", "ready");
        attachNavigation(canvas, map, () => void refresh());
      } catch (error) {
        if (request === generation) showError(error, attemptedUrl);
      }
    };

    load.addEventListener("click", () => void loadBoth());
    tilt.addEventListener("input", applyControls);
    zoomSlider.addEventListener("input", () => {
      applyControls();
    });
    panel.append(source);
    // The default field values are documentation, not a working
    // manifest — unlike packageLoader's bundled fixture, real world +
    // city packages are too large to ship in the lab itself (see the
    // README's Demos section for how to build and serve them locally).
    // Fetching them unconditionally on mount previously failed instantly
    // with an opaque "TypeError: Failed to fetch" and no indication why.
    showIdle();
  },
};
