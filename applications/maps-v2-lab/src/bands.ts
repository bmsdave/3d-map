// Семь полос входа — те же, что в zoom-and-layers.md §2 и bands.rs пайплайна.
// Здесь это спецификация для карточек: ожидаемые значения, против которых
// e2e сверит поведение SDK, когда он будет подключён.

export interface Band {
  name: string;
  entry: number;
}

export const BANDS: readonly Band[] = [
  { name: "World", entry: 0 },
  { name: "Region", entry: 5 },
  { name: "City", entry: 8 },
  { name: "District", entry: 10 },
  { name: "Street", entry: 12 },
  { name: "Address", entry: 14 },
  { name: "Micro", entry: 16 },
];

export function expectedBand(zoom: number): Band {
  let current = BANDS[0]!;
  for (const band of BANDS) {
    if (zoom >= band.entry) current = band;
  }
  return current;
}

export interface Transition {
  lower: Band;
  upper: Band;
}

export const TRANSITIONS: readonly Transition[] = BANDS.slice(1).map(
  (upper, i) => ({ lower: BANDS[i]!, upper }),
);

// Одно место на все карточки: Ealing Broadway. Фиксированная точка нужна,
// чтобы фикстурный пакет был один и тот же и никто никуда не скролил.
export const FIXED_CENTRE = { lon: -0.3049, lat: 51.5149 };
