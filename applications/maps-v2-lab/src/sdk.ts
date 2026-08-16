// Подключение SDK к карточкам: одна инициализация wasm на страницу,
// фикстуры тянет хост (карточка), тайлы отдаются в Map один раз.

import init, { Map as SdkMap } from "./generated/maps2-web/maps2_web.js";

export interface DebugState {
  zoom: number;
  centre_lon: number;
  centre_lat: number;
  bearing: number;
  /** Камера ещё едет сама: инерция или анимация зума. */
  moving: boolean;
  band: string;
  composition: string;
  override: boolean;
  settled: boolean;
  tile_level: number;
  resident_classes: string[];
  tiles_drawn: number;
  draw_calls: number;
  resident_gpu: number;
  evictions: number;
  casing: boolean;
  miter_limit: number;
  joins: { miter: number; bevel: number };
  road_widths: Record<string, number>;
  tilt: number;
  label_candidates: number;
  labels_placed: number;
  labels_rejected: number;
  label_occupancy: number;
  label_budget: number;
  /** Форма, в которой рендерер только что нарисовал кадр. */
  shape: "flat" | "blend" | "globe";
  globeness: number;
  relief: boolean;
  hypsometric: boolean;
  expressiveness: number;
  exaggeration: number;
  /** Сколько резидентных тайлов держат текстуру высот. */
  height_tiles: number;
  /** Высота под центром камеры, метры; null — тайл без высот. */
  centre_height_m: number | null;
}

// Одна судьба одного кандидата в подписи. Карточки и e2e читают именно
// это: видимость подписи — свойство кадра, а не фичи, поэтому
// проверяется список и его инварианты, а не «подпись X на экране».
export type LabelState =
  | "placed"
  | "collision"
  | "budget"
  | "offscreen"
  | "duplicate";

export interface LabelEntry {
  id: string;
  rank: number;
  class: string;
  state: LabelState;
  blocked_by: string | null;
  text: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface MapHandle {
  setZoom(zoom: number): void;
  setSourceLevels(levels: number[]): void;
  missingTiles(): string[];
  loadTile(bytes: Uint8Array): void;
  unloadTile(z: number, x: number, y: number): void;
  setCentre(lon: number, lat: number): void;
  setBandOverride(band: string | null): void;
  setTransitionAnimated(animated: boolean): void;
  setRoadCasing(on: boolean): void;
  setMiterLimit(limit: number): void;
  setRoadWidthPx(className: string, px: number): void;
  samplePixel(x: number, y: number): [number, number, number, number];
  setLabelBudget(budget: number): void;
  setHaloEm(halo: number): void;
  setCollisionBoxes(on: boolean): void;
  setSpecimen(text: string | null): void;
  setBearing(degrees: number): void;
  setTilt(degrees: number): void;
  setRelief(on: boolean): void;
  setHypsometric(on: boolean): void;
  setReliefExpressiveness(value: number): void;
  setReliefExaggeration(value: number): void;
  globeness(): number;
  render(): void;
  /** On-demand benchmark for the lab and its browser performance contract. */
  measureFrames(samples: number): number;
  debug(): DebugState;
  labelDebug(): LabelEntry[];
  pointerDown(x: number, y: number, nowMs: number): void;
  pointerMove(x: number, y: number, nowMs: number): boolean;
  pointerUp(): void;
  wheel(x: number, y: number, deltaY: number, pinch: boolean): void;
  doubleClick(x: number, y: number): void;
  /** Отвечает, был ли клавишный жест нашим: только тогда хост его гасит. */
  key(name: string): boolean;
  /** Кадр инерции или анимации зума; отвечает, едет ли камера дальше. */
  tick(dtMs: number): boolean;
}

// Где стоит камера, чтобы увидеть пакет. Пишется генератором фикстур —
// проекцию Меркатора не повторяем в TypeScript.
export interface PackCentre {
  lon: number;
  lat: number;
  zoom: number;
}

export interface PackageSource {
  name: string;
  attribution: string;
  licence: string;
}

export interface TilePackageManifest {
  format: "MT2";
  format_version: number;
  tiles: string[];
  tile_digests: Record<string, string>;
  view: PackCentre;
  sources: PackageSource[];
}

export interface PackageLoadResult {
  loaded: number;
  unavailable: number;
}

export interface TilePackageLoader {
  manifest: TilePackageManifest;
  loadVisible(): Promise<PackageLoadResult>;
}

const MAX_PACKAGE_TILES = 50_000;
const MAX_TILE_BYTES = 4 * 1024 * 1024;

export async function loadPackCentre(pack: string): Promise<PackCentre> {
  const response = await fetch(`/fixtures/${pack}/centre.json`);
  return (await response.json()) as PackCentre;
}

let wasmReady: Promise<unknown> | null = null;

async function loadFixtureTiles(map: InstanceType<typeof SdkMap>, pack: string): Promise<void> {
  const manifest = (await (await fetch(`/fixtures/${pack}/manifest.json`)).json()) as string[];
  await Promise.all(
    manifest.map(async (path) => {
      const response = await fetch(`/fixtures/${pack}/${path}.mt2`);
      map.load_tile(new Uint8Array(await response.arrayBuffer()));
    }),
  );
}

function isTilePackageManifest(value: unknown): value is TilePackageManifest {
  if (!value || typeof value !== "object") return false;
  const manifest = value as Partial<TilePackageManifest>;
  return manifest.format === "MT2"
    && (manifest.format_version === 2 || manifest.format_version === 3 || manifest.format_version === 4)
    && Array.isArray(manifest.tiles)
    && manifest.tiles.every((path) => typeof path === "string" && /^\d+\/\d+\/\d+\.mt2$/.test(path))
    && !!manifest.tile_digests
    && typeof manifest.tile_digests === "object"
    && manifest.tiles.every((path) => typeof manifest.tile_digests![path] === "string" && /^[0-9a-f]{64}$/.test(manifest.tile_digests![path]!))
    && !!manifest.view
    && Number.isFinite(manifest.view.lon)
    && Number.isFinite(manifest.view.lat)
    && Number.isFinite(manifest.view.zoom)
    && Array.isArray(manifest.sources);
}

async function fetchPackageManifest(url: string): Promise<TilePackageManifest> {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`cannot load package manifest: ${response.status}`);
  const value: unknown = await response.json();
  if (hasTooManyTiles(value)) throw new Error(`package exceeds ${MAX_PACKAGE_TILES} tiles`);
  if (!isTilePackageManifest(value)) throw new Error("invalid MT2 package manifest");
  return value;
}

function hasTooManyTiles(value: unknown): boolean {
  return !!value && typeof value === "object" && Array.isArray((value as { tiles?: unknown }).tiles)
    && (value as { tiles: unknown[] }).tiles.length > MAX_PACKAGE_TILES;
}

function packageLevels(manifest: TilePackageManifest): number[] {
  return [...new Set(manifest.tiles.map((path) => Number(path.split("/")[0])))].sort((a, b) => a - b);
}

async function fetchTile(url: string, digest: string): Promise<Uint8Array> {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`cannot load package tile: ${response.status}`);
  const length = Number(response.headers.get("content-length"));
  if (Number.isFinite(length) && length > MAX_TILE_BYTES) throw new Error("package tile exceeds 4 MiB");
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.byteLength > MAX_TILE_BYTES) throw new Error("package tile exceeds 4 MiB");
  if (await sha256(bytes) !== digest) throw new Error("package tile checksum mismatch");
  return bytes;
}

async function sha256(bytes: Uint8Array): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", Uint8Array.from(bytes));
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

/**
 * Connects a versioned MT2 package to an existing map. The caller sets the
 * package view, then calls `loadVisible` after camera changes. Network policy
 * stays in the host instead of leaking into the render loop.
 */
export async function createTilePackageLoader(map: MapHandle, manifestUrl: string): Promise<TilePackageLoader> {
  const manifest = await fetchPackageManifest(manifestUrl);
  const base = new URL(manifestUrl, window.location.href);
  const paths = new Map(manifest.tiles.map((path) => [path, new URL(path, base).toString()]));
  const loaded = new Set<string>();
  map.setSourceLevels(packageLevels(manifest));
  return {
    manifest,
    async loadVisible(): Promise<PackageLoadResult> {
      const requested = map.missingTiles();
      const available = requested.filter((path) => paths.has(path) && !loaded.has(path));
      const unavailable = requested.length - available.length;
      const bytes = await Promise.all(available.map(async (path) => [path, await fetchTile(paths.get(path)!, manifest.tile_digests[path]!)] as const));
      for (const [path, tile] of bytes) {
        map.loadTile(tile);
        loaded.add(path);
      }
      map.render();
      return { loaded: bytes.length, unavailable };
    },
  };
}

declare global {
  interface Window {
    // Ручка последней созданной карты. Лаба — и есть отладочная
    // поверхность: отсюда её дёргает консоль браузера и e2e, когда
    // проверяет пиксели пробой.
    maps2?: MapHandle;
  }
}

let nextCanvasId = 0;

interface PackageMapApi {
  set_source_levels(levels: Uint8Array): void;
  missing_tiles(): string;
  unload_tile(z: number, x: number, y: number): void;
}

function packageMapApi(map: InstanceType<typeof SdkMap>): PackageMapApi {
  return map as unknown as PackageMapApi;
}

function p95RenderMs(map: InstanceType<typeof SdkMap>, samples: number): number {
  const durations = Array.from({ length: samples }, () => {
    const started = performance.now();
    map.render();
    return performance.now() - started;
  }).sort((left, right) => left - right);
  return durations[Math.ceil(durations.length * 0.95) - 1] ?? Number.POSITIVE_INFINITY;
}

export async function createMap(canvas: HTMLCanvasElement, pack: string | null): Promise<MapHandle> {
  if (!canvas.id) {
    nextCanvasId += 1;
    canvas.id = `sdk-canvas-${nextCanvasId}`;
  }
  wasmReady ??= init();
  await wasmReady;
  const map = new SdkMap(canvas.id);
  const packageApi = packageMapApi(map);
  if (pack) await loadFixtureTiles(map, pack);
  const handle: MapHandle = {
    setZoom: (zoom) => map.set_zoom(zoom),
    setSourceLevels: (levels) => packageApi.set_source_levels(new Uint8Array(levels)),
    missingTiles: () => JSON.parse(packageApi.missing_tiles()) as string[],
    loadTile: (bytes) => map.load_tile(bytes),
    unloadTile: (z, x, y) => packageApi.unload_tile(z, x, y),
    setCentre: (lon, lat) => map.set_centre(lon, lat),
    setBandOverride: (band) => map.set_band_override(band ?? undefined),
    setTransitionAnimated: (animated) => map.set_transition_animated(animated),
    setRoadCasing: (on) => map.set_road_casing(on),
    setMiterLimit: (limit) => map.set_miter_limit(limit),
    setRoadWidthPx: (className, px) => map.set_road_width_px(className, px),
    samplePixel: (x, y) =>
      map.sample_pixel(x, y).split(",").map(Number) as [number, number, number, number],
    setLabelBudget: (budget) => map.set_label_budget(budget),
    setHaloEm: (halo) => map.set_halo_em(halo),
    setCollisionBoxes: (on) => map.set_collision_boxes(on),
    setSpecimen: (text) => map.set_specimen(text ?? undefined),
    setBearing: (degrees) => map.set_bearing(degrees),
    setTilt: (degrees) => map.set_tilt(degrees),
    setRelief: (on) => map.set_relief(on),
    setHypsometric: (on) => map.set_hypsometric(on),
    setReliefExpressiveness: (value) => map.set_relief_expressiveness(value),
    setReliefExaggeration: (value) => map.set_relief_exaggeration(value),
    globeness: () => map.globeness(),
    render: () => map.render(),
    measureFrames: (samples) => p95RenderMs(map, samples),
    debug: () => JSON.parse(map.debug()) as DebugState,
    labelDebug: () => JSON.parse(map.label_debug()) as LabelEntry[],
    pointerDown: (x, y, nowMs) => map.pointer_down(x, y, nowMs),
    pointerMove: (x, y, nowMs) => map.pointer_move(x, y, nowMs),
    pointerUp: () => map.pointer_up(),
    wheel: (x, y, deltaY, pinch) => map.wheel(x, y, deltaY, pinch),
    doubleClick: (x, y) => map.double_click(x, y),
    key: (name) => map.key(name),
    tick: (dtMs) => map.tick(dtMs),
  };
  window.maps2 = handle;
  return handle;
}

/** Гоняет кадры, пока композиция не устаканится (или не выйдет лимит). */
export function renderUntilSettled(map: MapHandle, onFrame?: () => void): void {
  const started = performance.now();
  const tick = () => {
    map.render();
    onFrame?.();
    if (!map.debug().settled && performance.now() - started < 2000) {
      requestAnimationFrame(tick);
    }
  };
  requestAnimationFrame(tick);
}
