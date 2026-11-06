export const LONGEST = 120;

const TOKENS = ["date", "time", "width", "height"];
const FORBIDDEN = /[\\/:*?"<>|]|\p{Cc}/u;

export type Problem =
  | { kind: "empty" }
  | { kind: "long" }
  | { kind: "forbidden" }
  | { kind: "unknown"; token: string };

export interface Sample {
  date: string;
  time: string;
  width: number;
  height: number;
}

export function problem(pattern: string): Problem | null {
  if (pattern.trim() === "") {
    return { kind: "empty" };
  }
  if (Array.from(pattern).length > LONGEST) {
    return { kind: "long" };
  }
  if (FORBIDDEN.test(pattern)) {
    return { kind: "forbidden" };
  }
  for (const after of pattern.split("{").slice(1)) {
    const end = after.indexOf("}");
    if (end === -1) {
      return { kind: "unknown", token: `{${after}` };
    }
    const token = after.slice(0, end);
    if (!TOKENS.includes(token)) {
      return { kind: "unknown", token: `{${token}}` };
    }
  }
  return null;
}

export function stem(pattern: string, sample: Sample): string {
  return pattern
    .replaceAll("{date}", sample.date)
    .replaceAll("{time}", sample.time)
    .replaceAll("{width}", String(sample.width))
    .replaceAll("{height}", String(sample.height))
    .trim();
}

export function sampleAt(now: Date, width: number, height: number): Sample {
  const two = (value: number): string => String(value).padStart(2, "0");
  return {
    date: `${String(now.getFullYear())}-${two(now.getMonth() + 1)}-${two(now.getDate())}`,
    time: `${two(now.getHours())}${two(now.getMinutes())}${two(now.getSeconds())}`,
    width,
    height,
  };
}
