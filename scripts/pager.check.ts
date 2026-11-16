import assert from "node:assert/strict";
import { CARDS, step } from "../src/compact/pager.ts";

assert.equal(step(4, 5, 1), 0); // wraps forward
assert.equal(step(0, 5, -1), 4); // wraps back
assert.equal(step(2, 5, 1), 3);
assert.deepEqual(CARDS, ["music", "pomodoro", "shelf", "todo", "widgets"]);
console.log("pager.check ok");
