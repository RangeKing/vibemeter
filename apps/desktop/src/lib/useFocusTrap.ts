import { useEffect, useRef, type RefObject } from "react";

const FOCUSABLE = "a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex='-1'])";

/** Trap Tab focus inside a modal dialog while `active` is true; restore focus on close. */
export function useFocusTrap(active: boolean): RefObject<HTMLElement | null> {
  const containerRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!active) return;
    const node = containerRef.current;
    if (!node) return;
    const previouslyFocused = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const focusables = () => [...node.querySelectorAll<HTMLElement>(FOCUSABLE)]
      .filter((element) => element.tabIndex !== -1 && !element.hasAttribute("disabled"));
    focusables()[0]?.focus();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const items = focusables();
      if (!items.length) {
        event.preventDefault();
        return;
      }
      const firstItem = items[0];
      const lastItem = items[items.length - 1];
      if (event.shiftKey && document.activeElement === firstItem) {
        event.preventDefault();
        lastItem.focus();
      } else if (!event.shiftKey && document.activeElement === lastItem) {
        event.preventDefault();
        firstItem.focus();
      }
    };
    node.addEventListener("keydown", onKeyDown);
    return () => {
      node.removeEventListener("keydown", onKeyDown);
      previouslyFocused?.focus();
    };
  }, [active]);

  return containerRef;
}
