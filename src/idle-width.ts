/** Idle pill width: clock width, or wide enough for the title text; + the working spinner (12px + 10px gap). */
export function idlePillWidth(textW: number | null, collapsedW: number, working: boolean): number {
  const spin = working ? 22 : 0;
  const min = collapsedW + spin;
  if (textW === null) return min;
  // + 2x25px padding + 22px icon + 10px gap
  return Math.min(Math.max(420, min), Math.max(min, Math.ceil(textW) + 82 + spin));
}
