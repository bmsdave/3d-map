import { cardById } from "./cards";
import { destroyHome, home } from "./home";
import { PerfOverlay } from "./perfOverlay";
import { installPerfTrace, perfOverlayRequested } from "./perfTrace";
import { destroyShowcase, showcase } from "./showcase";
import { el, stageEl } from "./ui";
import "./style.css";

// Хэш-роутер. «#/» — живая доска: все студии смонтированы сразу, без
// перехода по ссылке. «#/card/<id>» открывает одну студию крупно и
// остаётся точкой входа для e2e: тест загружает ровно одну фичу.

const app = document.querySelector<HTMLDivElement>("#app")!;
let activeCleanup: (() => void) | null = null;
let activeOverlayCancel: (() => void) | null = null;

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
  const c = card.mount(stage, panel);
  if (typeof c === "function") activeCleanup = c;
  else if (typeof card.unmount === "function") activeCleanup = card.unmount;
  else activeCleanup = null;
  if (perfOverlayRequested()) activeOverlayCancel = attachOverlay(stage);
  return el("main", { class: "card-page", "data-card": card.id }, [
    el("a", { class: "back", href: "#/" }, ["← вся доска"]),
    el("h1", {}, [card.title]),
    el("p", { class: "purpose" }, [card.purpose]),
    el("div", { class: "card-layout" }, [stage, panel]),
  ]);
}

/**
 * Показания кадра поверх любой студии, а не одной.
 *
 * Оверлей был встроен в карточку map-real за галочкой — то есть
 * единственная студия, у которой можно было спросить, куда ушло время,
 * оказалась той, где его меньше всего. Здесь он навешивается снаружи, на
 * готовую сцену: карточкам про него знать незачем.
 */
function attachOverlay(stage: HTMLElement): () => void {
  const overlay = new PerfOverlay();
  stage.style.position = "relative";
  stage.append(overlay.root);
  let frame = 0;
  const draw = () => {
    const map = window.maps2;
    if (!stage.isConnected) return;
    if (map) {
      overlay.beginHostWork();
      overlay.endHostWork(map);
    }
    frame = requestAnimationFrame(draw);
  };
  frame = requestAnimationFrame(draw);
  return () => cancelAnimationFrame(frame);
}

function view(): { route: string; body: HTMLElement } {
  const match = location.hash.match(/^#\/card\/([\w-]+)$/);
  if (match?.[1]) return { route: `#/card/${match[1]}`, body: renderCard(match[1]) };
  if (location.hash === "#/showcase") return { route: "#/showcase", body: showcase() };
  return { route: "#/", body: home() };
}

// Каждая студия рисует настоящую землю, а у настоящей земли есть
// правообладатели: ODbL требует называть источник там, где показана
// карта, а не только в манифесте пакета. Одна строка на всю лабу —
// каждая студия смотрит на одну и ту же вырезку.
function attribution(): HTMLElement {
  return el("footer", { class: "attribution", "data-testid": "attribution" }, [
    "Данные: © участники ",
    el("a", { href: "https://www.openstreetmap.org/copyright" }, ["OpenStreetMap"]),
    " (ODbL) · рельеф суши Copernicus DEM (© DLR e.V. 2010–2014, © Airbus DS 2014–2018, ESA/EU) · батиметрия ",
    el("a", { href: "https://www.gebco.net" }, ["GEBCO 2026"]),
    " · границы и города Natural Earth.",
  ]);
}

function route(): void {
  try { activeCleanup?.(); } catch {}
  try { activeOverlayCancel?.(); } catch {}
  activeCleanup = null;
  activeOverlayCancel = null;
  destroyShowcase();
  destroyHome();
  const { route: current, body } = view();
  app.replaceChildren(
    el("div", { class: "shell", "data-route": current }, [header(current), body, attribution()]),
  );
}

installPerfTrace();
window.addEventListener("hashchange", route);
route();
