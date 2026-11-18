// Month grid maths for the Calendar tab (pure: scripts/calendar-grid.check.ts). Months are 0-11 like Date.

export type Cell = { y: number; m: number; d: number; inMonth: boolean };
type Span = { start: number; end: number; allDay: boolean };

/** The six weeks (42 days) that show month `m` of `y`, weeks starting on `weekStart` (0 = Sunday, 1 = Monday). */
export function monthCells(y: number, m: number, weekStart = 1): Cell[] {
  const first = new Date(y, m, 1);
  const lead = (first.getDay() - weekStart + 7) % 7;
  return Array.from({ length: 42 }, (_, i) => {
    const d = new Date(y, m, 1 - lead + i); // Date rolls over months and years
    return { y: d.getFullYear(), m: d.getMonth(), d: d.getDate(), inMonth: d.getMonth() === m };
  });
}

/** [start, end) of a local day in ms (DST-safe: the next midnight, not +24 h). */
export function dayBounds(y: number, m: number, d: number): [number, number] {
  return [new Date(y, m, d).getTime(), new Date(y, m, d + 1).getTime()];
}

/** Events touching the local day: timed ones that overlap it and all-day ones that cover it, all-day first, then by start. */
export function eventsOn<T extends Span>(events: T[], y: number, m: number, d: number): T[] {
  const [t0, t1] = dayBounds(y, m, d);
  return events
    .filter((e) => (e.allDay ? e.start < t1 && e.end > t0 : e.start < t1 && e.end > t0 || (e.start === e.end && e.start >= t0 && e.start < t1)))
    .sort((a, b) => Number(b.allDay) - Number(a.allDay) || a.start - b.start);
}

/** Month `m` of `y` shifted by `by` months, normalised. */
export function shiftMonth(y: number, m: number, by: number): { y: number; m: number } {
  const d = new Date(y, m + by, 1);
  return { y: d.getFullYear(), m: d.getMonth() };
}
