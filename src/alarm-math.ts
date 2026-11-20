// Alarm times (pure: scripts/alarm.check.ts). An alarm rings at "HH:MM" local time on the chosen weekdays
// (0 = Sunday … 6 = Saturday); no days = once, at the next time that clock time comes round.

export type Alarm = { id: number; time: string; label: string; days: number[]; on: boolean; snoozeUntil?: number | null; last?: number };

/** "07:30" → [7, 30], or null for anything else. */
export function parseTime(t: string): [number, number] | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(t);
  if (!m) return null;
  const h = Number(m[1]), min = Number(m[2]);
  return h < 24 && min < 60 ? [h, min] : null;
}

/** The next time `a` rings at or after `now` (ms), or null when it is off / its time is invalid. */
export function nextRing(a: Alarm, now: number): number | null {
  if (!a.on) return null;
  if (a.snoozeUntil && now - a.snoozeUntil < 60_000) return a.snoozeUntil; // pending, or due within its minute
  const hm = parseTime(a.time);
  if (!hm) return null;
  const d = new Date(now);
  for (let i = 0; i < 8; i++) {
    const t = new Date(d.getFullYear(), d.getMonth(), d.getDate() + i, hm[0], hm[1]).getTime(); // DST-safe: local date maths
    if (t < now - 59_999) continue; // its minute is already over
    if (a.days.length && !a.days.includes(new Date(t).getDay())) continue;
    if (a.last === t) continue; // already rang for this one
    return t;
  }
  return null;
}

/** Should `a` ring now? Its ring time has come, within the last minute (an alarm missed while the PC slept or the
 *  app was closed stays missed, like a phone's). Returns that ring time, so it is marked and never rings twice. */
export function dueNow(a: Alarm, now: number): number | null {
  const t = nextRing(a, now);
  return t !== null && t <= now && now - t < 60_000 ? t : null;
}

/** "In 7 h 20 min", "In 45 min", "In 2 days". */
export function untilText(t: number, now: number): string {
  const min = Math.max(0, Math.round((t - now) / 60_000));
  if (min < 1) return "Now";
  if (min < 60) return `In ${min} min`;
  const h = Math.floor(min / 60);
  if (h < 24) return min % 60 ? `In ${h} h ${min % 60} min` : `In ${h} h`;
  const d = Math.round(h / 24);
  return `In ${d} day${d === 1 ? "" : "s"}`;
}

const DAY = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/** "Once", "Every day", "Weekdays", "Weekends", or "Mon, Wed, Fri". */
export function daysText(days: number[]): string {
  const s = [...new Set(days)].sort((a, b) => a - b);
  if (!s.length) return "Once";
  if (s.length === 7) return "Every day";
  if (s.join() === "1,2,3,4,5") return "Weekdays";
  if (s.join() === "0,6") return "Weekends";
  return s.map((d) => DAY[d]).join(", ");
}
