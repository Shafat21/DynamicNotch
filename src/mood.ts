// Which face Pal shows. Pure: Task 9 feeds MoodInput from Claude Code sessions (spec §17.2).
export type PalMood = "calm" | "happy" | "listening" | "thinking" | "needs-you" | "done" | "napping" | "broke";

export type MoodInput = {
  bridgeOnline: boolean;
  connected: boolean;
  statuses: Array<"idle" | "working" | "needs-you" | "done" | "error">;
  promptPending: boolean;
  reminderDue: boolean; // a reminder banner waits for Snooze / Done
  listening: boolean;
  greeting: boolean;
  doneRecently: boolean;
  quietForMs: number;
};

/** Priority: broke > needs-you > listening > thinking > done > happy > calm > napping. */
export function moodFor(i: MoodInput): PalMood {
  if (!i.bridgeOnline || i.statuses.includes("error")) return "broke";
  if (i.promptPending || i.reminderDue || i.statuses.includes("needs-you")) return "needs-you";
  if (i.listening) return "listening";
  if (i.statuses.includes("working")) return "thinking";
  if (i.doneRecently) return "done";
  if (i.greeting) return "happy";
  if (!i.connected || i.quietForMs >= 30 * 60 * 1000) return "napping";
  return "calm";
}
