/**
 * DOM rendering for interaction tests. Needs the jsdom environment:
 * put `// @vitest-environment jsdom` at the top of the test file.
 *
 * Effects run, timers can be faked with `vi.useFakeTimers()`, and backend calls are
 * answered through `invoke` from `tauriMock` with the `deferred` promises below.
 */
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { vi } from "vitest";

declare global {
  // React reads this global to allow act() outside a test renderer.
  // oxlint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

export interface Mounted {
  container: HTMLElement;
  /** Render a new element into the same root (prop change). */
  rerender: (element: ReactElement) => Promise<void>;
  unmount: () => Promise<void>;
  /** Visible text with whitespace collapsed. */
  text: () => string;
}

/** Mount `element` into a fresh root attached to `document.body`. */
export async function mount(element: ReactElement): Promise<Mounted> {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  await act(async () => root.render(element));
  return {
    container,
    rerender: (next) => act(async () => root.render(next)),
    unmount: async () => {
      await act(async () => root.unmount());
      container.remove();
    },
    text: () => (container.textContent ?? "").replace(/\s+/g, " ").trim(),
  };
}

/** Let pending microtasks and React updates settle. */
export function flush(): Promise<void> {
  return act(async () => {
    await Promise.resolve();
  });
}

/** Advance fake timers inside act() so scheduled polls and their state updates run. */
export function advance(ms: number): Promise<void> {
  return act(async () => {
    vi.advanceTimersByTime(ms);
    await Promise.resolve();
  });
}

export interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
}

/** A promise settled by the test, standing in for one backend call. */
export function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Fire a DOM event on `el` inside act(). */
export function fire(el: Element, type: string, init: EventInit = {}): Promise<void> {
  return act(async () => {
    el.dispatchEvent(new Event(type, { bubbles: true, ...init }));
  });
}

/** Toggle a checkbox through React's onChange (click flips `checked` in jsdom). */
export function click(el: Element): Promise<void> {
  return act(async () => {
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}
