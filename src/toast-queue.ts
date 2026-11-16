// When a queued live toast may show (pure: checked by scripts/toast.check.ts). main.ts owns the queue.

/** A new toast takes the island only from the idle pill or a media pop. An open island (typing, dropping a file),
 *  a prompt, reminder, dictation or another toast keeps it queued until settle() / the island going idle. */
export const takesOver = (state: string): boolean => state === "idle" || state === "notify";

/** Drops toasts that waited longer than `maxAge` ms (behind a long prompt, say): old news is not replayed. */
export const freshOnly = <T extends { at: number }>(queue: T[], now: number, maxAge = 60_000): T[] => queue.filter((t) => now - t.at <= maxAge);
