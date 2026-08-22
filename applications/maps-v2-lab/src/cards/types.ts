export interface CardSpec {
  id: string;
  title: string;
  // Одна строка: что именно эта карточка позволяет проверить атомарно.
  purpose: string;
  group: string;
  mount(stage: HTMLElement, panel: HTMLElement): (() => void) | void;
  unmount?: () => void;
}
