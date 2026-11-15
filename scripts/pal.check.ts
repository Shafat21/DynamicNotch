import assert from "node:assert/strict";
import { FACES, lerpPath } from "../src/pal-shapes.ts";
import { moodFor, type MoodInput } from "../src/mood.ts";

const letters = (d: string) => d.replace(/[^A-Za-z]/g, "");
for (const part of ["eyeL", "eyeR", "mouth"] as const) {
  const ref = letters(FACES.calm[part]);
  for (const [m, f] of Object.entries(FACES)) assert.equal(letters(f[part]), ref, `${m}.${part} structure differs`);
}
assert.equal(lerpPath(FACES.calm.mouth, FACES.done.mouth, 0), FACES.calm.mouth);
assert.equal(lerpPath(FACES.calm.mouth, FACES.done.mouth, 1), FACES.done.mouth);
assert.throws(() => lerpPath("M0 0L1 1", "M0 0C1 1 2 2 3 3", 0.5));

const base: MoodInput = { bridgeOnline: true, connected: true, statuses: [], promptPending: false, reminderDue: false, listening: false, greeting: false, doneRecently: false, quietForMs: 0 };
assert.equal(moodFor(base), "calm");
assert.equal(moodFor({ ...base, connected: false }), "napping");
assert.equal(moodFor({ ...base, quietForMs: 30 * 60 * 1000 }), "napping");
assert.equal(moodFor({ ...base, bridgeOnline: false }), "broke");
assert.equal(moodFor({ ...base, statuses: ["working", "error"] }), "broke");
assert.equal(moodFor({ ...base, statuses: ["working"], promptPending: true }), "needs-you");
assert.equal(moodFor({ ...base, statuses: ["working"], listening: true }), "listening");
assert.equal(moodFor({ ...base, reminderDue: true, listening: true, greeting: true }), "needs-you"); // a due reminder beats listening/happy
assert.equal(moodFor({ ...base, reminderDue: true, connected: false }), "needs-you"); // even without Claude Code
assert.equal(moodFor({ ...base, reminderDue: true, bridgeOnline: false }), "broke"); // same rank as a prompt
assert.equal(moodFor({ ...base, statuses: ["working", "done"] }), "thinking");
assert.equal(moodFor({ ...base, doneRecently: true, greeting: true }), "done");
assert.equal(moodFor({ ...base, greeting: true }), "happy");
assert.equal(moodFor({ ...base, greeting: true, connected: false }), "happy"); // greeting beats napping
console.log("pal check ok");
