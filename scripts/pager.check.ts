import assert from "node:assert/strict";
import { CARDS, refreshes, step } from "../src/compact/pager.ts";

assert.equal(step(4, 5, 1), 0); // wraps forward
assert.equal(step(0, 5, -1), 4); // wraps back
assert.equal(step(2, 5, 1), 3);
assert.deepEqual(CARDS, ["music", "pomodoro", "shelf", "todo", "widgets"]);
// a data change re-renders only the cards that show it (a rebuild drops typed text in the Todo input)
assert.equal(refreshes("todo", ["music"]), false);
assert.equal(refreshes("music", ["music"]), true);
assert.equal(refreshes("widgets", ["todo", "widgets"]), true);
assert.equal(refreshes("shelf", []), true); // no list: whatever is showing
console.log("pager.check ok");
