import assert from "node:assert/strict";
import { weekOf } from "../src/compact/week.ts";

// Tuesday 17 Nov 2026 → Monday 16 .. Sunday 22
const w = weekOf(new Date(2026, 10, 17, 15, 30));
assert.equal(w.length, 7);
assert.deepEqual(w.map((d) => d.getDate()), [16, 17, 18, 19, 20, 21, 22]);
// Sunday belongs to the week that started the Monday before
assert.equal(weekOf(new Date(2026, 10, 22)).at(0)!.getDate(), 16);
// across a month end
assert.deepEqual(weekOf(new Date(2026, 11, 1)).map((d) => d.getDate()), [30, 1, 2, 3, 4, 5, 6]);
console.log("week.check ok");
