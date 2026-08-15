import { cardById, CARDS } from "./cards";
import { el, stageEl } from "./ui";
import "./style.css";

// Хэш-роутер: #/card/<id> открывает одну карточку. Прямая ссылка — точка
// входа для e2e: тест загружает ровно одну фичу, без глобуса и скролла.

const app = document.querySelector<HTMLDivElement>("#app")!;

function header(): HTMLElement {
  return el("header", {}, [
    el("h1", {}, [el("a", { href: "#/" }, ["maps-v2 lab"])]),
    el("span", { class: "tagline" }, [
      "атомарные карточки для TDD-разработки SDK",
    ]),
  ]);
}

function renderIndex(): HTMLElement {
  const main = el("main", { "data-testid": "index" });
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
  app.replaceChildren(
    el("div", { class: "shell" }, [
      header(),
      match?.[1] ? renderCard(match[1]) : renderIndex(),
    ]),
  );
}

window.addEventListener("hashchange", route);
route();
