process.env.TZ = "UTC"; // reset times are local: pin the zone before any Date is made
import assert from "node:assert/strict";
import { costText, level, limitText, live, resetText, staleText, warnDot, type Usage } from "../src/usage.ts";

const now = Date.UTC(2026, 10, 16, 12, 0) / 1000; // Mon 16 Nov 2026 12:00 UTC

assert.equal(level(0), "ok");
assert.equal(level(79.9), "ok");
assert.equal(level(80), "warn");
assert.equal(level(94.9), "warn");
assert.equal(level(95), "crit");
assert.equal(level(10, true), "crit");

assert.equal(resetText(now + 3 * 3600 + 40 * 60, now), "3:40 PM"); // same day: time only
assert.equal(resetText(Date.UTC(2026, 10, 19, 9, 0) / 1000, now), "Thu 9:00 AM");
assert.equal(resetText(Date.UTC(2026, 10, 17, 0, 30) / 1000, now), "Tue 12:30 AM"); // past midnight is another day

assert.equal(staleText(0, now), null); // never updated
assert.equal(staleText(now - 299, now), null);
assert.equal(staleText(now - 300, now), "updated 5 m ago");
assert.equal(staleText(now - 12 * 60 - 30, now), "updated 12 m ago");
assert.equal(staleText(now - 2 * 3600, now), "updated 2 h ago");

const w = (p: number, inS = 3600) => ({ usedPercentage: p, resetsAt: now + inS });
assert.equal(live(undefined, now), null);
assert.equal(live(w(50, -1), now), null); // reset already passed
assert.deepEqual(live(w(50), now), w(50));

const u = (o: Partial<Usage>): Usage => ({ updatedAt: now, limitReached: false, ...o });
assert.equal(warnDot(null, now), false);
assert.equal(warnDot(u({ fiveHour: w(79) }), now), false);
assert.equal(warnDot(u({ fiveHour: w(80) }), now), true);
assert.equal(warnDot(u({ fiveHour: w(99, -10) }), now), false); // stale window ignored
assert.equal(warnDot(u({ sevenDay: w(99) }), now), false); // only the 5-hour window warns
assert.equal(warnDot(u({ limitReached: true }), now), true);

assert.equal(limitText(u({ limitReached: true }), now), "Limit reached");
assert.equal(limitText(u({ limitReached: true, fiveHour: w(60, 1800), sevenDay: w(100, 6 * 86400) }), now), "Limit reached · resets Sun 12:00 PM");
assert.equal(limitText(u({ limitReached: true, fiveHour: w(100, 1800) }), now), "Limit reached · resets 12:30 PM");

assert.equal(costText(0.8412), "≈ $0.84 this session (estimate)");
console.log("usage check ok");
