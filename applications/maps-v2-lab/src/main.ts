import { cardById, CARDS } from "./cards";
import { destroyShowcase, showcase } from "./showcase";
import { el, stageEl } from "./ui";
import "./style.css";

// Хэш-роутер: #/card/<id> открывает одну карточку. Прямая ссылка — точка
// входа для e2e: тест загружает ровно одну фичу, без глобуса и скролла.

const app = document.querySelector<HTMLDivElement>("#app")!;

function header(): HTMLElement {
  return el("header", {}, [
    el("h1", {}, [el("a", { href: "#/showcase" }, ["maps-v2 lab"])]),
    el("span", { class: "tagline" }, [
      "deterministic 3D map studies",
    ]),
  ]);
}

// Реальный, копируемый пример — та же последовательность вызовов SDK,
// что использует каждая карточка ниже (см. src/sdk.ts createMap/loadPackCentre).
const QUICK_START_SNIPPET = `import { createMap, loadPackCentre } from "./sdk";

const canvas = document.querySelector("canvas")!;
const centre = await loadPackCentre("ealing"); // synthetic fixture; see "Пакет: загрузка по спросу" for a real package
const map = await createMap(canvas, "ealing");
map.setCentre(centre.lon, centre.lat);
map.setZoom(centre.zoom);
map.render();`;

function quickStart(): HTMLElement {
  return el("section", { class: "quick-start", "data-testid": "quick-start" }, [
    el("h2", {}, ["Quick start"]),
    el("p", {}, [
      "Every card below runs this same SDK call shape. Load your own MT2 package instead of a fixture through ",
      el("a", { href: "#/card/package-loader" }, ["Пакет: загрузка по спросу"]),
      ".",
    ]),
    el("pre", { class: "mono quick-start-code" }, [el("code", {}, [QUICK_START_SNIPPET])]),
  ]);
}

function renderIndex(): HTMLElement {
  const main = el("main", { "data-testid": "index" });
  main.append(quickStart());
  const groups = [...new Set(CARDS.map((card) => card.group))];
  for (const group of groups) {
    main.append(el("div", { class: "section-label" }, [group]));
    const grid = el("ul", { class: "card-grid" });
    for (const card of CARDS.filter((c) => c.group === group)) {
      grid.append(
        el("li", {}, [
          el("a", { href: `#/card/${card.id}` }, [
            el("h2", {}, [card.title]),
            el("p", {}, [card.purpose]),
            el("span", { class: "mono card-id" }, [card.id]),
          ]),
        ]),
      );
    }
    main.append(grid);
  }
  return main;
}

function renderCard(id: string): HTMLElement {
  const card = cardById(id);
  if (!card) {
    return el("main", {}, [
      el("p", {}, [`Нет карточки «${id}». `, el("a", { href: "#/" }, ["К списку"])]),
    ]);
  }
  const stage = stageEl(card.id);
  const panel = el("aside", { class: "panel" });
  card.mount(stage, panel);
  return el("main", { class: "card-page", "data-card": card.id }, [
    el("a", { class: "back", href: "#/" }, ["← все карточки"]),
    el("h1", {}, [card.title]),
    el("p", { class: "purpose" }, [card.purpose]),
    el("div", { class: "card-layout" }, [stage, panel]),
  ]);
}

function route(): void {
  const match = location.hash.match(/^#\/card\/([\w-]+)$/);
  const isShowcase = location.hash === "#/showcase";
  destroyShowcase();
  app.replaceChildren(
    el("div", { class: "shell" }, [
      header(),
      isShowcase ? showcase() : match?.[1] ? renderCard(match[1]) : renderIndex(),
    ]),
  );
}

window.addEventListener("hashchange", route);
route();
