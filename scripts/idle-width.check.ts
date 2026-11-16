import assert from "node:assert/strict";
import { idlePillWidth } from "../src/idle-width.ts";

assert.equal(idlePillWidth(null, 180, false), 180);
assert.equal(idlePillWidth(null, 180, true), 202);
assert.equal(idlePillWidth(70, 180, false), 180);
assert.equal(idlePillWidth(70, 180, true), 202);
assert.equal(idlePillWidth(300, 180, false), 382);
assert.equal(idlePillWidth(300, 180, true), 404); // spinner counted
assert.equal(idlePillWidth(1000, 180, false), 420);
assert.equal(idlePillWidth(1000, 180, true), 420);
assert.equal(idlePillWidth(null, 180, true, 72), 274); // spinner + call badge
assert.equal(idlePillWidth(300, 180, false, 72), 420); // capped like any title
console.log("idle-width ok");
