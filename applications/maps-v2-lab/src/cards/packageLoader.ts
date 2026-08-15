import { createMap, createTilePackageLoader } from "../sdk";
import { el, readout, section } from "../ui";
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
    const canvas = el("canvas", { width: "720", height: "480" });
    stage.replaceChildren(canvas);

    createMap(canvas, null)
      .then(async (map) => {
        const loader = await createTilePackageLoader(map, MANIFEST);
        const view = loader.manifest.view;
        map.setCentre(view.lon, view.lat);
        map.setZoom(view.zoom);
        const result = await loader.loadVisible();
        const state = map.debug();
        out.set("package-tiles", String(result.loaded));
        out.set("package-level", String(state.tile_level));
        out.set("package-missing", String(result.unavailable));
        out.set("package-attribution", loader.manifest.sources.map((source) => source.attribution).join(" · "));
        panel.append(section("Пакет", out.root));
        stage.setAttribute("data-loaded", String(result.loaded));
        stage.setAttribute("data-state", "ready");
      })
      .catch((error: unknown) => {
        stage.setAttribute("data-state", "error");
        stage.replaceChildren(String(error));
      });
  },
};
