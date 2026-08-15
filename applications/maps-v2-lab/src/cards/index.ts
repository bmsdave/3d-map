import { TRANSITIONS } from "../bands";
import { bandToggleCard } from "./bandToggle";
import { globeRelief } from "./globeRelief";
import { globeTransition } from "./globeTransition";
import { inputFlat } from "./inputFlat";
import { labelsCollision } from "./labelsCollision";
import { poiDensity } from "./poiDensity";
import { roadsMicro } from "./roadsMicro";
import { terrainShade } from "./terrainShade";
import type { CardSpec } from "./types";
import { typeSpecimen } from "./typeSpecimen";
import { viewportStability } from "./viewportStability";
import { zoomBands } from "./zoomBands";

export const CARDS: readonly CardSpec[] = [
  zoomBands,
  roadsMicro,
  typeSpecimen,
  labelsCollision,
  poiDensity,
  viewportStability,
  inputFlat,
  globeTransition,
  terrainShade,
  globeRelief,
  ...TRANSITIONS.map(bandToggleCard),
];

export function cardById(id: string): CardSpec | undefined {
  return CARDS.find((card) => card.id === id);
}
