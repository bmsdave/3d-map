import { createPackageMap, type MapHandle, type PackageMap } from "../sdk";
import { el } from "../ui";
import "./demo.css";

// The public demo: one package, one canvas, the whole SDK.
//
// The lab next door is a workbench — every study isolates one decision so
// a test can read it back. This page has the opposite job: show the thing
// working, on real ground, without asking the visitor to know what a
// residency plan is. It uses the same SDK entry points the lab does, so
// whatever ships here is the same code the studies measure.

/** Served from wherever the site is rooted — "/" in dev, "/<repo>/" on
 *  Pages. Tile URLs resolve against the manifest, so this one path is the
 *  only place the base has to be spelled out. */
const CARVE = `${import.meta.env.BASE_URL}packages/trafalgar/manifest.json`;

/**
 * Which package the demo opens.
 *
 * `?package=<manifest url>` points it at another one — a planet built by
 * `maps2-ingest build-map` and put on object storage, say — without a
 * rebuild, because the only thing that changes between one package and
 * another is this URL. Tile URLs resolve against the manifest, so a
 * package hosted anywhere works as long as it answers CORS.
 *
 * Without the parameter it opens the carve committed beside the lab,
 * which is what makes this page work from a clone with nothing else set
 * up.
 */
function packageUrl(): string {
  const asked = new URLSearchParams(window.location.search).get("package");
  return asked && asked.trim().length > 0 ? asked.trim() : CARVE;
}

const MANIFEST = packageUrl();

/// Below this the ground is a generalised world and relief is the only
/// thing that makes it visible; at and above it a real city extract owns
/// the ground, where hypsometric tint would read as countryside and
/// shading a 30 m DEM speckles the streets. Same threshold as the
/// map-real study.
const CITY_ENTRY_ZOOM = 12;

/** The renderer measures in canvas pixels: a style's road widths and type
 *  sizes are `ScreenPx`, and `ScreenPx` is what the canvas is sized in. A
 *  2x backing store therefore draws a perfectly correct map at half its
 *  intended physical size — half-width streets and unreadable type on
 *  exactly the displays that can afford the pixels. Until the SDK takes a
 *  device pixel ratio of its own, the backing store is the CSS size, and
 *  the display does the upscaling. */
const PIXEL_RATIO = 1;

interface Viewpoint {
  id: string;
  label: string;
  hint: string;
  lon: number;
  lat: number;
  zoom: number;
  tilt: number;
  bearing: number;
}

const VIEWPOINTS: readonly Viewpoint[] = [
  {
    id: "globe",
    label: "Globe",
    hint: "Coastlines, GEBCO bathymetry and hypsometric land — one pyramid, no basemap service.",
    lon: -0.1281, lat: 40, zoom: 2.6, tilt: 0, bearing: 0,
  },
  {
    id: "britain",
    label: "Britain",
    hint: "The globe flattens between zoom 3.5 and 4.5; the same tiles keep drawing across the change.",
    lon: -2.4, lat: 53.4, zoom: 5.2, tilt: 0, bearing: 0,
  },
  {
    id: "london",
    label: "London",
    hint: "The city extract takes over the ground at zoom 12: streets, water and parks, with labels collision-resolved against a screen-space grid.",
    lon: -0.1281, lat: 51.508, zoom: 12.8, tilt: 18, bearing: 0,
  },
  {
    id: "trafalgar",
    label: "Trafalgar Square",
    hint: "The extract's deepest level: service roads and footways under their casing, street names placed against everything already on screen.",
    lon: -0.1281, lat: 51.508, zoom: 16, tilt: 34, bearing: 336,
  },
];

const FLY_MS = 1400;
const ease = (t: number): number => (t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2);

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

/** Backing store in device pixels, CSS size left to the stylesheet. Returns
 *  false when nothing changed, so a resize observer firing on every layout
 *  does not invalidate the residency plan for free. */
function sizeCanvas(canvas: HTMLCanvasElement): boolean {
  // A hidden page — a background tab, a collapsed pane — lays its canvas
  // out at nothing. Following that would resize the map to a pixel and
  // throw away its residency plan for a size it will never draw at, so a
  // zero-size layout is not a size: it is the absence of one.
  if (canvas.clientWidth < 1 || canvas.clientHeight < 1) return false;
  const width = Math.round(canvas.clientWidth * PIXEL_RATIO);
  const height = Math.round(canvas.clientHeight * PIXEL_RATIO);
  if (canvas.width === width && canvas.height === height) return false;
  canvas.width = width;
  canvas.height = height;
  return true;
}

interface Readouts {
  root: HTMLElement;
  set(key: string, value: string): void;
}

function readouts(rows: readonly { key: string; label: string }[]): Readouts {
  const cells = new Map<string, HTMLElement>();
  const root = el("dl", { class: "readout" });
  for (const row of rows) {
    const value = el("dd", { "data-testid": `demo-${row.key}` }, ["—"]);
    cells.set(row.key, value);
    root.append(el("div", {}, [el("dt", {}, [row.label]), value]));
  }
  return {
    root,
    set(key, text) {
      const cell = cells.get(key);
      if (cell && cell.textContent !== text) cell.textContent = text;
    },
  };
}

interface Slider {
  row: HTMLElement;
  /** Follow the camera. The map is the truth: a slider shows where the
   *  camera ended up, including after a drag, a flight or a clamp against
   *  the package's edge. */
  set(value: number): void;
}

function slider(
  label: string,
  attrs: { min: string; max: string; step: string; value: string; id: string },
  onInput: (value: number) => void,
): Slider {
  const input = el("input", { type: "range", ...attrs, "data-testid": `demo-slider-${attrs.id}` });
  const shown = el("span", { class: "value" }, [attrs.value]);
  input.addEventListener("input", () => onInput(Number(input.value)));
  return {
    row: el("label", { class: "control" }, [
      el("span", { class: "control-label" }, [label, shown]),
      input,
    ]),
    set(value) {
      const text = Math.abs(value) < 10 ? value.toFixed(1) : String(Math.round(value));
      if (shown.textContent !== text) shown.textContent = text;
      input.value = String(value);
    },
  };
}

function toggle(label: string, id: string, on: boolean, onChange: (on: boolean) => void): HTMLElement {
  const input = el("input", { type: "checkbox", "data-testid": `demo-${id}` });
  input.checked = on;
  input.addEventListener("change", () => onChange(input.checked));
  return el("label", { class: "control toggle" }, [input, el("span", {}, [label])]);
}

function attribution(): HTMLElement {
  return el("p", { class: "attribution", "data-testid": "demo-attribution" }, [
    "© ",
    el("a", { href: "https://www.openstreetmap.org/copyright" }, ["OpenStreetMap"]),
    " contributors (ODbL) · land relief Copernicus DEM (© DLR e.V. 2010–2014, © Airbus DS 2014–2018, ESA/EU) · bathymetry ",
    el("a", { href: "https://www.gebco.net" }, ["GEBCO 2026"]),
    " · boundaries and places Natural Earth.",
  ]);
}

function shell(): {
  root: HTMLElement;
  canvas: HTMLCanvasElement;
  panel: HTMLElement;
  status: HTMLElement;
} {
  const canvas = el("canvas", { id: "map", tabindex: "0", "data-testid": "demo-canvas" });
  const panel = el("aside", { class: "panel", "data-testid": "demo-panel" });
  const status = el("div", { class: "status", "data-testid": "demo-status", role: "status" }, [
    "Loading the package…",
  ]);
  // On a phone the panel is as wide as the map, so opening with it up
  // means the demo's first frame is a control panel. The map leads; the
  // controls are one tap away.
  const narrow = window.matchMedia("(max-width: 720px)").matches;
  if (narrow) panel.setAttribute("data-collapsed", "");
  const disclose = el(
    "button",
    { type: "button", class: "disclose", "aria-expanded": String(!narrow) },
    ["Controls"],
  );
  disclose.addEventListener("click", () => {
    const open = panel.toggleAttribute("data-collapsed");
    disclose.setAttribute("aria-expanded", String(!open));
  });
  const root = el("div", { class: "demo-shell" }, [
    el("div", { class: "stage" }, [canvas, status]),
    el("header", { class: "brand" }, [
      el("h1", {}, ["maps-v2"]),
      el("p", {}, ["3D vector maps in Rust and WebGL2 — real data, one tile pyramid, no basemap service."]),
      el("nav", {}, [
        el("a", { href: "https://github.com/bmsdave/3d-map", class: "primary" }, ["GitHub"]),
        el("a", { href: `${import.meta.env.BASE_URL}` }, ["Lab studies"]),
      ]),
    ]),
    disclose,
    panel,
  ]);
  return { root, canvas, panel, status };
}

/** What the visitor asked for, and what the map was last told. Relief and
 *  tint belong to the world levels only, so the wanted state is a function
 *  of both the switches and the zoom — and reapplying it every frame would
 *  rebuild ground meshes for nothing. */
const terrain = { relief: true, tint: true, applied: "" };

function applyTerrain(map: MapHandle, zoom: number): boolean {
  const world = zoom < CITY_ENTRY_ZOOM;
  const relief = world && terrain.relief;
  const tint = world && terrain.tint;
  const wanted = `${relief}/${tint}`;
  if (wanted === terrain.applied) return false;
  terrain.applied = wanted;
  map.setRelief(relief);
  map.setHypsometric(tint);
  return true;
}

interface Controls {
  zoom: Slider;
  tilt: Slider;
  bearing: Slider;
  out: Readouts;
  select(id: string): void;
  /** Advance a viewpoint flight, if one is in the air. */
  step(now: number): void;
}

function buildPanel(panel: HTMLElement, map: MapHandle): Controls {
  const out = readouts([
    { key: "shape", label: "shape" },
    { key: "zoom", label: "zoom" },
    { key: "level", label: "tile level" },
    { key: "tiles", label: "tiles loaded" },
    { key: "frame", label: "frame" },
  ]);
  const hint = el("p", { class: "hint", "data-testid": "demo-hint" }, [VIEWPOINTS[0]!.hint]);

  let flight: { from: Viewpoint; to: Viewpoint; started: number } | null = null;
  const buttons = new Map<string, HTMLElement>();

  const zoom = slider("Zoom", { min: "0.6", max: "17", step: "0.1", value: "2.6", id: "zoom" }, (value) => {
    flight = null;
    map.setZoom(value);
    zoom.set(value);
  });
  const tilt = slider("Tilt", { min: "0", max: "60", step: "1", value: "0", id: "tilt" }, (value) => {
    flight = null;
    map.setTilt(value);
    tilt.set(value);
  });
  const bearing = slider("Bearing", { min: "0", max: "360", step: "1", value: "0", id: "bearing" }, (value) => {
    flight = null;
    map.setBearing(value);
    bearing.set(value);
  });

  const current = (): Viewpoint => {
    const state = map.debug();
    return {
      id: "current", label: "", hint: "",
      lon: state.centre_lon, lat: state.centre_lat,
      zoom: state.zoom, tilt: state.tilt, bearing: state.bearing,
    };
  };

  // Flying, rather than jumping, is the point of a preset here: the shape
  // change between globe and plane and the tile levels crossed on the way
  // are the parts worth seeing, and a jump skips both.
  const step = (now: number): void => {
    if (!flight) return;
    const t = Math.min(1, (now - flight.started) / FLY_MS);
    const k = ease(t);
    const at = (from: number, to: number): number => from + (to - from) * k;
    const { from, to } = flight;
    map.setCentre(at(from.lon, to.lon), at(from.lat, to.lat));
    map.setZoom(at(from.zoom, to.zoom));
    map.setTilt(at(from.tilt, to.tilt));
    map.setBearing(at(from.bearing, to.bearing));
    if (t >= 1) flight = null;
  };

  const select = (id: string): void => {
    const view = VIEWPOINTS.find((entry) => entry.id === id);
    if (!view) return;
    for (const [key, button] of buttons) {
      if (key === id) button.setAttribute("aria-current", "true");
      else button.removeAttribute("aria-current");
    }
    hint.textContent = view.hint;
    flight = { from: current(), to: view, started: performance.now() };
  };

  const tour = el("div", { class: "tour", "data-testid": "demo-tour" });
  for (const view of VIEWPOINTS) {
    const button = el("button", { type: "button", "data-view": view.id }, [view.label]);
    button.addEventListener("click", () => select(view.id));
    buttons.set(view.id, button);
    tour.append(button);
  }
  buttons.get(VIEWPOINTS[0]!.id)?.setAttribute("aria-current", "true");

  panel.replaceChildren(
    el("h2", {}, ["Viewpoints"]),
    tour,
    hint,
    el("h2", {}, ["Camera"]),
    el("div", { class: "controls" }, [zoom.row, tilt.row, bearing.row]),
    el("h2", {}, ["Ground"]),
    el("div", { class: "controls" }, [
      toggle("Relief shading", "relief", true, (on) => { terrain.relief = on; terrain.applied = ""; }),
      toggle("Elevation tint", "hypsometric", true, (on) => { terrain.tint = on; terrain.applied = ""; }),
      toggle("Road casing", "casing", true, (on) => { map.setRoadCasing(on); }),
    ]),
    el("h2", {}, ["Frame"]),
    out.root,
    attribution(),
  );

  return { zoom, tilt, bearing, out, select, step };
}

async function start(): Promise<void> {
  const host = document.querySelector<HTMLDivElement>("#demo")!;
  const { root, canvas, panel, status } = shell();
  host.replaceChildren(root);
  sizeCanvas(canvas);

  let pkg: PackageMap;
  try {
    pkg = await createPackageMap(canvas, {
      zoom: VIEWPOINTS[0]!.zoom,
      centre: { lon: VIEWPOINTS[0]!.lon, lat: VIEWPOINTS[0]!.lat },
      manifestUrl: MANIFEST,
    });
  } catch (error) {
    status.replaceChildren(
      `The package at ${MANIFEST} did not load: ${String(error)}`,
    );
    status.setAttribute("data-state", "error");
    return;
  }
  const { map, loader } = pkg;
  map.setReliefExpressiveness(0.22);
  map.setTilt(VIEWPOINTS[0]!.tilt);
  map.setBearing(VIEWPOINTS[0]!.bearing);
  map.setViewport(canvas.width, canvas.height);

  // A frame is drawn every animation frame, not only when something
  // changed. WebGL hands the drawing buffer to the compositor and does not
  // promise it back: an on-demand map goes blank the moment the page
  // composites without it, which is exactly what a still map does. The
  // renderer reports the cost of every frame in the panel, so what this
  // spends is on screen rather than assumed.
  const controls = buildPanel(panel, map);

  new ResizeObserver(() => {
    if (!sizeCanvas(canvas)) return;
    map.setViewport(canvas.width, canvas.height);
  }).observe(canvas);

  attachInput(canvas, map);

  // One loop for everything: a preset in flight, the camera coasting
  // after a flick, and the panel's reading of where the camera ended up.
  let previous = performance.now();
  let frameMs = 0;
  const frame = (now: number): void => {
    const dt = Math.max(now - previous, 1);
    previous = now;
    controls.step(now);
    map.tick(dt);
    const state = map.debug();
    applyTerrain(map, state.zoom);
    const started = performance.now();
    map.render();
    const spent = performance.now() - started;
    frameMs = frameMs === 0 ? spent : frameMs * 0.85 + spent * 0.15;
    controls.zoom.set(state.zoom);
    controls.tilt.set(state.tilt);
    controls.bearing.set(state.bearing);
    controls.out.set("shape", state.shape);
    controls.out.set("zoom", state.zoom.toFixed(2));
    controls.out.set("level", String(state.tile_level));
    controls.out.set("tiles", String(state.cpu_tiles));
    controls.out.set("frame", `${frameMs.toFixed(1)} ms`);
    root.setAttribute("data-shape", state.shape);
    root.setAttribute("data-zoom", state.zoom.toFixed(2));
    requestAnimationFrame(frame);
  };

  await pkg.refresh();
  status.setAttribute("data-state", "ready");
  // Short enough to stay one line on a phone; the keyboard gestures are
  // the kind of thing you look for rather than read.
  status.title = "Arrow keys pan, +/− step a zoom level, double-click steps in";
  status.replaceChildren(
    `${loader.manifest.tile_count} tiles · drag to pan, scroll to zoom`,
  );
  root.setAttribute("data-ready", "true");
  requestAnimationFrame(frame);
}

function attachInput(canvas: HTMLCanvasElement, map: MapHandle): void {
  let dragging = false;
  canvas.addEventListener("pointerdown", (event) => {
    dragging = true;
    canvas.setPointerCapture(event.pointerId);
    map.pointerDown(...canvasPoint(canvas, event), event.timeStamp);
  });
  canvas.addEventListener("pointermove", (event) => {
    if (!dragging) return;
    map.pointerMove(...canvasPoint(canvas, event), event.timeStamp);
  });
  const release = (): void => { dragging = false; map.pointerUp(); };
  canvas.addEventListener("pointerup", release);
  canvas.addEventListener("pointercancel", release);
  canvas.addEventListener("wheel", (event) => {
    event.preventDefault();
    map.wheel(...canvasPoint(canvas, event), event.deltaY, event.ctrlKey);
  }, { passive: false });
  canvas.addEventListener("dblclick", (event) => {
    map.doubleClick(...canvasPoint(canvas, event));
  });
  canvas.addEventListener("contextmenu", (event) => event.preventDefault());
  canvas.addEventListener("keydown", (event) => {
    if (map.key(event.key)) event.preventDefault();
  });
}

void start();
