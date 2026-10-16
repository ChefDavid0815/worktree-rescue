import "@testing-library/jest-dom/vitest";

// jsdom lacks a few browser APIs the UI relies on.
class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as unknown as { ResizeObserver: typeof RO }).ResizeObserver ??= RO;
if (!window.matchMedia) {
  window.matchMedia = ((q: string) => ({
    matches: false,
    media: q,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}
Element.prototype.scrollIntoView ??= function () {};

// Give layout boxes a size so virtualized lists render rows under jsdom.
Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 800 });
Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 1000 });
HTMLElement.prototype.getBoundingClientRect = function () {
  return { x: 0, y: 0, top: 0, left: 0, bottom: 800, right: 1000, width: 1000, height: 800, toJSON() {} } as DOMRect;
};
Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 800 });
Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 1000 });
