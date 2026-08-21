// Чем можно подвигать в каждой студии. Один список на все двадцать:
// тесты производительности не пишутся по одному на карточку — они
// генерируются отсюда, иначе девятнадцатая студия останется без них.
//
// Значения ручек подобраны так, чтобы движение было настоящим: слайдер
// зума, гоняемый между 16.0 и 16.1, не меняет уровень тайлов и меряет
// пустоту.

export type Interaction =
  /** Слайдер: перебрать значения через fill(). */
  | { kind: "range"; testId: string; values: string[] }
  /** Select: перебрать варианты. */
  | { kind: "select"; testId: string; values: string[] }
  /** Чекбокс: включить/выключить. */
  | { kind: "toggle"; testId: string }
  /** Текстовое поле. */
  | { kind: "text"; testId: string; values: string[] }
  /** Колесо над холстом: настоящий зум камеры, а не ручка карточки. */
  | { kind: "wheel"; steps: number }
  /** Протащить холст: настоящий пан. */
  | { kind: "drag"; dx: number; dy: number };

export interface Study {
  id: string;
  /** Главное взаимодействие: то, чем человек пользуется чаще всего. */
  primary: Interaction;
  /** Второе — на нём проверяются инварианты «без лишней работы». */
  secondary: Interaction;
}

const ZOOM_SWEEP = ["12", "14", "15.5", "16.5", "13"];
const TILT_SWEEP = ["0", "25", "45", "60", "10"];

/** Шесть тоглов полос генерируются так же, как сами карточки. */
const BAND_TOGGLES = [
  "toggle-world-region",
  "toggle-region-city",
  "toggle-city-district",
  "toggle-district-street",
  "toggle-street-address",
  "toggle-address-micro",
];

export const STUDIES: readonly Study[] = [
  {
    id: "zoom-bands",
    primary: { kind: "range", testId: "zoom-slider", values: ["4", "8", "11", "14", "16.5"] },
    secondary: { kind: "range", testId: "zoom-slider", values: ["12", "12.5", "13", "12.5", "12"] },
  },
  {
    id: "roads-micro",
    primary: { kind: "select", testId: "miter-limit", values: ["1.5", "4", "2"] },
    secondary: { kind: "toggle", testId: "casing-toggle" },
  },
  {
    id: "buildings-3d",
    primary: { kind: "range", testId: "building-tilt-slider", values: TILT_SWEEP },
    secondary: { kind: "drag", dx: -220, dy: 0 },
  },
  {
    id: "type-specimen",
    primary: { kind: "range", testId: "bearing-slider", values: ["0", "90", "180", "270", "0"] },
    secondary: { kind: "range", testId: "halo-slider", values: ["0", "0.07", "0.14", "0.04"] },
  },
  {
    id: "labels-collision",
    primary: { kind: "range", testId: "zoom-slider", values: ["12", "14", "16", "17", "15"] },
    secondary: { kind: "toggle", testId: "boxes-toggle" },
  },
  {
    id: "poi-density",
    primary: { kind: "range", testId: "budget-slider", values: ["2", "8", "14", "20", "8"] },
    secondary: { kind: "drag", dx: -180, dy: -120 },
  },
  {
    id: "viewport-stability",
    primary: { kind: "range", testId: "shift-slider", values: ["1", "5", "12", "20", "3"] },
    secondary: { kind: "range", testId: "shift-slider", values: ["4", "4", "4", "4"] },
  },
  {
    id: "input-flat",
    primary: { kind: "wheel", steps: 6 },
    secondary: { kind: "drag", dx: -260, dy: -140 },
  },
  {
    id: "globe-transition",
    primary: { kind: "range", testId: "globeness-slider", values: ["3", "3.6", "4.2", "5", "3.4"] },
    secondary: { kind: "range", testId: "globeness-slider", values: ["4", "4.1", "4", "4.1"] },
  },
  {
    id: "terrain-shade",
    primary: { kind: "range", testId: "expressiveness-slider", values: ["0", "0.35", "0.7", "1", "0.5"] },
    secondary: { kind: "toggle", testId: "relief-toggle" },
  },
  {
    id: "globe-relief",
    primary: { kind: "range", testId: "exaggeration-slider", values: ["0", "40", "80", "120", "44"] },
    secondary: { kind: "toggle", testId: "relief-toggle" },
  },
  {
    id: "globe-real",
    primary: { kind: "range", testId: "globe-real-zoom", values: ["2", "6", "10", "13", "4"] },
    secondary: { kind: "range", testId: "globe-real-tilt", values: TILT_SWEEP },
  },
  {
    id: "map-real",
    primary: { kind: "range", testId: "map-real-zoom", values: ZOOM_SWEEP },
    secondary: { kind: "range", testId: "map-real-tilt", values: TILT_SWEEP },
  },
  {
    id: "package-loader",
    primary: { kind: "range", testId: "package-tilt-slider", values: TILT_SWEEP },
    secondary: { kind: "drag", dx: -200, dy: 0 },
  },
  ...BAND_TOGGLES.map((id): Study => ({
    id,
    primary: { kind: "select", testId: "composition-toggle", values: [] },
    secondary: { kind: "select", testId: "transition-mode", values: ["animated", "instant"] },
  })),
];
