/** Tiny JSC-safe HTML helpers (no DOM / cheerio). */

export const decodeEntities = (value: string): string =>
  value
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&quot;/g, '"')
    .replace(/&#39;|&apos;/g, "'")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&#(\d+);/g, (_, code: string) =>
      String.fromCharCode(Number(code)),
    )
    .replace(/&#x([0-9a-f]+);/gi, (_, code: string) =>
      String.fromCharCode(Number.parseInt(code, 16)),
    );

export const stripTags = (value: string): string =>
  decodeEntities(value.replace(/<[^>]+>/g, " ")).replace(/\s+/g, " ").trim();

export const attr = (tag: string, name: string): string | undefined => {
  const match = tag.match(
    new RegExp(`\\b${name}\\s*=\\s*(?:"([^"]*)"|'([^']*)'|([^\\s>]+))`, "i"),
  );
  const raw = match?.[1] ?? match?.[2] ?? match?.[3];
  return raw == null ? undefined : decodeEntities(raw);
};

export const firstMatch = (
  html: string,
  pattern: RegExp,
): string | undefined => {
  const match = pattern.exec(html);
  return match?.[1];
};

export const allMatches = (html: string, pattern: RegExp): string[] => {
  const flags = pattern.flags.includes("g")
    ? pattern.flags
    : `${pattern.flags}g`;
  const global = new RegExp(pattern.source, flags);
  const values: string[] = [];
  let match: RegExpExecArray | null;
  while ((match = global.exec(html))) {
    if (match[1] != null) values.push(match[1]);
  }
  return values;
};

/** Extract inner HTML of the first element matching an opening tag regex. */
export const innerHtml = (
  html: string,
  openTagPattern: RegExp,
): string | undefined => {
  const open = openTagPattern.exec(html);
  if (!open || open.index == null) return undefined;
  const start = open.index + open[0].length;
  const tagName = open[0].match(/^<\/?([a-z0-9]+)/i)?.[1]?.toLowerCase();
  if (!tagName) return undefined;
  const rest = html.slice(start);
  const close = new RegExp(`</${tagName}\\s*>`, "i").exec(rest);
  return close ? rest.slice(0, close.index) : rest;
};

/** Collect balanced blocks for a tag that matches `openPattern`. */
export const collectBlocks = (
  html: string,
  openPattern: RegExp,
  tagName: string,
): string[] => {
  const flags = openPattern.flags.includes("g")
    ? openPattern.flags
    : `${openPattern.flags}g`;
  const global = new RegExp(openPattern.source, flags);
  const openTag = new RegExp(`<${tagName}\\b`, "gi");
  const closeTag = new RegExp(`</${tagName}\\s*>`, "gi");
  const blocks: string[] = [];
  let match: RegExpExecArray | null;

  while ((match = global.exec(html))) {
    const start = match.index;
    let depth = 1;
    let cursor = start + match[0].length;

    while (cursor < html.length && depth > 0) {
      openTag.lastIndex = cursor;
      closeTag.lastIndex = cursor;
      const nextOpen = openTag.exec(html);
      const nextClose = closeTag.exec(html);
      if (!nextClose) {
        cursor = html.length;
        break;
      }
      if (nextOpen && nextOpen.index < nextClose.index) {
        depth += 1;
        cursor = nextOpen.index + nextOpen[0].length;
      } else {
        depth -= 1;
        cursor = nextClose.index + nextClose[0].length;
      }
    }

    blocks.push(html.slice(start, cursor));
    // Continue scanning after this block to avoid nested re-matches.
    global.lastIndex = cursor;
  }
  return blocks;
};

export const metaContent = (html: string, property: string): string | undefined => {
  const pattern = new RegExp(
    `<meta[^>]+(?:property|name)=["']${property}["'][^>]+content=["']([^"']+)["']|<meta[^>]+content=["']([^"']+)["'][^>]+(?:property|name)=["']${property}["']`,
    "i",
  );
  const match = pattern.exec(html);
  return match?.[1] ?? match?.[2];
};

export const imageFromTag = (imgTag: string): string => {
  const candidates = [
    attr(imgTag, "data-src"),
    attr(imgTag, "data-lazy-src"),
    attr(imgTag, "data-cfsrc"),
  ];
  const srcset = attr(imgTag, "srcset") ?? attr(imgTag, "data-srcset");
  if (srcset) {
    const best = srcset
      .split(",")
      .map((part) => {
        const [url, width] = part.trim().split(/\s+/);
        return {
          url: url ?? "",
          width: Number.parseInt((width ?? "0").replace(/\D/g, ""), 10) || 0,
        };
      })
      .sort((a, b) => b.width - a.width)[0];
    if (best?.url) candidates.push(best.url);
  }
  candidates.push(attr(imgTag, "src"));
  return candidates.find((value) => !!value?.trim())?.trim() ?? "";
};

export { base64Decode } from "./base64";
