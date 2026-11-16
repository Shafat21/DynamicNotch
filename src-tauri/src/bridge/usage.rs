//! Account-wide plan usage from Claude Code's statusLine JSON (`rate_limits`, `cost`).
//!
//! Event contract — sticky `usage` (camelCase; optional keys are omitted when unknown):
//! `{fiveHour?: {usedPercentage: 0..100, resetsAt: epoch s}, sevenDay?: {…same}, sessionCostUsd?: number,
//!   updatedAt: epoch s of the last statusLine that carried rate_limits, limitReached: bool, limitMessage?: string}`
//! Not published until the first statusLine arrives (e.g. API-key users never get one: hide the widget).

use super::truncate;
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub used_percentage: f64,
    pub resets_at: i64,
}

#[derive(Serialize, Clone, PartialEq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<Window>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seven_day: Option<Window>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_cost_usd: Option<f64>,
    pub updated_at: i64,
    pub limit_reached: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_message: Option<String>,
    /// When the reached limit lifts (the limiting window's `resets_at` at the time), if known.
    #[serde(skip)]
    limit_until: Option<i64>,
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|f| f.is_finite())
}

/// A window from `{used_percentage, resets_at}`: % must be ≥ 0 (above 100 is capped), reset must not be past.
pub fn window(v: &Value, now: i64) -> Option<Window> {
    let pct = num(&v["used_percentage"]).filter(|p| *p >= 0.0)?.min(100.0);
    let resets_at = num(&v["resets_at"])? as i64;
    (resets_at >= now).then_some(Window { used_percentage: pct, resets_at })
}

/// The window whose reset lifts a reached limit: the first one at 100 %, else five_hour, else seven_day.
fn limit_window(five_hour: Option<Window>, seven_day: Option<Window>) -> Option<Window> {
    [five_hour, seven_day].into_iter().flatten().find(|w| w.used_percentage >= 100.0).or(five_hour).or(seven_day)
}

impl Usage {
    /// Applies one statusLine payload. Returns true if the published value changed.
    pub fn apply_statusline(&mut self, ev: &Value, now: i64) -> bool {
        let before = self.clone();
        if let Some(cost) = num(&ev["cost"]["total_cost_usd"]).filter(|c| *c >= 0.0) {
            self.session_cost_usd = Some(cost);
        }
        // no rate_limits object (API key, or before the session's first response): keep the last account-wide copy
        if let Some(rl) = ev.get("rate_limits").filter(|v| v.is_object()) {
            self.five_hour = window(&rl["five_hour"], now);
            self.seven_day = window(&rl["seven_day"], now);
            self.updated_at = now;
            let below = |w: Option<Window>| w.is_none_or(|w| w.used_percentage < 100.0);
            if self.limit_reached && self.five_hour.is_some() && below(self.five_hour) && below(self.seven_day) {
                self.clear_limit();
            }
        }
        self.expire(now);
        *self != before
    }

    /// StopFailure: `error == "rate_limit"` marks the limit as reached. Returns true if changed.
    pub fn apply_stop_failure(&mut self, ev: &Value) -> bool {
        if ev["error"] != "rate_limit" {
            return false;
        }
        let before = self.clone();
        let text = ["last_assistant_message", "error_details", "message"]
            .iter()
            .find_map(|k| ev[*k].as_str().filter(|s| !s.trim().is_empty()))
            .unwrap_or("Usage limit reached");
        self.limit_reached = true;
        self.limit_message = Some(truncate(text, 200));
        self.limit_until = limit_window(self.five_hour, self.seven_day).map(|w| w.resets_at);
        *self != before
    }

    /// Drops windows whose reset passed and lifts a limit past its reset. Returns true if changed.
    pub fn expire(&mut self, now: i64) -> bool {
        let before = self.clone();
        self.five_hour = self.five_hour.filter(|w| w.resets_at >= now);
        self.seven_day = self.seven_day.filter(|w| w.resets_at >= now);
        if self.limit_until.is_some_and(|t| t < now) {
            self.clear_limit();
        }
        *self != before
    }

    fn clear_limit(&mut self) {
        self.limit_reached = false;
        self.limit_message = None;
        self.limit_until = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_000_000;

    fn sl(rl: Value) -> Value {
        json!({"hook_event_name": "StatusLine", "session_id": "s1", "cost": {"total_cost_usd": 0.84}, "rate_limits": rl})
    }

    #[test]
    fn window_parses_int_and_float_and_rejects_bad_values() {
        assert_eq!(window(&json!({"used_percentage": 23.5, "resets_at": NOW + 10}), NOW), Some(Window { used_percentage: 23.5, resets_at: NOW + 10 }));
        assert_eq!(window(&json!({"used_percentage": 62, "resets_at": (NOW + 5) as f64}), NOW), Some(Window { used_percentage: 62.0, resets_at: NOW + 5 }));
        assert_eq!(window(&json!({"used_percentage": 140, "resets_at": NOW}), NOW).unwrap().used_percentage, 100.0);
        assert_eq!(window(&json!({"used_percentage": -1, "resets_at": NOW + 1}), NOW), None);
        assert_eq!(window(&json!({"used_percentage": "50", "resets_at": NOW + 1}), NOW), None);
        assert_eq!(window(&json!({"used_percentage": 50}), NOW), None);
        assert_eq!(window(&json!({"used_percentage": 50, "resets_at": NOW - 1}), NOW), None); // stale
        assert_eq!(window(&Value::Null, NOW), None);
    }

    #[test]
    fn statusline_sets_windows_cost_and_reports_changes() {
        let mut u = Usage::default();
        let ev = sl(json!({"five_hour": {"used_percentage": 23.5, "resets_at": NOW + 100}}));
        assert!(u.apply_statusline(&ev, NOW));
        assert_eq!(u.five_hour.unwrap().used_percentage, 23.5);
        assert_eq!((u.seven_day, u.session_cost_usd, u.updated_at), (None, Some(0.84), NOW));
        assert!(!u.apply_statusline(&ev, NOW)); // same second, same values
        assert_eq!(
            serde_json::to_value(&u).unwrap(),
            json!({"fiveHour": {"usedPercentage": 23.5, "resetsAt": NOW + 100}, "sessionCostUsd": 0.84, "updatedAt": NOW, "limitReached": false})
        );
    }

    #[test]
    fn missing_rate_limits_keeps_last_copy_and_missing_fields_are_ignored() {
        let mut u = Usage::default();
        u.apply_statusline(&sl(json!({"seven_day": {"used_percentage": 41.2, "resets_at": NOW + 999}})), NOW);
        assert!(!u.apply_statusline(&json!({"hook_event_name": "StatusLine", "cost": {"total_cost_usd": 0.84}}), NOW + 1));
        assert_eq!(u.seven_day.unwrap().used_percentage, 41.2);
        assert!(!u.apply_statusline(&json!({"cost": {"total_cost_usd": -3}, "rate_limits": null}), NOW + 1));
        assert!(u.apply_statusline(&sl(json!({})), NOW + 2)); // rate_limits present but empty: windows gone
        assert_eq!((u.five_hour, u.seven_day), (None, None));
    }

    #[test]
    fn expire_drops_past_windows() {
        let mut u = Usage::default();
        u.apply_statusline(&sl(json!({"five_hour": {"used_percentage": 5, "resets_at": NOW + 10}, "seven_day": {"used_percentage": 9, "resets_at": NOW + 99}})), NOW);
        assert!(!u.expire(NOW + 10));
        assert!(u.expire(NOW + 11));
        assert_eq!((u.five_hour, u.seven_day.map(|w| w.used_percentage)), (None, Some(9.0)));
    }

    #[test]
    fn rate_limit_failure_sets_limit_and_later_statusline_or_reset_clears_it() {
        let mut u = Usage::default();
        assert!(!u.apply_stop_failure(&json!({"error": "server_error", "last_assistant_message": "x"})));
        u.apply_statusline(&sl(json!({"five_hour": {"used_percentage": 100, "resets_at": NOW + 50}})), NOW);
        let long = "🙂".repeat(300);
        assert!(u.apply_stop_failure(&json!({"error": "rate_limit", "last_assistant_message": long})));
        assert!(u.limit_reached);
        assert_eq!(u.limit_message.as_ref().unwrap().chars().count(), 201);
        assert!(!u.apply_statusline(&sl(json!({"five_hour": {"used_percentage": 100, "resets_at": NOW + 50}})), NOW));
        assert!(u.limit_reached); // still at 100
        assert!(u.apply_statusline(&sl(json!({"five_hour": {"used_percentage": 3, "resets_at": NOW + 18000}})), NOW + 60));
        assert!(!u.limit_reached && u.limit_message.is_none());

        u.apply_stop_failure(&json!({"error": "rate_limit"}));
        assert_eq!(u.limit_message.as_deref(), Some("Usage limit reached"));
        assert!(!u.expire(NOW + 18000));
        assert!(u.expire(NOW + 18001)); // past the 5-hour reset: lifted
        assert!(!u.limit_reached);
    }

    #[test]
    fn weekly_limit_is_not_cleared_by_low_five_hour() {
        let mut u = Usage::default();
        u.apply_statusline(&sl(json!({"seven_day": {"used_percentage": 100, "resets_at": NOW + 500}})), NOW);
        u.apply_stop_failure(&json!({"error": "rate_limit", "last_assistant_message": "Weekly limit reached"}));
        u.apply_statusline(&sl(json!({"five_hour": {"used_percentage": 10, "resets_at": NOW + 100}, "seven_day": {"used_percentage": 100, "resets_at": NOW + 500}})), NOW);
        assert!(u.limit_reached);
        // failure arriving while both windows are known: the full weekly window decides when it lifts
        u.apply_stop_failure(&json!({"error": "rate_limit"}));
        u.expire(NOW + 101);
        assert!(u.limit_reached);
        assert!(u.expire(NOW + 501));
        assert!(!u.limit_reached);
    }

    #[test]
    fn limit_lift_prefers_full_window_then_five_hour_then_seven_day() {
        let w = |p: f64, r: i64| Some(Window { used_percentage: p, resets_at: r });
        assert_eq!(limit_window(w(100.0, 1), w(100.0, 2)), w(100.0, 1));
        assert_eq!(limit_window(w(50.0, 1), w(100.0, 2)), w(100.0, 2));
        assert_eq!(limit_window(w(50.0, 1), w(60.0, 2)), w(50.0, 1));
        assert_eq!(limit_window(None, w(60.0, 2)), w(60.0, 2));
        assert_eq!(limit_window(None, None), None);
    }
}
