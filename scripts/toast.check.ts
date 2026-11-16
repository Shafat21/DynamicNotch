import assert from "node:assert/strict";
import { freshOnly, takesOver } from "../src/toast-queue.ts";

// a toast takes the island only from the idle pill or a media pop
assert.equal(takesOver("idle"), true);
assert.equal(takesOver("notify"), true);
// it waits while the island is open (typing, dropping a file), or something more important / another toast shows
for (const s of ["hover", "prompt", "reminder", "dictation", "toast"]) assert.equal(takesOver(s), false, s);
// toasts that waited a minute behind a prompt are dropped, not replayed
const q = [{ at: 0, n: 1 }, { at: 50_000, n: 2 }, { at: 100_000, n: 3 }];
assert.deepEqual(freshOnly(q, 105_000).map((t) => t.n), [2, 3]);
console.log("toast.check ok");
