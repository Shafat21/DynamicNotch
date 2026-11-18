// Progress tab maths (pure: scripts/progress.check.ts): how far through today / this week / month / year, and
// whole days until a countdown date. Local time; weeks start on Monday like the Calendar grid.

export type Span = { label: string; frac: number; left: string };

const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? "" : "s"}`;

/** Fraction of [start, end) that `now` has covered, clamped to 0..1. */
const frac = (now: number, start: number, end: number) => Math.min(1, Math.max(0, (now - start) / (end - start)));

export function spans(now = new Date()): Span[] {
  const y = now.getFullYear(), m = now.getMonth(), d = now.getDate();
  const t = now.getTime();
  const day0 = new Date(y, m, d).getTime(), day1 = new Date(y, m, d + 1).getTime();
  const back = (now.getDay() + 6) % 7; // days since Monday
  const week0 = new Date(y, m, d - back).getTime(), week1 = new Date(y, m, d - back + 7).getTime();
  const month0 = new Date(y, m, 1).getTime(), month1 = new Date(y, m + 1, 1).getTime();
  const year0 = new Date(y, 0, 1).getTime(), year1 = new Date(y + 1, 0, 1).getTime();
  const hoursLeft = Math.ceil((day1 - t) / 3_600_000);
  const daysIn = (a: number, b: number) => Math.round((b - a) / 86_400_000); // DST days are 23/25 h: round, not floor
  const daysLeft = (end: number) => daysIn(day1, end); // whole days after today
  return [
    { label: "Today", frac: frac(t, day0, day1), left: `${plural(hoursLeft, "hour")} left` },
    { label: "This week", frac: frac(t, week0, week1), left: daysLeft(week1) ? `${plural(daysLeft(week1), "day")} left` : "Last day" },
    { label: now.toLocaleDateString("en-US", { month: "long" }), frac: frac(t, month0, month1), left: daysLeft(month1) ? `${plural(daysLeft(month1), "day")} left` : "Last day" },
    { label: String(y), frac: frac(t, year0, year1), left: `${plural(daysLeft(year1), "day")} left` },
  ];
}

/** Whole days from today to `date` ("YYYY-MM-DD", local): 0 = today, negative = passed. NaN for a bad date. */
export function daysUntil(date: string, now = new Date()): number {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  if (!m) return NaN;
  const target = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  if (target.getMonth() !== Number(m[2]) - 1) return NaN; // 2026-02-31 and the like
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  return Math.round((target.getTime() - today.getTime()) / 86_400_000);
}

/** "Today", "Tomorrow", "In 12 days", "3 days ago". */
export function untilLabel(n: number): string {
  if (n === 0) return "Today";
  if (n === 1) return "Tomorrow";
  if (n === -1) return "Yesterday";
  return n > 0 ? `In ${plural(n, "day")}` : `${plural(-n, "day")} ago`;
}
