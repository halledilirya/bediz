export function applyLook(theme: string, accent: string): void {
  const root = document.documentElement;
  if (theme === "light" || theme === "dark") {
    root.dataset.theme = theme;
  } else {
    delete root.dataset.theme;
  }
  if (accent === "blue") {
    delete root.dataset.accent;
  } else {
    root.dataset.accent = accent;
  }
}
