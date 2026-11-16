// The compact notch's cards, in order, and paging between them (pure: checked by scripts/pager.check.ts).
export type CardId = "music" | "pomodoro" | "shelf" | "todo" | "widgets";
export const CARDS: CardId[] = ["music", "pomodoro", "shelf", "todo", "widgets"];
export const CARD_NAMES: Record<CardId, string> = { music: "Music", pomodoro: "Pomodoro", shelf: "Shelf", todo: "Todo", widgets: "Widgets" };

/** Next index in `dir`, wrapping around `n` cards. */
export function step(i: number, n: number, dir: 1 | -1): number {
  return (((i + dir) % n) + n) % n;
}
