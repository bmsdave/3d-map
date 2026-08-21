// Где лаборатория проводит время. Не замена профилировщику браузера, а
// то, чего профилировщик не знает: за границей wasm он видит один вызов
// `map.render()` и не может сказать, во что тот превратился, — а имена
// фаз здесь знает только хост.
//
// Выключено по умолчанию: в горячем пути остаётся проверка одного
// булева поля. Включается `?perf=1` в адресе лабы или
// `window.__maps2Perf.enable()` из консоли.

/// Фазы, которые хост умеет назвать. Замкнутый список, а не свободная
/// строка: отчёт e2e печатает имя фазы, съевшей бюджет, и опечатка в
/// нём стоила бы часа поисков несуществующей проблемы.
export type Phase =
  | "render"
  | "debug"
  | "demand"
  | "decode"
  | "digest"
  | "fetch"
  // «pass» — стена всего прохода загрузки, «blocked» — та его часть,
  // что держала main thread. Разница между ними и есть ожидание сети.
  | "pass"
  | "blocked"
  | "clamp"
  | "labels";

interface Span {
  phase: Phase;
  /** Миллисекунды от начала до конца. */
  ms: number;
  /** `performance.now()` начала — чтобы сопоставить с кадрами. */
  at: number;
  /** Адрес работы внутри фазы: какой тайл декодировался. Необязателен —
   *  у `render` его нет и быть не может. */
  detail?: string;
}

/// Долгий кадр глазами браузера. `blockingDuration` — то самое время,
/// на которое main thread перестал отвечать, и меряет его не наш код.
export interface LongFrame {
  at: number;
  durationMs: number;
  blockingMs: number;
  /** Кто вызвал, по данным браузера: имя функции и её источник. */
  scripts: { name: string; sourceMs: number }[];
}

/// Фазы, которые действительно держат main thread. `fetch` и `pass`
/// ждут сеть, `digest` уходит в crypto.subtle, `blocked` — уже сумма;
/// мерить бюджет отзывчивости по ним значило бы записывать ожидание
/// канала в вину карте.
export const BLOCKING_PHASES: readonly Phase[] = [
  "render",
  "debug",
  "demand",
  "decode",
  "clamp",
  "labels",
];

/// Худший одиночный кусок работы за окно — то самое, из-за чего
/// слайдер отстаёт от пальца. Не сумма и не среднее: пятьдесят
/// честных двухмиллисекундных кадров ощущаются иначе, чем один
/// стомиллисекундный.
export interface WorstBlock {
  phase: Phase | null;
  ms: number;
  /** Что именно столько стоило, если фаза знает. Число говорит, что
   *  порвалось; адрес говорит, где чинить. */
  detail?: string;
}

export interface PhaseSummary {
  count: number;
  totalMs: number;
  maxMs: number;
  p95Ms: number;
}

/// Потолок кольцевого буфера. Один драг слайдера — это сотни спанов;
/// держать их все за сессию значит мерить память вместо времени.
const MAX_SPANS = 20_000;
const MAX_LONG_FRAMES = 500;

let enabled = false;
let spans: Span[] = [];
let longFrames: LongFrame[] = [];
let observer: PerformanceObserver | null = null;

/**
 * Замер синхронного участка. Возвращает то же, что и работа внутри, —
 * чтобы обёртка вставала в существующий код без перестановки строк.
 */
export function traced<T>(phase: Phase, work: () => T, detail?: string): T {
  if (!enabled) return work();
  const started = performance.now();
  try {
    return work();
  } finally {
    record(phase, started, performance.now(), detail);
  }
}

/**
 * То же для асинхронного участка. Меряет стену, а не блокировку: почти
 * всё время здесь — ожидание сети, и путать его с работой значит
 * объявить медленный канал медленной картой.
 */
export async function tracedAsync<T>(phase: Phase, work: () => Promise<T>): Promise<T> {
  if (!enabled) return work();
  const started = performance.now();
  try {
    return await work();
  } finally {
    record(phase, started, performance.now());
  }
}

/** Готовый замер, снятый чужим секундомером (например `blockedMs` загрузчика). */
export function recordSpan(phase: Phase, ms: number): void {
  if (!enabled) return;
  spans.push({ phase, ms, at: performance.now() - ms });
  if (spans.length > MAX_SPANS) spans.shift();
}

function record(phase: Phase, started: number, ended: number, detail?: string): void {
  spans.push({ phase, ms: ended - started, at: started, detail });
  if (spans.length > MAX_SPANS) spans.shift();
  // Те же интервалы попадают в запись DevTools, а не только в наш
  // буфер: два механизма для одного вопроса разошлись бы через месяц.
  performance.measure(`maps2:${phase}`, { start: started, end: ended });
}

function summary(): Record<string, PhaseSummary> {
  const byPhase = new Map<Phase, number[]>();
  for (const span of spans) {
    const list = byPhase.get(span.phase) ?? [];
    list.push(span.ms);
    byPhase.set(span.phase, list);
  }
  const out: Record<string, PhaseSummary> = {};
  for (const [phase, values] of byPhase) {
    const sorted = [...values].sort((a, b) => a - b);
    out[phase] = {
      count: sorted.length,
      totalMs: round(sorted.reduce((a, b) => a + b, 0)),
      maxMs: round(sorted[sorted.length - 1] ?? 0),
      p95Ms: round(sorted[Math.max(0, Math.ceil(sorted.length * 0.95) - 1)] ?? 0),
    };
  }
  return out;
}

function worstBlock(): WorstBlock {
  let worst: WorstBlock = { phase: null, ms: 0 };
  for (const span of spans) {
    if (!BLOCKING_PHASES.includes(span.phase)) continue;
    if (span.ms > worst.ms) worst = { phase: span.phase, ms: round(span.ms), detail: span.detail };
  }
  return worst;
}

function round(value: number): number {
  return Math.round(value * 100) / 100;
}

function enable(): void {
  if (enabled) return;
  enabled = true;
  // Долгие кадры считает браузер. Наши спаны отвечают, какая фаза
  // внутри вызова столько стоила; на вопрос «сколько именно main thread
  // не отвечал» честно отвечает только он сам.
  try {
    observer = new PerformanceObserver((list) => {
      for (const entry of list.getEntries() as PerformanceEntry[] &
        { blockingDuration?: number; scripts?: { name?: string; duration?: number }[] }[]) {
        longFrames.push({
          at: round(entry.startTime),
          durationMs: round(entry.duration),
          blockingMs: round(entry.blockingDuration ?? 0),
          scripts: (entry.scripts ?? []).map((script) => ({
            name: script.name ?? "?",
            sourceMs: round(script.duration ?? 0),
          })),
        });
        if (longFrames.length > MAX_LONG_FRAMES) longFrames.shift();
      }
    });
    observer.observe({ type: "long-animation-frame", buffered: true });
  } catch {
    // Браузер без Long Animation Frames: спаны остаются, атрибуции
    // кадров нет. Молча, потому что это не поломка лабы.
    observer = null;
  }
}

function reset(): void {
  spans = [];
  longFrames = [];
  performance.clearMeasures();
}

function log(): void {
  const worst = worstBlock();
  // eslint-disable-next-line no-console
  console.table(summary());
  // eslint-disable-next-line no-console
  console.log(
    `худший блок: ${worst.ms.toFixed(1)}ms (${worst.phase ?? "нет"}` +
      `${worst.detail ? ` ${worst.detail}` : ""})` +
      ` · долгих кадров ${longFrames.length}`,
  );
}

export interface PerfHandle {
  enable(): void;
  enabled(): boolean;
  reset(): void;
  summary(): Record<string, PhaseSummary>;
  longFrames(): LongFrame[];
  /** Худший одиночный блокирующий спан за окно, с именем фазы. */
  worstBlock(): WorstBlock;
  /** Худший долгий кадр по данным браузера. Ноль, если ни один кадр не
   *  перевалил за порог Long Animation Frames (50 мс) — это не значит
   *  «блокировок не было», это значит «крупных не было». */
  worstLongFrameMs(): number;
  spans(): Span[];
  log(): void;
}

declare global {
  interface Window {
    __maps2Perf?: PerfHandle;
  }
}

export function installPerfTrace(): void {
  window.__maps2Perf = {
    enable,
    enabled: () => enabled,
    reset,
    summary,
    longFrames: () => [...longFrames],
    worstBlock,
    worstLongFrameMs: () => longFrames.reduce((max, frame) => Math.max(max, frame.blockingMs), 0),
    spans: () => [...spans],
    log,
  };
  if (new URLSearchParams(location.search).has("perf")) enable();
}

/**
 * Показывать ли оверлей поверх сцены — `?perf=overlay`, отдельно от
 * `?perf=1`.
 *
 * Оверлей опрашивает `debug()` каждый кадр, а это сама по себе работа:
 * включённый во время замера, он и добавлял спаны, и не давал студии
 * успокоиться — сцена, за которой наблюдают, никогда не бывает
 * неподвижной. Человеку он нужен, замеру мешает.
 */
export function perfOverlayRequested(): boolean {
  return new URLSearchParams(location.search).get("perf") === "overlay";
}
