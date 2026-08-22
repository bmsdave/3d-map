import { createMap, createTilePackageLoader, TRAFALGAR_MANIFEST, type MapHandle } from "../sdk";
import { controlRow, el, readout, section } from "../ui";
import type { CardSpec } from "./types";
import { attachNavigation } from "./navigation";

const MANIFEST = TRAFALGAR_MANIFEST;



export const packageLoader: CardSpec = {
  id: "package-loader",
  title: "Пакет: загрузка по спросу",
  purpose:
    "Версионированный MT2 manifest задаёт камеру, уровни и список тайлов; хост подгружает только покрытие видимой области.",
  group: "Пакеты",
  mount(stage, panel) {
    let navCleanup: (() => void) | null = null;
    const out = readout([
      { key: "package-tiles", label: "загружено из пакета" },
      { key: "package-unloaded", label: "выгружено из памяти" },
      { key: "package-level", label: "уровень пакета (SDK)" },
      { key: "package-missing", label: "вне покрытия" },
      { key: "package-attribution", label: "атрибуция источника" },
    ]);
    let manifestUrl = MANIFEST;
    const input = el("input", {
      type: "url",
      value: manifestUrl,
      "data-testid": "package-manifest-url",
      "aria-label": "URL manifest пакета",
    });
    const load = el("button", { type: "button" }, ["Загрузить пакет"]);
    const tilt = el("input", {
      type: "range",
      min: "0",
      max: "60",
      step: "1",
      value: "0",
      "data-testid": "package-tilt-slider",
      "aria-label": "Наклон карты",
    });
    const source = section("Источник пакета", el("div", {}, [
      controlRow("Manifest URL", input),
      load,
      controlRow("Наклон", tilt),
    ]));
    let generation = 0;
    let recoveries = 0;
    let activeMap: MapHandle | null = null;
    const applyTilt = () => {
      if (!activeMap) return;
      activeMap.setTilt(Number(tilt.value));
      activeMap.render();
      stage.setAttribute("data-tilt", activeMap.debug().tilt.toFixed(1));
    };
    const showError = (error: unknown) => {
      const retry = el("button", { type: "button" }, ["Повторить"]);
      retry.addEventListener("click", () => void loadPackage());
      stage.setAttribute("data-state", "error");
      stage.replaceChildren(String(error), retry);
    };
    const loadPackage = async () => {
      const request = ++generation;
      const canvas = el("canvas", { width: "720", height: "480" });
      stage.setAttribute("data-state", "loading");
      stage.removeAttribute("data-loaded");
      stage.removeAttribute("data-unloaded");
      stage.removeAttribute("data-manifest");
      stage.replaceChildren(canvas);
      activeMap = null;
      try {
        const map = await createMap(canvas);
        const loader = await createTilePackageLoader(map, manifestUrl);
        if (request !== generation) return;
        const view = loader.manifest.view;
        map.setCentre(view.lon, view.lat);
        map.setZoom(view.zoom);
        let loaded = 0;
        let unloaded = 0;
        const refresh = async () => {
          const result = await loader.loadVisible();
          if (request !== generation) return;
          loaded += result.loaded;
          unloaded += result.unloaded;
          out.set("package-tiles", String(loaded));
          out.set("package-unloaded", String(unloaded));
          out.set("package-missing", String(result.unavailable));
          stage.setAttribute("data-loaded", String(loaded));
          stage.setAttribute("data-unloaded", String(unloaded));
          stage.setAttribute("data-unavailable", String(result.unavailable));
        };
        await refresh();
        if (request !== generation) return;
        const state = map.debug();
        activeMap = map;
        applyTilt();
        out.set("package-level", String(state.tile_level));
        out.set("package-attribution", loader.manifest.sources.map((source) => source.attribution).join(" · "));
        panel.replaceChildren(source, section("Пакет", out.root));
        stage.setAttribute("data-manifest", manifestUrl);
        stage.setAttribute("data-state", "ready");
        navCleanup = attachNavigation(canvas, map, () => void refresh());
        canvas.addEventListener("webglcontextlost", (event) => {
          event.preventDefault();
          recoveries += 1;
          stage.setAttribute("data-recoveries", String(recoveries));
          void loadPackage();
        });
      } catch (error) {
        if (request === generation) showError(error);
      }
    };
    load.addEventListener("click", () => {
      manifestUrl = input.value.trim();
      void loadPackage();
    });
    tilt.addEventListener("input", applyTilt);
    panel.append(source);
    void loadPackage();
    return () => {
      generation++;
      try { navCleanup?.(); } catch {}
    };
  },
};
