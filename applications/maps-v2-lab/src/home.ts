import { FIXED_CENTRE } from "./bands";
import { CARDS } from "./cards";
import type { CardSpec } from "./cards/types";
import { createPackageMap } from "./sdk";
import { el, stageEl } from "./ui";

// Живая доска: главная страница — это и есть лаборатория. Каждая студия
// смонтирована на месте, без перехода по ссылке; ссылка на отдельную
// страницу остаётся точкой входа для e2e и для «одна фича крупно».

/// Контекст WebGL2 — ресурс с потолком на вкладку: Chrome начинает ронять
/// самые старые примерно на шестнадцатом. Двадцать одновременно живых
/// студий поэтому убили бы верх доски молча. Доска держит живыми только
/// ближайшие к вьюпорту и возвращает контекст, когда студия уезжает, —
/// это же удерживает честным бюджет кадра, пока всё лежит на одном экране.
const LIVE_BUDGET = 6;

/// За сколько до появления в кадре студия начинает рисовать. Примерно
/// экран: к моменту, когда до неё доскроллили, она уже нарисована.
const PRELOAD_MARGIN_PX = 700;

/// Сколько один кадр героя стоит на месте, прежде чем уехать к следующему.
const MOMENT_MS = 9000;

interface Moment {
  label: string;
  caption: string;
  zoom: number;
  tilt: number;
}

// Один пакет, одна карта, один контекст: моменты двигают камеру, а не
// пересоздают карту, иначе герой стоил бы трёх студий.
const HERO_MOMENTS: readonly Moment[] = [
  { label: "Micro · z16.4", caption: "Trafalgar Square: buildings, roofs, and the joins of the Strand", zoom: 16.4, tilt: 42 },
  { label: "Street · z13.8", caption: "Westminster and the river, with their named places", zoom: 13.8, tilt: 26 },
  { label: "City · z10.2", caption: "Greater London, where the bands hand composition over", zoom: 10.2, tilt: 12 },
];

// Реальный, копируемый пример — та же последовательность вызовов SDK,
// что выполняет каждая студия ниже (см. src/sdk.ts createPackageMap).
const QUICK_START_SNIPPET = `import { createPackageMap } from "./sdk";

const canvas = document.querySelector("canvas")!;
// Real OSM, Copernicus and GEBCO tiles, demand-loaded around
// Trafalgar Square; camera moves pull the tiles they need.
const { map } = await createPackageMap(canvas, { zoom: 16 });
map.setTilt(42);
map.render();`;

interface Slot {
  card: CardSpec;
  root: HTMLElement;
  stage: HTMLElement;
  panel: HTMLElement;
  live: boolean;
  near: boolean;
}

interface Group {
  name: string;
  chip: HTMLElement;
  slots: Slot[];
}

interface Board {
  slots: Slot[];
  groups: Group[];
  observer: IntersectionObserver | null;
  destroyed: boolean;
  pending: boolean;
  motion: boolean;
  liveCount: HTMLElement;
  onScroll: () => void;
  stopHero: (() => void) | null;
  allChip: HTMLElement | null;
  query: string;
  group: string | null;
}

let activeBoard: Board | null = null;

export function home(): HTMLElement {
  destroyHome();
  const liveCount = el("strong", { "data-testid": "live-count" }, ["0"]);
  const board: Board = {
    slots: [],
    groups: [],
    observer: null,
    destroyed: false,
    pending: false,
    motion: true,
    liveCount,
    onScroll: () => {},
    stopHero: null,
    allChip: null,
    query: "",
    group: null,
  };
  activeBoard = board;

  const root = el("main", {
    class: "home",
    "data-testid": "home",
    "data-studies": String(CARDS.length),
    "data-live-budget": String(LIVE_BUDGET),
  });
  root.append(heroSection(board));

  // Одна плотная сетка на все двадцать студий. Группа — не строка, а
  // метка на плитке и фильтр в панели: половина групп состоит из одной
  // студии, и заголовок-строка оставлял бы после каждой пустую полосу
  // в три колонки шириной.
  const grid = el("div", { class: "study-grid", "data-testid": "study-grid" });
  for (const name of [...new Set(CARDS.map((card) => card.group))]) {
    const slots = CARDS.filter((card) => card.group === name).map((card) => {
      const slot = createSlot(card);
      board.slots.push(slot);
      grid.append(slot.root);
      return slot;
    });
    board.groups.push({ name, chip: groupChip(board, name, slots.length), slots });
  }

  root.append(toolbar(board), grid);

  board.observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        const slot = board.slots.find((candidate) => candidate.root === entry.target);
        if (slot) slot.near = entry.isIntersecting;
      }
      schedule(board);
    },
    { rootMargin: `${PRELOAD_MARGIN_PX}px 0px` },
  );
  for (const slot of board.slots) board.observer.observe(slot.root);

  board.onScroll = () => schedule(board);
  window.addEventListener("scroll", board.onScroll, { passive: true });
  window.addEventListener("resize", board.onScroll, { passive: true });
  return root;
}

export function destroyHome(): void {
  const board = activeBoard;
  if (!board) return;
  board.destroyed = true;
  board.observer?.disconnect();
  window.removeEventListener("scroll", board.onScroll);
  window.removeEventListener("resize", board.onScroll);
  board.stopHero?.();
  for (const slot of board.slots) release(slot);
  activeBoard = null;
}

// ---- hero ----

function heroSection(board: Board): HTMLElement {
  const canvas = el("canvas", { width: "960", height: "600" });
  const label = el("span", { class: "hero-label", "data-testid": "hero-label" }, [HERO_MOMENTS[0]!.label]);
  const caption = el("p", { class: "hero-caption" }, [HERO_MOMENTS[0]!.caption]);
  const stage = el("div", { class: "hero-stage", "data-testid": "hero-stage", "data-state": "loading" }, [
    canvas,
    el("div", { class: "hero-overlay" }, [label, caption]),
  ]);
  void mountHero(canvas, stage, label, caption, board);

  return el("section", { class: "home-hero" }, [
    el("div", { class: "hero-copy" }, [
      el("p", { class: "eyebrow" }, ["3D Maps SDK v2 · alpha"]),
      el("h1", {}, ["The lab, running."]),
      el("p", { class: "hero-lead" }, [
        `Every study below renders live on this page — ${CARDS.length} of them, no click-through. `,
        "Real OpenStreetMap, Copernicus and GEBCO tiles around Trafalgar Square, WebGL2, one Rust core.",
      ]),
      el("pre", { class: "mono quick-start-code", "data-testid": "quick-start" }, [
        el("code", {}, [QUICK_START_SNIPPET]),
      ]),
      el("p", { class: "hero-note" }, [
        "Point a study at your own MT2 package through ",
        el("a", { href: "#/card/package-loader" }, ["Пакет: загрузка по спросу"]),
        ", or watch the animated gallery in the ",
        el("a", { href: "#/showcase" }, ["showcase"]),
        ".",
      ]),
    ]),
    stage,
  ]);
}

async function mountHero(
  canvas: HTMLCanvasElement,
  stage: HTMLElement,
  label: HTMLElement,
  caption: HTMLElement,
  board: Board,
): Promise<void> {
  try {
    const { map } = await createPackageMap(canvas, { zoom: HERO_MOMENTS[0]!.zoom, centre: FIXED_CENTRE });
    if (board.destroyed) {
      loseContexts(stage);
      return;
    }
    stage.setAttribute("data-state", "ready");
    const started = performance.now();
    let shown = -1;
    let frame = 0;
    const draw = (now: number) => {
      if (board.destroyed) return;
      frame = requestAnimationFrame(draw);
      if (!board.motion) return;
      // Отметка времени кадра может быть чуть раньше момента подписки:
      // rAF отдаёт время начала кадра, а не вызова. Без зажима первый
      // кадр берёт отрицательный индекс момента.
      const elapsed = Math.max(0, now - started) / MOMENT_MS;
      const index = Math.floor(elapsed) % HERO_MOMENTS.length;
      const from = HERO_MOMENTS[index]!;
      const to = HERO_MOMENTS[(index + 1) % HERO_MOMENTS.length]!;
      // Кадр стоит две трети времени и только потом уезжает — иначе
      // камера всё время в движении и ни один момент не читается.
      const k = smoothstep(Math.max(0, (elapsed % 1) - 0.66) / 0.34);
      map.setZoom(mix(from.zoom, to.zoom, k));
      map.setTilt(mix(from.tilt, to.tilt, k));
      map.setBearing(Math.max(0, now - started) / 110);
      map.render();
      if (index !== shown) {
        shown = index;
        label.textContent = from.label;
        caption.textContent = from.caption;
      }
      stage.setAttribute("data-frame", String(Math.floor(now)));
    };
    frame = requestAnimationFrame(draw);
    board.stopHero = () => {
      cancelAnimationFrame(frame);
      loseContexts(stage);
    };
  } catch (error: unknown) {
    stage.setAttribute("data-state", "error");
    stage.setAttribute("data-error", String(error));
  }
}

function mix(from: number, to: number, k: number): number {
  return from + (to - from) * k;
}

function smoothstep(k: number): number {
  const clamped = Math.min(1, Math.max(0, k));
  return clamped * clamped * (3 - 2 * clamped);
}

// ---- toolbar ----

function toolbar(board: Board): HTMLElement {
  const filter = el("input", {
    type: "search",
    class: "study-filter",
    placeholder: "Filter studies…",
    "aria-label": "Filter studies",
    "data-testid": "study-filter",
  });
  filter.addEventListener("input", () => {
    board.query = filter.value;
    applyFilter(board);
  });

  const motion = el("button", { type: "button", class: "toolbar-button", "data-testid": "motion-toggle" }, [
    "Pause motion",
  ]);
  motion.addEventListener("click", () => {
    board.motion = !board.motion;
    motion.textContent = board.motion ? "Pause motion" : "Play motion";
    motion.setAttribute("data-motion", String(board.motion));
  });

  const all = el("button", {
    type: "button",
    class: "chip",
    "data-group": "",
    "aria-pressed": "true",
  }, ["Все", el("span", { class: "group-count" }, [String(board.slots.length)])]);
  all.addEventListener("click", () => selectGroup(board, null));
  board.allChip = all;

  return el("div", { class: "home-toolbar", "data-testid": "home-toolbar" }, [
    el("nav", { class: "toolbar-groups", "aria-label": "Группы студий" }, [
      all,
      ...board.groups.map((group) => group.chip),
    ]),
    el("div", { class: "toolbar-right" }, [
      filter,
      el("span", { class: "live-count" }, [board.liveCount, ` / ${board.slots.length} live`]),
      motion,
    ]),
  ]);
}

function groupChip(board: Board, name: string, count: number): HTMLElement {
  const chip = el("button", {
    type: "button",
    class: "chip",
    "data-group": name,
    "aria-pressed": "false",
  }, [name, el("span", { class: "group-count" }, [String(count)])]);
  chip.addEventListener("click", () => selectGroup(board, board.group === name ? null : name));
  return chip;
}

function selectGroup(board: Board, name: string | null): void {
  board.group = name;
  board.allChip?.setAttribute("aria-pressed", String(name === null));
  for (const group of board.groups) {
    group.chip.setAttribute("aria-pressed", String(group.name === name));
  }
  applyFilter(board);
  window.scrollTo({ top: gridTop(board), behavior: "smooth" });
}

function gridTop(board: Board): number {
  const first = board.slots.find((slot) => !slot.root.hidden);
  if (!first) return window.scrollY;
  return window.scrollY + first.root.getBoundingClientRect().top - 96;
}

function applyFilter(board: Board): void {
  const needle = board.query.trim().toLowerCase();
  for (const slot of board.slots) {
    const hay = `${slot.card.title} ${slot.card.id} ${slot.card.group} ${slot.card.purpose}`.toLowerCase();
    const hit = (needle === "" || hay.includes(needle))
      && (board.group === null || slot.card.group === board.group);
    slot.root.hidden = !hit;
    if (!hit) release(slot);
  }
  schedule(board);
}

// ---- studies ----

function createSlot(card: CardSpec): Slot {
  const stage = idleStage(card, "waiting for a GPU slot");
  const panel = el("aside", { class: "panel" });
  const root = el("article", {
    class: "study",
    "data-testid": "study",
    "data-card": card.id,
    "data-live": "false",
  }, [
    el("div", { class: "study-head" }, [
      el("span", { class: "study-group" }, [card.group]),
      el("h2", {}, [card.title]),
      el("a", {
        class: "mono study-open",
        href: `#/card/${card.id}`,
        "aria-label": `Открыть «${card.title}» отдельной страницей`,
      }, [card.id]),
    ]),
    el("p", { class: "study-purpose" }, [card.purpose]),
    stage,
    panel,
  ]);
  return { card, root, stage, panel, live: false, near: false };
}

function idleStage(card: CardSpec, note: string): HTMLElement {
  const stage = stageEl(card.id);
  stage.setAttribute("data-state", "queued");
  stage.replaceChildren(note);
  return stage;
}

function mount(slot: Slot): void {
  if (slot.live) return;
  slot.live = true;
  slot.root.setAttribute("data-live", "true");
  slot.card.mount(slot.stage, slot.panel);
}

function release(slot: Slot): void {
  if (!slot.live) return;
  slot.live = false;
  slot.root.setAttribute("data-live", "false");
  const spentStage = slot.stage;
  const spentPanel = slot.panel;
  slot.stage = idleStage(slot.card, "GPU slot released — scroll back to restore");
  slot.panel = el("aside", { class: "panel" });
  spentStage.replaceWith(slot.stage);
  spentPanel.replaceWith(slot.panel);
  loseContexts(spentStage);
  // Монтирование могло быть ещё в полёте: его карта создаст контекст уже
  // после этой строки, в узле, который никто больше не держит. Второй
  // проход по нему — единственный способ вернуть этот контекст.
  setTimeout(() => loseContexts(spentStage), 2000);
}

function loseContexts(root: HTMLElement): void {
  root.querySelectorAll("canvas").forEach((canvas) => {
    canvas.getContext("webgl2")?.getExtension("WEBGL_lose_context")?.loseContext();
  });
}

function schedule(board: Board): void {
  if (board.destroyed || board.pending) return;
  board.pending = true;
  requestAnimationFrame(() => {
    board.pending = false;
    if (!board.destroyed) balance(board);
  });
}

// Живыми остаются ближайшие к центру экрана — и ровно столько, сколько
// разрешает бюджет. Освобождение идёт раньше монтирования: контекст,
// который сейчас понадобится, должен сначала вернуться.
function balance(board: Board): void {
  const middle = window.innerHeight / 2;
  const wanted = board.slots
    .filter((slot) => slot.near && !slot.root.hidden)
    .map((slot) => {
      const box = slot.root.getBoundingClientRect();
      return { slot, distance: Math.abs(box.top + box.height / 2 - middle) };
    })
    .sort((left, right) => left.distance - right.distance)
    .slice(0, LIVE_BUDGET)
    .map((entry) => entry.slot);
  const keep = new Set(wanted);
  for (const slot of board.slots) if (!keep.has(slot)) release(slot);
  for (const slot of wanted) mount(slot);
  board.liveCount.textContent = String(board.slots.filter((slot) => slot.live).length);
}
