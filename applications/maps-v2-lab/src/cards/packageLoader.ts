import { createMap, createTilePackageLoader } from "../sdk";
import { controlRow, el, readout, section } from "../ui";
import type { CardSpec } from "./types";

const MANIFEST = "/fixtures/ealing/package-manifest.json";

export const packageLoader: CardSpec = {
  id: "package-loader",
  title: "Пакет: загрузка по спросу",
  purpose:
    "Версионированный MT2 manifest задаёт камеру, уровни и список тайлов; хост подгружает только покрытие видимой области.",
  group: "Пакеты",
  mount(stage, panel) {
    const out = readout([
      { key: "package-tiles", label: "загружено из пакета" },
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
    const source = section("Источник пакета", el("div", {}, [
      controlRow("Manifest URL", input),
      load,
    ]));
    let generation = 0;
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
      stage.removeAttribute("data-manifest");
      stage.replaceChildren(canvas);
      try {
        const map = await createMap(canvas, null);
        const loader = await createTilePackageLoader(map, manifestUrl);
        if (request !== generation) return;
        const view = loader.manifest.view;
        map.setCentre(view.lon, view.lat);
        map.setZoom(view.zoom);
        const result = await loader.loadVisible();
        if (request !== generation) return;
        const state = map.debug();
        out.set("package-tiles", String(result.loaded));
        out.set("package-level", String(state.tile_level));
        out.set("package-missing", String(result.unavailable));
        out.set("package-attribution", loader.manifest.sources.map((source) => source.attribution).join(" · "));
        panel.replaceChildren(source, section("Пакет", out.root));
        stage.setAttribute("data-loaded", String(result.loaded));
        stage.setAttribute("data-manifest", manifestUrl);
        stage.setAttribute("data-state", "ready");
      } catch (error) {
        if (request === generation) showError(error);
      }
    };
    load.addEventListener("click", () => {
      manifestUrl = input.value.trim();
      void loadPackage();
    });
    panel.append(source);
    void loadPackage();
  },
};
