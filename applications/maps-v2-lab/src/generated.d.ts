// Типы для wasm-бандла, собранного без --typescript.
declare module "*/maps2_web.js" {
  export default function init(input?: unknown): Promise<unknown>;
  export class Map {
    constructor(canvasId: string);
    load_tile(bytes: Uint8Array): void;
    set_zoom(zoom: number): void;
    set_centre(lon: number, lat: number): void;
    set_viewport(width: number, height: number): void;
    render(): void;
    debug(): string;
    set_band_override(band: string | undefined): void;
    set_transition_animated(animated: boolean): void;
    set_road_casing(on: boolean): void;
    set_miter_limit(limit: number): void;
    set_road_width_px(className: string, px: number): void;
    sample_pixel(x: number, y: number): string;
    set_label_budget(budget: number): void;
    set_halo_em(halo: number): void;
    set_collision_boxes(on: boolean): void;
    set_specimen(text: string | undefined): void;
    set_bearing(degrees: number): void;
    set_tilt(degrees: number): void;
    label_debug(): string;
    globeness(): number;
    set_relief(on: boolean): void;
    set_hypsometric(on: boolean): void;
    set_relief_expressiveness(value: number): void;
    set_relief_exaggeration(value: number): void;
    pointer_down(x: number, y: number, nowMs: number): void;
    pointer_move(x: number, y: number, nowMs: number): boolean;
    pointer_up(): void;
    wheel(x: number, y: number, deltaY: number, pinch: boolean): void;
    double_click(x: number, y: number): void;
    key(name: string): boolean;
    tick(dtMs: number): boolean;
  }
}
