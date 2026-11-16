import assert from "node:assert/strict";
import { hitRectFor } from "../src/drag.ts";

// top: exactly today's formula (x centred, 15 px margin, gap above)
assert.deepEqual(hitRectFor("top", 180, 40, 1200, 700, 10, 1), { x: 495, y: 0, w: 210, h: 80 });
// bottom: mirrored
assert.deepEqual(hitRectFor("bottom", 180, 40, 1200, 700, 10, 1), { x: 495, y: 635, w: 210, h: 80 });
// left: 40x120 tab hugging the left edge, centred vertically
assert.deepEqual(hitRectFor("left", 40, 120, 1200, 700, 10, 1), { x: 0, y: 275, w: 80, h: 150 });
// right: mirrored
assert.deepEqual(hitRectFor("right", 40, 120, 1200, 700, 10, 1), { x: 1120, y: 275, w: 80, h: 150 });
// scale k multiplies the margin
assert.deepEqual(hitRectFor("top", 360, 80, 1200, 700, 10, 2), { x: 390, y: 0, w: 420, h: 150 });
console.log("edge.check ok");
