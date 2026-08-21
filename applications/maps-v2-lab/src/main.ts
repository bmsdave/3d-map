import { cardById } from "./cards";
import { destroyHome, home } from "./home";
import { destroyShowcase, showcase } from "./showcase";
import { el, stageEl } from "./ui";
import "./style.css";

// Хэш-роутер. «#/» — живая доска: все студии смонтированы сразу, без
// перехода по ссылке. «#/card/<id>» открывает одну студию крупно и
// остаётся точкой входа для e2e: тест загружает ровно одну фичу.

const app = document.querySelector<HTMLDivElement>("#app")!;

function header(route: string): HTMLElement {
  const link = (href: string, text: string): HTMLElement =>
    el("a", href === route ? { href, "aria-current": "page" } : { href }, [text]);
  return el("header", {}, [
    el("h1", {}, [el("a", { href: "#/" }, ["maps-v2 lab"])]),
    el("span", { class: "tagline" }, ["deterministic 3D map studies"]),
    el("nav", { class: "top-nav" }, [link("#/", "Board"), link("#/showcase", "Showcase")]),
  ]);
}

function renderCard(id: string): HTMLElement {
  const card = cardById(id);
  if (!card) {
    return el("main", {}, [
      el("p", {}, [`Нет карточки «${id}». `, el("a", { href: "#/" }, ["К доске"])]),
    ]);
  }
  const stage = stageEl(card.id);
  const panel = el("aside", { class: "panel" });
  card.mount(stage, panel);
  return el("main", { class: "card-page", "data-card": card.id }, [
    el("a", { class: "back", href: "#/" }, ["← вся доска"]),
    el("h1", {}, [card.title]),
    el("p", { class: "purpose" }, [card.purpose]),
    el("div", { class: "card-layout" }, [stage, panel]),
  ]);
}

function view(): { route: string; body: HTMLElement } {
  const match = location.hash.match(/^#\/card\/([\w-]+)$/);
  if (match?.[1]) return { route: `#/card/${match[1]}`, body: renderCard(match[1]) };
  if (location.hash === "#/showcase") return { route: "#/showcase", body: showcase() };
  return { route: "#/", body: home() };
}

function route(): void {
  destroyShowcase();
  destroyHome();
  const { route: current, body } = view();
  app.replaceChildren(el("div", { class: "shell", "data-route": current }, [header(current), body]));
}

window.addEventListener("hashchange", route);
route();
