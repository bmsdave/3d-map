// Подключение SDK к карточкам: одна инициализация wasm на страницу,
// фикстуры тянет хост (карточка), тайлы отдаются в Map один раз.

import init, { Map as SdkMap } from "./generated/maps2-web/maps2_web.js";
import { recordSpan, traced, tracedAsync } from "./perfTrace";

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
  /** Where the last frame's milliseconds went, measured in the renderer. */
  frame_ms: number;
  residency_ms: number;
  ground_ms: number;
  buildings_ms: number;
  fills_ms: number;
  roads_ms: number;
  labels_ms: number;
  /** Residency plans the frame computed. More than one means a caller
   *  asked the same question twice. */
  plans: number;
  /** Tiles the host is holding, and their bytes. */
  cpu_tiles: number;
  cpu_bytes: number;
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
  addSourceLevels(levels: number[]): void;
  missingTiles(): string[];
  prefetchTiles(): string[];
  fallbackTiles(): string[];
  evictableTiles(): string[];
  /** All four demand lists from one residency plan. */
  tileDemand(): TileDemand;
  loadTile(bytes: Uint8Array): void;
  unloadTile(z: number, x: number, y: number): void;
  setCentre(lon: number, lat: number): void;
  /** Tell the map the canvas has a new backing-store size. The host
   *  resizes the canvas; the map plans and draws for whatever size it
   *  was last told about. */
  setViewport(width: number, height: number): void;
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

// Где стоит камера, чтобы увидеть пакет. Пишется тем, кто пакет собрал, —
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
  /** Уровни, которые пакет несёт. Единственный источник этого списка,
   *  когда тайлы не перечислены поимённо. */
  levels: number[];
  /** Сколько тайлов в пакете — есть и тогда, когда их имён нет. */
  tile_count: number;
  /** Absent on a package too large to name every tile: the client then
   *  computes tile URLs and reads `bounds` to know where to look. */
  tiles?: string[];
  tile_digests?: Record<string, string>;
  view: PackCentre;
  sources: PackageSource[];
  /** Земля, за которую пакет отвечает, по уровням. Пишет
   *  `maps2-ingest carve`; у пакета на весь мир её нет — там некуда
   *  выехать. */
  bounds?: Record<string, LevelBox>;
}

/** Прямоугольник в градусах, как его записала вырезка. */
export interface LevelBox {
  west: number;
  south: number;
  east: number;
  north: number;
}

/** What the renderer wants, answered by a single residency plan. */
export interface TileDemand {
  missing: string[];
  prefetch: string[];
  fallback: string[];
  evictable: string[];
}

export interface PackageLoadResult {
  loaded: number;
  unloaded: number;
  unavailable: number;
  /** Main-thread milliseconds this pass spent, excluding awaited I/O.
   *  A slow network and a slow map look identical from the outside and
   *  are fixed in completely different places. */
  blockedMs: number;
}

export interface TilePackageLoader {
  manifest: TilePackageManifest;
  loadVisible(): Promise<PackageLoadResult>;
}

/// How long a single pass may hold the main thread decoding tiles
/// before letting the page breathe. Decoding a tile builds every mesh
/// and shapes every label in it, so a batch arriving together — which
/// is what a zoom does — blocked for most of a second in one go.
// CPU buckets (fills/buildings/lines/labels) + heights unpack live in `maps2-web/src/decode.rs` —
// `load_tile` is Worker-eligible; host still slices *between* tiles here. Worker move is step 2.
const DECODE_SLICE_MS = 6;

const MAX_PACKAGE_TILES = 50_000;
const MAX_TILE_BYTES = 4 * 1024 * 1024;

let wasmReady: Promise<unknown> | null = null;

function isTilePackageManifest(value: unknown): value is TilePackageManifest {
  if (!value || typeof value !== "object") return false;
  const manifest = value as Partial<TilePackageManifest>;
  const version = manifest.format_version;
  return manifest.format === "MT2"
    && typeof version === "number" && Number.isInteger(version) && version >= 2 && version <= 6
    && isCoverage(manifest)
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

/**
 * How a manifest says which tiles it has.
 *
 * A carve names them, with a digest each: the client knows before asking
 * whether a tile exists, and a corrupt tile is caught on arrival. A
 * planet cannot — the list alone would be gigabytes — so it carries
 * `levels` and a per-level `bounds` box instead, and a tile outside the
 * box is not asked for while one inside that turns out not to exist
 * answers 404, which is a normal thing for a tile server to say.
 *
 * One or the other, and never neither: a manifest with no way to tell
 * where its ground is would have the client guessing at the whole
 * planet.
 */
function isCoverage(manifest: Partial<TilePackageManifest>): boolean {
  const listed = Array.isArray(manifest.tiles)
    && manifest.tiles.every((path) => typeof path === "string" && /^\d+\/\d+\/\d+\.mt2$/.test(path))
    && !!manifest.tile_digests
    && typeof manifest.tile_digests === "object"
    && manifest.tiles.every((path) =>
      typeof manifest.tile_digests![path] === "string"
      && /^[0-9a-f]{64}$/.test(manifest.tile_digests![path]!));
  const bounded = Array.isArray(manifest.levels)
    && manifest.levels.length > 0
    && manifest.levels.every((level) => Number.isInteger(level))
    && !!manifest.bounds
    && manifest.levels.some((level) => !!manifest.bounds![String(level)]);
  return listed || bounded;
}

function hasTooManyTiles(value: unknown): boolean {
  return !!value && typeof value === "object" && Array.isArray((value as { tiles?: unknown }).tiles)
    && (value as { tiles: unknown[] }).tiles.length > MAX_PACKAGE_TILES;
}

function packageLevels(manifest: TilePackageManifest): number[] {
  const listed = manifest.tiles?.map((path) => Number(path.split("/")[0])) ?? manifest.levels;
  return [...new Set(listed)].sort((a, b) => a - b);
}

async function fetchTile(url: string, digest?: string): Promise<Uint8Array> {
  try {
    return await fetchTileOnce(url, digest);
  } catch (error) {
    if (!isRetryableTileError(error)) throw error;
    return fetchTileOnce(url, digest);
  }
}

async function fetchTileOnce(url: string, digest?: string): Promise<Uint8Array> {
  const response = await tracedAsync("fetch", () => fetch(url));
  if (!response.ok) throw new TileResponseError(response.status);
  const length = Number(response.headers.get("content-length"));
  if (Number.isFinite(length) && length > MAX_TILE_BYTES) throw new Error("package tile exceeds 4 MiB");
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.byteLength > MAX_TILE_BYTES) throw new Error("package tile exceeds 4 MiB");
  // Checked when the manifest says what to expect. A package too large
  // to carry a digest per tile is trusted to the transport instead —
  // `maps2-ingest verify-package` checks those bytes where they are
  // built, rather than on every machine that draws them.
  if (digest !== undefined && await tracedAsync("digest", () => sha256(bytes)) !== digest) {
    throw new Error("package tile checksum mismatch");
  }
  return bytes;
}

class TileResponseError extends Error {
  constructor(readonly status: number) {
    super(`cannot load package tile: ${status}`);
  }
}

function isRetryableTileError(error: unknown): boolean {
  return error instanceof TypeError || error instanceof TileResponseError && (error.status === 429 || error.status >= 500);
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
export async function createTilePackageLoader(
  map: MapHandle,
  manifestUrl: string,
  options: { additive?: boolean; active?: () => boolean } = {},
): Promise<TilePackageLoader> {
  const manifest = await fetchPackageManifest(manifestUrl);
  const base = new URL(manifestUrl, window.location.href);
  const listed = manifest.tiles
    ? new Map(manifest.tiles.map((path) => [path, new URL(path, base).toString()]))
    : null;
  // Paths a fetch has already answered 404 for. A package that does not
  // list its tiles cannot say in advance which of them exist, and without
  // remembering the misses every camera move would ask again for the same
  // empty ocean.
  const absent = new Set<string>();
  const urlOf = (path: string): string => listed?.get(path) ?? new URL(path, base).toString();
  const covers = (path: string): boolean =>
    !absent.has(path) && (listed ? listed.has(path) : withinPackageBounds(manifest, path));
  const loaded = new Set<string>();
  const inFlight = new Set<string>();
  // A second package composed onto the same map (e.g. a low-zoom world
  // package under a high-zoom regional one) must add its levels rather
  // than replace the first package's — see addSourceLevels's doc comment
  // in maps2-web for why a replace silently mis-plans residency.
  if (options.additive) {
    map.addSourceLevels(packageLevels(manifest));
  } else {
    map.setSourceLevels(packageLevels(manifest));
  }
  return {
    manifest,
    async loadVisible(): Promise<PackageLoadResult> {
      // Загрузка карты, которую уже никто не смотрит, — не бесплатная
      // фоновая работа: декодирование тайла строит все его меши и
      // раскладывает все подписи. Шесть студий доски, доигрывающих
      // свой проход после перехода, отнимали процессор ровно у той
      // студии, которую человек только что открыл.
      const wanted = options.active ?? (() => true);
      if (!wanted()) return { loaded: 0, unloaded: 0, unavailable: 0, blockedMs: 0 };
      const passStarted = performance.now();
      // One residency plan answers all four questions. Asking them
      // separately cost a plan each, and this runs on every pointer
      // move: a single drag was computing nine plans per event.
      let blockedMs = 0;
      const blocking = <T>(work: () => T): T => {
        const started = performance.now();
        const value = work();
        blockedMs += performance.now() - started;
        return value;
      };
      const demand = blocking(() => traced("demand", () => map.tileDemand()));
      // Only ever unload this package's own tiles. The evictable list
      // speaks for the whole map, and several packages compose onto one
      // map — a world package dropping the city package's tiles would
      // leave that loader still believing they were resident, so it
      // would never refetch them and the city would never come back.
      const evicted = demand.evictable.filter((path) => loaded.has(path));
      blocking(() => {
        for (const path of evicted) {
          unloadPackageTile(map, path);
          loaded.delete(path);
        }
      });
      // Missing tiles are wanted on screen now; prefetch tiles are a level
      // deeper the camera hasn't reached yet (see prefetch_tiles's doc
      // comment in maps2-web) — fetching both here means a package
      // boundary's deeper level is already resident by the time the
      // camera's zoom actually crosses it, instead of popping in.
      // Missing tiles are wanted on screen now; fallback tiles are the
      // coarser ancestors that stand in wherever this package has no
      // coverage at all; prefetch tiles are a level deeper the camera
      // hasn't reached yet (see the doc comments on prefetch_tiles and
      // fallback_tiles in maps2-web). All three are fetched here, but
      // only a genuinely missing tile counts as a hole in the map —
      // a package legitimately carries no tile at most of these paths.
      const { missing, fallback, prefetch } = demand;
      const requested = [...missing, ...fallback, ...prefetch];
      const available = requested.filter((path) => covers(path) && !loaded.has(path) && !inFlight.has(path));
      const unavailable = missing.filter((path) => !covers(path)).length;
      available.forEach((path) => inFlight.add(path));
      let bytes: readonly (readonly [string, Uint8Array])[] = [];
      try {
        // A 404 is a fact about the package, not a failure of the pass:
        // an envelope covers ground the build had nothing to say about.
        // It is remembered and skipped, and the tiles that did arrive are
        // still decoded — one absent tile must not throw away the batch
        // it travelled with.
        const arrivals = await Promise.all(available.map(async (path) => {
          try {
            return [path, await fetchTile(urlOf(path), manifest.tile_digests?.[path])] as const;
          } catch (error) {
            if (error instanceof TileResponseError && error.status === 404) {
              absent.add(path);
              return null;
            }
            throw error;
          }
        }));
        bytes = arrivals.filter((arrival) => arrival !== null);
      } finally {
        available.forEach((path) => inFlight.delete(path));
      }
      const after = blocking(() => traced("demand", () => map.tileDemand()));
      const stillWanted = new Set([...after.missing, ...after.fallback, ...after.prefetch]);
      let loadedNow = 0;
      // Decode in slices, yielding between them. The work is the same;
      // spreading it means a zoom that pulls a dozen tiles no longer
      // freezes the page while they are all turned into meshes.
      let sliceStart = performance.now();
      for (const [path, tile] of bytes) {
        if (!wanted()) break;
        if (!stillWanted.has(path)) continue;
        blocking(() => {
          traced("decode", () => map.loadTile(tile), path);
          loaded.add(path);
          loadedNow += 1;
        });
        if (performance.now() - sliceStart >= DECODE_SLICE_MS) {
          await new Promise((resume) => setTimeout(resume, 0));
          sliceStart = performance.now();
        }
      }
      // A repaint is only owed when the frame would differ. The caller
      // has already drawn this camera; loading nothing changes nothing.
      if (loadedNow > 0 || evicted.length > 0) blocking(() => map.render());
      // Проход уже посчитал своё заблокированное время — второй
      // секундомер поверх первого мерил бы обёртку, а не работу.
      recordSpan("pass", performance.now() - passStarted);
      recordSpan("blocked", blockedMs);
      return { loaded: loadedNow, unloaded: evicted.length, unavailable, blockedMs };
    },
  };
}

function unloadPackageTile(map: MapHandle, path: string): void {
  const match = /^(\d+)\/(\d+)\/(\d+)\.mt2$/.exec(path);
  if (!match) return;
  map.unloadTile(Number(match[1]), Number(match[2]), Number(match[3]));
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
  add_source_levels(levels: Uint8Array): void;
  missing_tiles(): string;
  prefetch_tiles(): string;
  fallback_tiles(): string;
  evictable_tiles(): string;
  tile_demand(): string;
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

export async function createMap(canvas: HTMLCanvasElement): Promise<MapHandle> {
  if (!canvas.id) {
    nextCanvasId += 1;
    canvas.id = `sdk-canvas-${nextCanvasId}`;
  }
  wasmReady ??= init();
  await wasmReady;
  const map = new SdkMap(canvas.id);
  const packageApi = packageMapApi(map);
  const handle: MapHandle = {
    setZoom: (zoom) => map.set_zoom(zoom),
    setSourceLevels: (levels) => packageApi.set_source_levels(new Uint8Array(levels)),
    addSourceLevels: (levels) => packageApi.add_source_levels(new Uint8Array(levels)),
    missingTiles: () => JSON.parse(packageApi.missing_tiles()) as string[],
    prefetchTiles: () => JSON.parse(packageApi.prefetch_tiles()) as string[],
    fallbackTiles: () => JSON.parse(packageApi.fallback_tiles()) as string[],
    tileDemand: () => JSON.parse(packageApi.tile_demand()) as TileDemand,
    evictableTiles: () => JSON.parse(packageApi.evictable_tiles()) as string[],
    loadTile: (bytes) => map.load_tile(bytes),
    unloadTile: (z, x, y) => packageApi.unload_tile(z, x, y),
    setCentre: (lon, lat) => map.set_centre(lon, lat),
    setViewport: (width, height) => map.set_viewport(width, height),
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
    render: () => traced("render", () => map.render()),
    measureFrames: (samples) => p95RenderMs(map, samples),
    debug: () => traced("debug", () => JSON.parse(map.debug()) as DebugState),
    labelDebug: () => traced("labels", () => JSON.parse(map.label_debug()) as LabelEntry[]),
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

// Трафальгарская площадь: одно место на всю лабораторию. Пакет вырезан
// вокруг неё (`maps2-ingest carve`), и каждая студия смотрит на него.
export const TRAFALGAR = { lon: -0.1281, lat: 51.508 };
export const TRAFALGAR_MANIFEST = "/packages/trafalgar/manifest.json";
/** Только городские уровни той же площади, отдельным пакетом. Нужен
 *  ровно одной студии — той, что складывает два пакета на одной карте;
 *  композиция из одного пакета ничего не показывает. */
export const TRAFALGAR_CITY_MANIFEST = "/packages/trafalgar-city/manifest.json";

export interface PackageMap {
  /** Ручка карты, которая сама досылает тайлы под новую камеру. */
  map: MapHandle;
  loader: TilePackageLoader;
  /** Ждать, когда нужен детерминированный кадр: загрузка под текущую
   *  камеру уже прошла, а не запланирована. */
  refresh(): Promise<void>;
  /** Позвать, когда догрузка что-то принесла. Показания карточки
   *  снимаются с кадра, а кадр под новой камерой доезжает позже
   *  движения камеры — без этого в readout остаётся прочерк. */
  onSettled(listener: () => void): void;
}

/**
 * Карта на вырезанном пакете: демонд-загрузка вместо «скачать пакет
 * целиком».
 *
 * Фикстурный путь удалён — синтетика теперь только через `public/fixtures` напрямую.
 * Здесь работает тот же загрузчик, что и в карточках реальных данных.
 *
 * Карточке не приходится знать про загрузку: движение камеры само
 * тянет за собой догрузку и возврат камеры на землю, которую пакет
 * умеет ответить. Кто хочет дождаться — ждёт `refresh`.
 */
export async function createPackageMap(
  canvas: HTMLCanvasElement,
  options: { zoom: number; centre?: { lon: number; lat: number }; manifestUrl?: string },
): Promise<PackageMap> {
  const raw = await createMap(canvas);
  const loader = await createTilePackageLoader(raw, options.manifestUrl ?? TRAFALGAR_MANIFEST, {
    active: () => canvas.isConnected,
  });
  const centre = options.centre ?? TRAFALGAR;
  raw.setCentre(centre.lon, centre.lat);
  raw.setZoom(options.zoom);

  // Один проход за раз. События камеры приходят много быстрее, чем
  // успевает загрузка, и запуск нового прохода на каждое событие
  // складывал параллельные проходы по одной и той же камере. Отвечать
  // стоит только последней — запрос, пришедший на лету, просто метит
  // ответ устаревшим (тот же приём, что в карточке map-real).
  const listeners: (() => void)[] = [];
  let refreshing = false;
  let stale = false;
  const refresh = async (): Promise<void> => {
    // Карта, чей холст уже не в документе, ничего не покажет — а
    // качать продолжает. Доска держит шесть живых студий и роняет их
    // контексты при переходе на страницу одной: без этого шесть
    // загрузок продолжали отбирать канал у той, которую человек
    // только что открыл.
    if (!canvas.isConnected) return;
    if (refreshing) {
      stale = true;
      return;
    }
    refreshing = true;
    let arrived = 0;
    try {
      do {
        stale = false;
        arrived += (await loader.loadVisible()).loaded;
      } while (stale && canvas.isConnected);
    } finally {
      refreshing = false;
    }
    if (arrived > 0) for (const listener of listeners) listener();
  };

  // Вырезка конечна, и у неё есть край. Камеру, уехавшую за него,
  // возвращает не рендерер — он честно покажет дыру, — а хост: это
  // его пакет и его решение, куда можно смотреть.
  const clamp = (): void => traced("clamp", () => {
    const state = raw.debug();
    const box = levelBox(loader.manifest, state.tile_level);
    if (!box) return;
    const lon = Math.min(Math.max(state.centre_lon, box.west), box.east);
    const lat = Math.min(Math.max(state.centre_lat, box.south), box.north);
    if (lon !== state.centre_lon || lat !== state.centre_lat) raw.setCentre(lon, lat);
  });

  // Камера успевает измениться несколько раз за кадр (шоукейс двигает
  // зум, курс и наклон подряд), а спросить у карты, где она стоит, —
  // это разбор JSON отладочного состояния. Поэтому проверка границ и
  // догрузка откладываются на конец такта и схлопываются в одну.
  let settling = false;
  const settle = (): void => {
    if (settling) return;
    settling = true;
    queueMicrotask(() => {
      settling = false;
      clamp();
      void refresh();
    });
  };
  const after = <A extends unknown[], R>(call: (...args: A) => R) => (...args: A): R => {
    const value = call(...args);
    settle();
    return value;
  };
  const map: MapHandle = {
    ...raw,
    setZoom: after(raw.setZoom),
    setViewport: after(raw.setViewport),
    setCentre: after(raw.setCentre),
    setTilt: after(raw.setTilt),
    setBearing: after(raw.setBearing),
    setBandOverride: after(raw.setBandOverride),
    pointerMove: after(raw.pointerMove),
    wheel: after(raw.wheel),
    doubleClick: after(raw.doubleClick),
    tick: after(raw.tick),
  };
  window.maps2 = map;

  await refresh();
  return { map, loader, refresh, onSettled: (listener) => listeners.push(listener) };
}

/**
 * Whether a package's envelope covers a tile path.
 *
 * Only asked of a manifest that does not list its tiles. The box is in
 * degrees, so the tile's own column and row range is computed from it and
 * the path tested against that — the same Mercator arithmetic the tile
 * ids were made with, and the reason a tile far outside the package is
 * never requested at all.
 */
function withinPackageBounds(manifest: TilePackageManifest, path: string): boolean {
  const [z, x, y] = path.split("/").map((part) => Number.parseInt(part, 10));
  if (z === undefined || x === undefined || y === undefined) return false;
  const box = levelBox(manifest, z);
  if (!box) return false;
  const span = 2 ** z;
  const column = (lon: number) => Math.floor((lon + 180) / 360 * span);
  const row = (lat: number) => {
    const radians = lat * Math.PI / 180;
    return Math.floor((1 - Math.asinh(Math.tan(radians)) / Math.PI) / 2 * span);
  };
  return x >= column(box.west) && x <= column(box.east)
    && y >= row(box.north) && y <= row(box.south);
}

/** Границы уровня, а если этого уровня в пакете нет — ближайшего
 *  вышестоящего: именно его тайлы рендерер и подставит. */
export function levelBox(manifest: TilePackageManifest, level: number): LevelBox | null {
  const bounds = manifest.bounds;
  if (!bounds) return null;
  for (let candidate = level; candidate >= 0; candidate -= 1) {
    const box = bounds[String(candidate)];
    if (box) return box;
  }
  return null;
}
