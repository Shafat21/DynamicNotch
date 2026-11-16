// The Monday-to-Sunday week around a date (local time), for the compact Widgets card. Pure: scripts/week.check.ts.

/** Monday-based dates of the week containing `d`. */
export function weekOf(d: Date): Date[] {
  const monday = new Date(d.getFullYear(), d.getMonth(), d.getDate() - ((d.getDay() + 6) % 7));
  return Array.from({ length: 7 }, (_, i) => new Date(monday.getFullYear(), monday.getMonth(), monday.getDate() + i));
}
