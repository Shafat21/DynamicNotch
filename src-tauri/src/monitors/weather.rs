//! Open-Meteo weather (no API key). Fetch on start, every 30 min, and on `refresh()`. Event: `weather`.

use super::http;
use crate::{publish, AppState};
use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{thread, time::Duration, time::Instant};
use tauri::{AppHandle, Manager};

const INTERVAL: Duration = Duration::from_secs(30 * 60);
static REFRESH: AtomicBool = AtomicBool::new(true);

#[derive(Serialize, Debug)]
pub struct Hour {
    time: String,
    temp: String,
    icon: &'static str,
    /// chance of rain, %
    rain: Option<u8>,
}

/// Extras (feels, high/low, humidity, wind, rain) are `None` when Open-Meteo leaves them out.
#[derive(Serialize, Debug)]
pub struct Weather {
    city: String,
    temp: String,
    desc: &'static str,
    icon: &'static str,
    feels: Option<String>,
    high: Option<String>,
    low: Option<String>,
    /// relative humidity, %
    humidity: Option<u8>,
    /// km/h
    wind: Option<u32>,
    /// the highest chance of rain in the hours shown, %
    rain: Option<u8>,
    hourly: Vec<Hour>,
}

const HOURS: usize = 8;

pub fn refresh() {
    REFRESH.store(true, Ordering::Relaxed);
}

pub fn spawn(app: AppHandle) {
    thread::spawn(move || {
        let mut last = Instant::now();
        loop {
            if REFRESH.swap(false, Ordering::Relaxed) || last.elapsed() >= INTERVAL {
                last = Instant::now();
                let s = app.state::<AppState>().db.lock().unwrap().settings();
                match s.map_err(|e| e.to_string()).and_then(|s| fetch(&s.location, s.lat, s.lon)) {
                    Ok(w) => publish(&app, "weather", w),
                    Err(e) => eprintln!("weather update failed: {e}"),
                }
            }
            thread::sleep(Duration::from_secs(1));
        }
    });
}

/// City name -> ("City, Country", lat, lon).
pub fn geocode(city: &str) -> Result<(String, f64, f64), String> {
    let res: Value = http()
        .get("https://geocoding-api.open-meteo.com/v1/search")
        .query("name", city)
        .query("count", "1")
        .query("language", "en")
        .query("format", "json")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())?;
    let r = res["results"].get(0).ok_or_else(|| format!("Could not find location: {city}"))?;
    let name = match r["country"].as_str() {
        Some(c) => format!("{}, {c}", r["name"].as_str().unwrap_or(city)),
        None => r["name"].as_str().unwrap_or(city).to_string(),
    };
    Ok((name, r["latitude"].as_f64().unwrap_or(0.0), r["longitude"].as_f64().unwrap_or(0.0)))
}

fn fetch(city: &str, lat: f64, lon: f64) -> Result<Weather, String> {
    let res: Value = http()
        .get("https://api.open-meteo.com/v1/forecast")
        .query("latitude", lat.to_string())
        .query("longitude", lon.to_string())
        .query("current_weather", "true")
        .query("current", "apparent_temperature,relative_humidity_2m")
        .query("hourly", "temperature_2m,weathercode,precipitation_probability")
        .query("daily", "temperature_2m_max,temperature_2m_min")
        .query("forecast_days", "2")
        .query("timezone", "auto")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())?;
    parse(city, &res)
}

/// Pure parsing, split out so it can be unit tested without the network.
fn parse(city: &str, res: &Value) -> Result<Weather, String> {
    let cur = &res["current_weather"];
    let code = cur["weathercode"].as_i64().ok_or("missing current_weather")?;
    let times = res["hourly"]["time"].as_array().ok_or("missing hourly")?;
    // With timezone=auto both current and hourly times are location-local "YYYY-MM-DDTHH:MM".
    let hour_key = format!("{}:00", cur["time"].as_str().unwrap_or("").get(..13).unwrap_or(""));
    let start = times.iter().position(|t| t.as_str() == Some(hour_key.as_str())).unwrap_or(0);

    let deg = |v: &Value| v.as_f64().map(|t| format!("{}°", t.round() as i64));
    let pct = |v: &Value| v.as_f64().map(|p| p.round().clamp(0.0, 100.0) as u8);
    let hourly: Vec<Hour> = (start + 1..start + 1 + HOURS)
        .filter(|&i| i < times.len())
        .map(|i| Hour {
            time: hour_label(times[i].as_str().unwrap_or("")),
            temp: format!("{}°", res["hourly"]["temperature_2m"][i].as_f64().unwrap_or(0.0).round() as i64),
            icon: icon(res["hourly"]["weathercode"][i].as_i64().unwrap_or(-1)),
            rain: pct(&res["hourly"]["precipitation_probability"][i]),
        })
        .collect();

    Ok(Weather {
        city: city.to_string(),
        temp: format!("{}°", cur["temperature"].as_f64().unwrap_or(0.0).round() as i64),
        desc: desc(code),
        icon: icon(code),
        feels: deg(&res["current"]["apparent_temperature"]),
        high: deg(&res["daily"]["temperature_2m_max"][0]),
        low: deg(&res["daily"]["temperature_2m_min"][0]),
        humidity: pct(&res["current"]["relative_humidity_2m"]),
        wind: cur["windspeed"].as_f64().map(|w| w.round().max(0.0) as u32),
        rain: hourly.iter().filter_map(|h| h.rain).max(),
        hourly,
    })
}

/// "2026-11-15T15:00" -> "3PM"
fn hour_label(t: &str) -> String {
    let h: u32 = t.get(11..13).and_then(|h| h.parse().ok()).unwrap_or(0);
    let h12 = if h.is_multiple_of(12) { 12 } else { h % 12 };
    format!("{h12}{}", if h < 12 { "AM" } else { "PM" })
}

/// Icon keys understood by the frontend (see src/icons.ts).
fn icon(code: i64) -> &'static str {
    match code {
        0 => "sunny",
        1 | 2 => "partly",
        45 | 48 => "fog",
        51 | 53 | 55 | 80 | 81 | 82 => "rainy",
        61 | 63 | 65 => "pouring",
        71 | 73 | 75 => "snowy",
        95 | 96 | 99 => "lightning",
        _ => "cloudy",
    }
}

fn desc(code: i64) -> &'static str {
    match code {
        0 => "Clear Sky",
        1 => "Mainly Clear",
        2 => "Partly Cloudy",
        3 => "Overcast",
        45 => "Fog",
        48 => "Depositing Rime Fog",
        51 => "Light Drizzle",
        53 => "Moderate Drizzle",
        55 => "Dense Drizzle",
        61 => "Slight Rain",
        63 => "Moderate Rain",
        65 => "Heavy Rain",
        71 => "Slight Snow",
        73 => "Moderate Snow",
        75 => "Heavy Snow",
        80 => "Slight Rain Showers",
        81 => "Moderate Rain Showers",
        82 => "Violent Rain Showers",
        95 => "Thunderstorm",
        96 => "Thunderstorm with Slight Hail",
        99 => "Thunderstorm with Heavy Hail",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_current_and_next_hours() {
        let times: Vec<String> = (0..24).map(|h| format!("2026-11-15T{h:02}:00")).collect();
        let res = json!({
            "current_weather": { "time": "2026-11-15T13:45", "temperature": 21.6, "weathercode": 2 },
            "hourly": { "time": times, "temperature_2m": vec![10.4; 24], "weathercode": vec![61; 24] }
        });
        let w = parse("Dhaka, Bangladesh", &res).unwrap();
        assert_eq!(w.temp, "22°");
        assert_eq!(w.desc, "Partly Cloudy");
        assert_eq!(w.hourly.len(), 8);
        assert_eq!(w.hourly[0].time, "2PM");
        assert_eq!(w.hourly[0].icon, "pouring");
    }

    #[test]
    fn parses_feels_high_low_humidity_wind_and_rain_chance() {
        let times: Vec<String> = (0..24).map(|h| format!("2026-11-15T{h:02}:00")).collect();
        let res = json!({
            "current_weather": { "time": "2026-11-15T13:45", "temperature": 21.6, "weathercode": 2, "windspeed": 11.6 },
            "current": { "apparent_temperature": 23.4, "relative_humidity_2m": 78 },
            "daily": { "temperature_2m_max": [27.2, 28.0], "temperature_2m_min": [18.6, 19.0] },
            "hourly": { "time": times, "temperature_2m": vec![10.4; 24], "weathercode": vec![61; 24],
                        "precipitation_probability": (0..24).map(|h| h * 4).collect::<Vec<_>>() }
        });
        let w = parse("Dhaka, Bangladesh", &res).unwrap();
        assert_eq!(w.feels.as_deref(), Some("23°"));
        assert_eq!((w.high.as_deref(), w.low.as_deref()), (Some("27°"), Some("19°")));
        assert_eq!(w.humidity, Some(78));
        assert_eq!(w.wind, Some(12)); // km/h
        assert_eq!(w.hourly.len(), 8); // the strip shows the next 8 hours
        assert_eq!(w.hourly[0].rain, Some(56)); // 2 PM: 14 * 4 %
        assert_eq!(w.rain, Some(84)); // the highest chance in the next 8 hours (9 PM)
    }

    #[test]
    fn extras_missing_are_none_not_errors() {
        let res = json!({
            "current_weather": { "time": "2026-11-15T13:45", "temperature": 21.6, "weathercode": 0 },
            "hourly": { "time": ["2026-11-15T14:00"], "temperature_2m": [20.0], "weathercode": [0] }
        });
        let w = parse("X", &res).unwrap();
        assert_eq!((w.feels, w.high, w.humidity, w.wind, w.rain), (None, None, None, None, None));
    }

    #[test]
    fn hour_labels() {
        assert_eq!(hour_label("2026-11-15T00:00"), "12AM");
        assert_eq!(hour_label("2026-11-15T12:00"), "12PM");
        assert_eq!(hour_label("2026-11-15T09:00"), "9AM");
    }
}
