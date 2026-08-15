// Мелкие DOM-хелперы. Без фреймворка: карточек девять, состояние у каждой
// своё и плоское, а e2e читает данные из data-атрибутов, не из виртуального DOM.

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Record<string, string> = {},
  children: (Node | string)[] = [],
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    node.setAttribute(key, value);
  }
  node.append(...children);
  return node;
}

export const PENDING = "—";

export interface Readout {
  root: HTMLElement;
  set(key: string, value: string): void;
}

// Таблица показаний. Значение PENDING означает «SDK ещё не отвечает» —
// именно это состояние e2e-тест увидит красным, пока фича не реализована.
export function readout(rows: { key: string; label: string }[]): Readout {
  const dds = new Map<string, HTMLElement>();
  const root = el("dl", { class: "readout" });
  for (const row of rows) {
    const dd = el(
      "dd",
      { "data-testid": `readout-${row.key}`, "data-pending": "" },
      [PENDING],
    );
    dds.set(row.key, dd);
    root.append(el("div", {}, [el("dt", {}, [row.label]), dd]));
  }
  return {
    root,
    set(key, value) {
      const dd = dds.get(key);
      if (!dd) throw new Error(`no readout row: ${key}`);
      dd.textContent = value;
      if (value === PENDING) dd.setAttribute("data-pending", "");
      else dd.removeAttribute("data-pending");
    },
  };
}

export function controlRow(
  label: string,
  control: HTMLElement,
  testId?: string,
): HTMLElement {
  const row = el("div", { class: "control-row" }, [
    el("span", {}, [label]),
    control,
  ]);
  if (testId) row.setAttribute("data-testid", testId);
  return row;
}

export function switchControl(
  checked: boolean,
  testId: string,
  onChange: (on: boolean) => void,
): HTMLElement {
  const input = el("input", { type: "checkbox", "data-testid": testId });
  input.checked = checked;
  input.addEventListener("change", () => onChange(input.checked));
  return el("label", { class: "switch" }, [
    input,
    el("span", { class: "track" }),
  ]);
}

export function section(title: string, body: HTMLElement): HTMLElement {
  return el("section", {}, [el("h2", {}, [title]), body]);
}

// Сцена. data-state="unimplemented" — честное состояние «рендерер maps-v2
// ещё не подключён»; карточка меняет его, когда появляется чем рисовать.
export function stageEl(cardId: string): HTMLElement {
  return el(
    "div",
    {
      class: "stage",
      "data-testid": "stage",
      "data-card": cardId,
      "data-state": "unimplemented",
      tabindex: "0",
    },
    ["рендерер maps-v2 ещё не подключён"],
  );
}
