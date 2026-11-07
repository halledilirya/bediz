export type Text = (id: string, args?: Record<string, string>) => string;

export function explain(failure: unknown, text: Text): string {
  if (typeof failure === "object" && failure !== null) {
    const { kind, path, reason } = failure as Record<string, unknown>;
    if (kind === "missing" && typeof path === "string") {
      return text("settings-reason-missing", { path });
    }
    if (kind === "windows" && typeof reason === "string") {
      return reason;
    }
  }
  return text("settings-reason-unexpected");
}
