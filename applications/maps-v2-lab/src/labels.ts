// Инварианты кадра подписей, посчитанные хостом из label_debug().
// Живут здесь, а не в карточке, потому что их читают три карточки и
// e2e — и потому что это ровно те утверждения, которыми роадмап
// разрешает проверять подписи: список и его свойства, не пиксели.

import type { LabelEntry } from "./sdk";

export function placed(entries: LabelEntry[]): LabelEntry[] {
  return entries.filter((e) => e.state === "placed");
}

function overlaps(a: LabelEntry, b: LabelEntry): boolean {
  return (
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
  );
}

/** Сколько пар размещённых боксов пересеклись. Инвариант: ноль. */
export function overlapCount(entries: LabelEntry[]): number {
  const boxes = placed(entries);
  let count = 0;
  for (let i = 0; i < boxes.length; i += 1) {
    for (let j = i + 1; j < boxes.length; j += 1) {
      if (overlaps(boxes[i]!, boxes[j]!)) count += 1;
    }
  }
  return count;
}

/**
 * Есть ли отвергнутый по коллизии, который важнее того, кто его
 * вытеснил. Инвариант: нет — порядок жадного размещения (rank, id).
 */
export function rankInversions(entries: LabelEntry[]): number {
  const byId = new Map(placed(entries).map((e) => [e.id, e]));
  return entries.filter((e) => {
    if (e.state !== "collision" || e.blocked_by === null) return false;
    const blocker = byId.get(e.blocked_by);
    if (!blocker) return false;
    return blocker.rank > e.rank || (blocker.rank === e.rank && blocker.id > e.id);
  }).length;
}

/** Доля размещённых, сменившихся между двумя кадрами. */
export function churn(before: LabelEntry[], after: LabelEntry[]): number {
  const was = new Set(placed(before).map((e) => e.id));
  const now = new Set(placed(after).map((e) => e.id));
  if (was.size === 0) return 0;
  let changed = 0;
  for (const id of was) if (!now.has(id)) changed += 1;
  for (const id of now) if (!was.has(id)) changed += 1;
  return changed / was.size;
}

/** Сдвиг центра на dx экранных пикселей во flat-Меркаторе. */
export function lonPerPixel(zoom: number): number {
  return 360 / (256 * 2 ** zoom);
}
