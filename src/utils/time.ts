/**
 * Parse a timestamp string from SQLite into a Date.
 * SQLite's `datetime('now')` returns UTC without a trailing "Z", which
 * `new Date()` would otherwise interpret as local time. Append "Z" so the
 * string is parsed as UTC.
 */
export function parseSqliteUtc(isoDate: string): Date {
  return new Date(isoDate.endsWith("Z") ? isoDate : isoDate + "Z");
}

/**
 * Format an ISO date string as a human-readable relative time.
 * Handles SQLite UTC strings without the trailing "Z".
 */
export function relativeTime(isoDate: string): string {
  const now = Date.now();
  const then = parseSqliteUtc(isoDate).getTime();
  const diffMs = now - then;
  const minutes = Math.floor(diffMs / 60000);
  if (minutes < 1) return "agora";
  if (minutes < 60) return `há ${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `há ${hours} h`;
  const days = Math.floor(hours / 24);
  if (days === 1) return "ontem";
  if (days < 30) return `há ${days} dias`;
  const months = Math.floor(days / 30);
  return months === 1 ? "há 1 mês" : `há ${months} meses`;
}
