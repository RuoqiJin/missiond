/**
 * 时间处理单一来源 — 解析 + 格式化统一收口。
 *
 * 根因：SQLite datetime('now') 存 UTC 但无 'Z' 后缀，
 * JS new Date() 会误认为本地时间。此模块统一处理。
 */

/** 宽容解析：SQLite UTC 无 Z → 补 Z，已有时区标记的原样解析 */
export function parseUtcDate(dateStr: string | Date | undefined | null): Date | null {
  if (!dateStr) return null;
  if (dateStr instanceof Date) return dateStr;

  let str = dateStr.trim();

  // YYYY-MM-DD HH:MM:SS (SQLite default, no timezone)
  if (/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(str)) {
    str = str.replace(' ', 'T') + 'Z';
  } else if (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}$/.test(str)) {
    // Has T but no Z
    str += 'Z';
  }

  const date = new Date(str);
  return isNaN(date.getTime()) ? null : date;
}

/** 获取 UTC 毫秒时间戳，null-safe */
export function utcMs(dateStr: string | Date | undefined | null): number {
  return parseUtcDate(dateStr)?.getTime() ?? 0;
}

/** 格式化为北京时间，可自定义 options */
export function formatBeijing(
  dateStr: string | Date | undefined | null,
  options: Intl.DateTimeFormatOptions = {
    year: 'numeric', month: '2-digit', day: '2-digit',
    hour: '2-digit', minute: '2-digit', second: '2-digit',
    hour12: false,
  },
): string {
  const date = parseUtcDate(dateStr);
  if (!date) return '-';
  return new Intl.DateTimeFormat('zh-CN', { ...options, timeZone: 'Asia/Shanghai' }).format(date);
}

/** 仅时分秒 (HH:MM:SS) 北京时间 */
export function formatBeijingTime(dateStr: string | Date | undefined | null): string {
  return formatBeijing(dateStr, { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
}

// ── Daily View helpers ────────────────────────────────────

/** Get UTC timestamp range for a Beijing timezone day (YYYY-MM-DD → 00:00~24:00 Asia/Shanghai) */
export function beijingDayRange(dateStr: string): { start: number; end: number } {
  const [y, m, d] = dateStr.split('-').map(Number);
  // Beijing 00:00:00 = UTC minus 8h
  const start = Date.UTC(y, m - 1, d) - 8 * 3600_000;
  return { start, end: start + 86400_000 };
}

/** UTC ms → YYYY-MM-DD in Beijing timezone */
export function toBeijingDate(ms: number): string {
  return new Intl.DateTimeFormat('sv-SE', { timeZone: 'Asia/Shanghai' }).format(new Date(ms));
}

/** Today's YYYY-MM-DD in Beijing timezone */
export function todayBeijing(): string {
  return toBeijingDate(Date.now());
}
