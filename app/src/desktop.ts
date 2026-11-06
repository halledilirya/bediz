import "./styles/desktop.css";

function isTextField(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLTextAreaElement ||
    (target instanceof HTMLInputElement &&
      (target.type === "text" || target.type === "search"))
  );
}

export function behaveLikeAnApp(): void {
  document.addEventListener("contextmenu", (event) => {
    if (!isTextField(event.target)) {
      event.preventDefault();
    }
  });
  document.addEventListener("dragstart", (event) => {
    event.preventDefault();
  });
  document.addEventListener("mousedown", (event) => {
    if (event.button === 1) {
      event.preventDefault();
    }
  });
}
