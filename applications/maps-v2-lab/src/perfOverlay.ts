import type { MapHandle } from "./sdk";

/// How many frames the rolling figures average over. Long enough that a
/// single slow frame does not dominate the reading, short enough that a
/// change shows up while you are still doing the thing that caused it.
const WINDOW_FRAMES = 60;

/// Frame budget for 60 fps. The overlay colours against this, so a
/// glance says whether the map is keeping up rather than what it costs.
const BUDGET_MS = 16.7;

interface Sample {
  /** Renderer time for the frame — main thread, blocking. */
  frame: number;
  /** Wall time of the host's load pass, most of which is awaited I/O. */
  pass: number;
  /** Main-thread time inside that pass: the part that can cause jank. */
  blocked: number;
}

/**
 * A heads-up display over the canvas: where the frame time goes, how
 * much the host is asking for, and how much of the map is being held.
 *
 * The numbers come from the renderer's own measurements rather than
 * from wrapping calls out here, because the phases inside one `render`
 * are exactly what a host cannot see.
 */
export class PerfOverlay {
  readonly root: HTMLElement;
  private readonly samples: Sample[] = [];
  private hostStart = 0;
  private loadBlocked = 0;
  private loadWall = 0;

  constructor() {
    this.root = document.createElement("div");
    this.root.dataset.testid = "perf-overlay";
    this.root.style.cssText = [
      "position:absolute", "top:8px", "left:8px", "z-index:5",
      "font:11px/1.45 ui-monospace,SFMono-Regular,Menlo,monospace",
      "background:rgba(20,20,24,.82)", "color:#eee", "padding:8px 10px",
      "border-radius:6px", "pointer-events:none", "white-space:pre",
      "min-width:210px",
    ].join(";");
  }

  /** Call when the host begins the work it wants counted for a frame. */
  beginHostWork(): void {
    this.hostStart = performance.now();
    this.loadBlocked = 0;
    this.loadWall = 0;
  }

  /// Report a load pass: how long it held the main thread, and how
  /// long it took in total.
  ///
  /// The blocking figure is measured directly around the synchronous
  /// calls rather than derived by subtracting awaited time from wall
  /// time. Wall time across an `await` includes whatever the browser
  /// felt like doing — a background tab clamps timers to about a
  /// second — so a derived figure reports the browser's scheduling as
  /// though it were the map's cost.
  recordLoad(blockedMs: number, wallMs: number): void {
    this.loadBlocked += blockedMs;
    this.loadWall += wallMs;
  }

  /** Call after that work, with the map, to record and repaint. */
  endHostWork(map: MapHandle): void {
    const state = map.debug();
    // A pass that waits 200 ms for a tile has not blocked anything.
    // Separating the two is the difference between a slow network and
    // a slow map, and they are fixed in completely different places.
    this.samples.push({
      frame: state.frame_ms,
      pass: this.loadWall,
      blocked: this.loadBlocked,
    });
    if (this.samples.length > WINDOW_FRAMES) this.samples.shift();
    this.root.textContent = this.text(state);
  }

  private text(state: ReturnType<MapHandle["debug"]>): string {
    const frames = this.samples.map((s) => s.frame);
    const blocked = this.samples.map((s) => s.blocked);
    const passes = this.samples.map((s) => s.pass);
    const lines = [
      `${bar(mean(frames), BUDGET_MS)} draw   ${ms(mean(frames))} max ${ms(Math.max(...frames, 0))}`,
      `${bar(mean(blocked), BUDGET_MS)} block  ${ms(mean(blocked))} p95 ${ms(percentile(blocked, 0.95))}`,
      `        load   ${ms(mean(passes))} p95 ${ms(percentile(passes, 0.95))}`,
      "",
      `residency ${ms(state.residency_ms)}   plans ${state.plans}`,
      `ground    ${ms(state.ground_ms)}`,
      `fills     ${ms(state.fills_ms)}`,
      `roads     ${ms(state.roads_ms)}`,
      `buildings ${ms(state.buildings_ms)}`,
      `labels    ${ms(state.labels_ms)}   placed ${state.labels_placed}/${state.label_candidates}`,
      "",
      `z ${state.zoom.toFixed(2)} → level ${state.tile_level} (${state.shape})`,
      `tiles  drawn ${state.tiles_drawn}  gpu ${state.resident_gpu}  cpu ${state.cpu_tiles}`,
      `calls ${state.draw_calls}   held ${mib(state.cpu_bytes)}   evicted ${state.evictions}`,
    ];
    return lines.join("\n");
  }
}

function mean(values: number[]): number {
  return values.length === 0 ? 0 : values.reduce((a, b) => a + b, 0) / values.length;
}

function percentile(values: number[], share: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * share) - 1)] ?? 0;
}

function ms(value: number): string {
  return `${value.toFixed(2)}ms`.padStart(8);
}

function mib(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)}MiB`;
}

/// Five cells against the frame budget: full is over.
function bar(value: number, budget: number): string {
  const filled = Math.min(5, Math.round((value / budget) * 5));
  return `[${"█".repeat(filled)}${"·".repeat(5 - filled)}]`;
}
