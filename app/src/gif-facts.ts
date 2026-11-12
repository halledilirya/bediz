const KILOBYTE = 1024;
const MEGABYTE = KILOBYTE * KILOBYTE;

export function size(bytes: number, language: string): string {
  if (bytes < MEGABYTE) {
    return new Intl.NumberFormat(language, {
      style: "unit",
      unit: "kilobyte",
      maximumFractionDigits: 0,
    }).format(Math.max(1, Math.round(bytes / KILOBYTE)));
  }
  return new Intl.NumberFormat(language, {
    style: "unit",
    unit: "megabyte",
    maximumFractionDigits: 1,
  }).format(bytes / MEGABYTE);
}

export function length(milliseconds: number, language: string): string {
  return new Intl.NumberFormat(language, {
    style: "unit",
    unit: "second",
    maximumFractionDigits: 1,
  }).format(milliseconds / 1000);
}

export function percent(value: number, language: string): string {
  return new Intl.NumberFormat(language, { style: "percent" }).format(
    value / 100,
  );
}

export function limit(seconds: number, language: string): string {
  const minutes = seconds >= 60 && seconds % 60 === 0;
  return new Intl.NumberFormat(language, {
    style: "unit",
    unit: minutes ? "minute" : "second",
    unitDisplay: "long",
  }).format(minutes ? seconds / 60 : seconds);
}
