import "./styles/sections.css";

const NEAR_TOP = 80;

export function enableSections(
  rail: HTMLElement,
  pane: HTMLElement,
): () => void {
  const buttons = [
    ...rail.querySelectorAll<HTMLButtonElement>("button[data-section]"),
  ];
  const section = (button: HTMLButtonElement): HTMLElement | null =>
    document.getElementById(button.dataset.section ?? "");
  const mark = (current: HTMLButtonElement | undefined): void => {
    for (const button of buttons) {
      button.setAttribute("aria-current", String(button === current));
    }
  };
  const follow = (): void => {
    const shown = buttons.filter((button) => !button.hidden);
    const scrolls = pane.scrollHeight > pane.clientHeight;
    const atEnd = pane.scrollTop + pane.clientHeight >= pane.scrollHeight - 2;
    if (scrolls && pane.scrollTop > 0 && atEnd) {
      mark(shown.at(-1));
      return;
    }
    let current = shown[0];
    for (const button of shown) {
      const top = section(button)?.offsetTop ?? Infinity;
      if (top - pane.scrollTop <= NEAR_TOP) {
        current = button;
      }
    }
    mark(current);
  };
  let jumping = false;
  const stopJumping = (): void => {
    jumping = false;
  };
  for (const button of buttons) {
    button.addEventListener("click", () => {
      mark(button);
      const target = section(button);
      if (!target) {
        return;
      }
      const gap = parseFloat(getComputedStyle(pane).paddingTop);
      const furthest = pane.scrollHeight - pane.clientHeight;
      const top = Math.min(Math.max(target.offsetTop - gap, 0), furthest);
      jumping = Math.abs(top - pane.scrollTop) >= 1;
      pane.scrollTo({ top });
    });
  }
  pane.addEventListener("scrollend", stopJumping);
  for (const input of ["wheel", "pointerdown", "keydown"]) {
    pane.addEventListener(input, stopJumping, { passive: true });
  }
  let queued = false;
  pane.addEventListener("scroll", () => {
    if (jumping || queued) {
      return;
    }
    queued = true;
    requestAnimationFrame(() => {
      queued = false;
      follow();
    });
  });
  follow();
  return follow;
}
