import type { Locale } from "./i18n";

const UNITS = ["B", "KB", "MB", "GB", "TB"];

export function formatBytes(n: number | null | undefined, digits = 1): string {
  if (n === null || n === undefined || !Number.isFinite(n)) return "—";
  if (n < 1024) return `${n} B`;
  let v = n;
  let i = 0;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 100 ? 0 : digits)} ${UNITS[i]}`;
}

export function formatDateTime(ms: number | null | undefined, locale: Locale): string {
  if (!ms) return "—";
  return new Intl.DateTimeFormat(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(ms));
}

export function formatTime(ms: number, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit", hour12: false }).format(
    new Date(ms),
  );
}

export function formatDay(ms: number, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, { weekday: "short", month: "short", day: "numeric" }).format(
    new Date(ms),
  );
}

export function formatRelative(ms: number | null | undefined, locale: Locale, now = Date.now()): string {
  if (!ms) return "—";
  const diff = ms - now;
  const abs = Math.abs(diff);
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  if (abs < 45_000) return rtf.format(Math.round(diff / 1000), "second");
  if (abs < 45 * 60_000) return rtf.format(Math.round(diff / 60_000), "minute");
  if (abs < 22 * 3_600_000) return rtf.format(Math.round(diff / 3_600_000), "hour");
  if (abs < 26 * 86_400_000) return rtf.format(Math.round(diff / 86_400_000), "day");
  return formatDateTime(ms, locale);
}

export function shortHash(h: string | null | undefined, n = 8): string {
  return h ? h.slice(0, n) : "—";
}

/** Last `n` path segments, keeping the separator style of the input. */
export function pathTail(p: string, n = 2): string {
  const sep = p.includes("\\") ? "\\" : "/";
  const parts = p.split(/[\\/]/).filter(Boolean);
  if (parts.length <= n) return p;
  return `…${sep}${parts.slice(-n).join(sep)}`;
}

export function baseName(p: string): string {
  const parts = p.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? p;
}

export function samePath(a: string, b: string): boolean {
  const norm = (s: string) => s.replace(/\//g, "\\").replace(/\\+$/, "").toLowerCase();
  return norm(a) === norm(b);
}
