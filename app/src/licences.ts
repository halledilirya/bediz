export const NOTICES = ["/licences/rust.txt", "/licences/frontend.txt"];

export interface Answer {
  ok: boolean;
  type: string | null;
  text: () => Promise<string>;
}

export type Fetch = (path: string) => Promise<Answer>;

async function notice(
  fetcher: Fetch,
  path: string,
  missing: string,
): Promise<string> {
  try {
    const answer = await fetcher(path);
    const page = (answer.type ?? "").includes("text/html");
    return answer.ok && !page ? await answer.text() : missing;
  } catch {
    return missing;
  }
}

export async function readNotices(
  fetcher: Fetch,
  missing: string,
): Promise<string> {
  const parts = await Promise.all(
    NOTICES.map((path) => notice(fetcher, path, missing)),
  );
  return [...new Set(parts)].join("\n\n");
}
