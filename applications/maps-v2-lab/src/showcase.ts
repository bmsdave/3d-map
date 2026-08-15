import { FIXED_CENTRE } from "./bands";
import { createMap, type MapHandle } from "./sdk";
import { el } from "./ui";

interface Scene {
  title: string;
  caption: string;
  pack: "ealing" | "roads" | "ridge";
  zoom: number;
  bearing: number;
  tilt: number;
  relief: boolean;
}

const SCENES: readonly Scene[] = [
  ["First light", "A slow climb from the synthetic horizon", "ridge", 3.2, 12, 12, true],
  ["Blue hour", "A globe rotates into a quiet city", "ealing", 4.0, 24, 10, false],
  ["Contour", "Terrain breathes above the datum", "ridge", 6.2, 38, 28, true],
  ["Ribbon", "A road line finds its rhythm", "roads", 17, 0, 18, false],
  ["Long shadow", "North-west light moves over relief", "ridge", 8.2, 52, 42, true],
  ["Crossfade", "The globe eases into the sheet", "ealing", 4.4, 68, 8, false],
  ["Junction", "Casings hold a complex interchange", "roads", 17.2, 86, 25, false],
  ["Atlas", "Named places settle into position", "ealing", 16, 104, 34, false],
  ["Orbit", "A low-altitude arc around the ridge", "ridge", 3.8, 122, 16, true],
  ["Green room", "The fixture park comes into focus", "ealing", 12.4, 140, 20, false],
  ["Switchback", "Sharp turns reveal their joins", "roads", 16.8, 158, 30, false],
  ["Highlands", "Exaggeration makes the ridge speak", "ridge", 7.4, 176, 48, true],
  ["Northbound", "Camera momentum, held to the frame", "ealing", 14.1, 194, 26, false],
  ["Roundabout", "A circular road stays continuous", "roads", 17.1, 212, 22, false],
  ["Far side", "The globe keeps its curvature", "ridge", 2.8, 230, 10, true],
  ["Density", "Labels compete for a disciplined screen", "ealing", 16.3, 248, 36, false],
  ["Overpass", "Storeys pass without visual conflict", "roads", 17.4, 266, 20, false],
  ["Rise", "Land lifts only while it is a globe", "ridge", 4.1, 284, 32, true],
  ["City pulse", "A precise zoom through the fixture", "ealing", 10.6, 302, 18, false],
  ["Afterglow", "One last pass across the terrain", "ridge", 5.5, 320, 38, true],
].map(([title, caption, pack, zoom, bearing, tilt, relief]) => ({
  title: title as string,
  caption: caption as string,
  pack: pack as Scene["pack"],
  zoom: zoom as number,
  bearing: bearing as number,
  tilt: tilt as number,
  relief: relief as boolean,
}));

let activeState: { playing: boolean; destroyed: boolean } | null = null;

export function showcase(): HTMLElement {
  const root = el("main", { class: "showcase", "data-testid": "showcase", "data-playing": "true" });
  const toggle = el("button", { class: "showcase-toggle", "data-testid": "showcase-toggle", type: "button" }, ["Pause motion"]);
  const grid = el("div", { class: "showcase-grid" });
  const state = { playing: true, destroyed: false };
  activeState = state;
  toggle.addEventListener("click", () => toggleMotion(root, toggle, state));
  root.append(showcaseIntro(toggle), grid);
  SCENES.forEach((scene, index) => grid.append(mountScene(scene, index, state)));
  return root;
}

function showcaseIntro(toggle: HTMLButtonElement): HTMLElement {
  return el("section", { class: "showcase-hero" }, [
    el("p", { class: "eyebrow" }, ["3D Maps SDK v2 · alpha experiments"]),
    el("h1", {}, ["Twenty moving studies of the same deterministic world."]),
    el("p", { class: "showcase-copy" }, ["Every tile below is rendered by the SDK. No video, no image mockups, no real-world data." ]),
    toggle,
  ]);
}

function mountScene(scene: Scene, index: number, state: { playing: boolean; destroyed: boolean }): HTMLElement {
  const stage = el("article", { class: "showcase-card", "data-testid": "showcase-demo", "data-state": "loading" });
  const canvas = el("canvas", { width: "480", height: "320" });
  const copy = el("div", { class: "showcase-card-copy" }, [el("span", { class: "showcase-index" }, [`${String(index + 1).padStart(2, "0")}`]), el("h2", {}, [scene.title]), el("p", {}, [scene.caption])]);
  stage.append(canvas, copy);
  void createSceneMap(canvas, scene, index, state, stage);
  return stage;
}

async function createSceneMap(canvas: HTMLCanvasElement, scene: Scene, index: number, state: { playing: boolean; destroyed: boolean }, stage: HTMLElement): Promise<void> {
  try {
    const map = await createMap(canvas, scene.pack);
    map.setCentre(FIXED_CENTRE.lon, FIXED_CENTRE.lat);
    map.setRelief(scene.relief);
    map.setHypsometric(scene.relief);
    map.setReliefExaggeration(scene.relief ? 44 : 0);
    stage.setAttribute("data-state", "ready");
    animateScene(map, scene, index, state, stage);
  } catch (error) {
    stage.setAttribute("data-state", "error");
    stage.setAttribute("data-error", String(error));
  }
}

function animateScene(map: MapHandle, scene: Scene, index: number, state: { playing: boolean; destroyed: boolean }, stage: HTMLElement): void {
  const draw = (now: number) => {
    if (state.destroyed) return;
    if (state.playing) drawScene(map, scene, index, now, stage);
    requestAnimationFrame(draw);
  };
  requestAnimationFrame(draw);
}

function drawScene(map: MapHandle, scene: Scene, index: number, now: number, stage: HTMLElement): void {
  const phase = now / 3500 + index * 0.41;
  map.setZoom(scene.zoom + Math.sin(phase) * 0.34);
  map.setBearing(scene.bearing + Math.cos(phase * 0.7) * 18);
  map.setTilt(scene.tilt + Math.sin(phase * 0.8) * 8);
  map.render();
  stage.setAttribute("data-frame", String(Math.floor(now)));
}

function toggleMotion(root: HTMLElement, toggle: HTMLButtonElement, state: { playing: boolean }): void {
  state.playing = !state.playing;
  root.setAttribute("data-playing", String(state.playing));
  toggle.textContent = state.playing ? "Pause motion" : "Play motion";
}

export function destroyShowcase(): void {
  if (activeState) activeState.destroyed = true;
  activeState = null;
}
