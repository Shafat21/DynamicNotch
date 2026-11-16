// Claude Code plan usage (sticky `usage` event, bridge/usage.rs): pure helpers, checked by scripts/usage.check.ts.
export type UsageWindow = { usedPercentage: number; resetsAt: number }; // resetsAt: epoch s
export type Usage = { fiveHour?: UsageWindow; sevenDay?: UsageWindow; sessionCostUsd?: number; updatedAt: number; limitReached: boolean; limitMessage?: string };
export type Level = "ok" | "warn" | "crit";

const STALE_S = 5 * 60;

/** Ring / bar colour: accent, amber from 80 %, red from 95 % or when the limit was hit. */
export function level(pct: number, limitReached = false): Level {
  return limitReached || pct >= 95 ? "crit" : pct >= 80 ? "warn" : "ok";
}

/** A window that exists and hasn't reset yet; null otherwise (hidden). */
export const live = (w: UsageWindow | undefined, nowS: number): UsageWindow | null => (w && w.resetsAt > nowS ? w : null);

/** Local reset time: "3:40 PM" today, "Thu 9:00 AM" on another day. */
export function resetText(resetsAt: number, nowS: number): string {
  const d = new Date(resetsAt * 1000);
  const time = d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  return d.toDateString() === new Date(nowS * 1000).toDateString() ? time : `${d.toLocaleDateString("en-US", { weekday: "short" })} ${time}`;
}

/** "updated 12 m ago" once the numbers are older than 5 min, else null. */
export function staleText(updatedAt: number, nowS: number): string | null {
  const s = nowS - updatedAt;
  if (!updatedAt || s < STALE_S) return null;
  const m = Math.floor(s / 60);
  return m < 60 ? `updated ${m} m ago` : `updated ${Math.floor(m / 60)} h ago`;
}

/** Idle pill warning dot: the 5-hour window is at 80 %+ or the limit was hit. */
export const warnDot = (u: Usage | null, nowS: number): boolean => !!u && (u.limitReached || (live(u.fiveHour, nowS)?.usedPercentage ?? 0) >= 80);

/** "Limit reached · resets 3:40 PM" from the full window (else the 5-hour one); the message stays in the tooltip. */
export function limitText(u: Usage, nowS: number): string {
  const ws = [live(u.fiveHour, nowS), live(u.sevenDay, nowS)].filter((w): w is UsageWindow => !!w);
  const w = ws.find((x) => x.usedPercentage >= 100) ?? ws[0];
  return w ? `Limit reached · resets ${resetText(w.resetsAt, nowS)}` : "Limit reached";
}

export const pct =(w: UsageWindow) => `${Math.round(w.usedPercentage)}%`;
export const costText = (usd: number) => `≈ $${usd.toFixed(2)} this session (estimate)`;
