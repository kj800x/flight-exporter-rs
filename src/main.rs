use axum::{http::header, response::IntoResponse, routing::get, Router};
use prometheus::{Encoder, GaugeVec, Opts, Registry, TextEncoder};
use serde::Deserialize;
use std::sync::Arc;

// --- Serde structs for adsb.lol API ---

#[derive(Debug, Deserialize)]
struct AdsbResponse {
    #[serde(default)]
    ac: Vec<Aircraft>,
}

#[derive(Debug, Deserialize)]
struct Aircraft {
    alt_baro: Option<AltBaro>,
    alt_geom: Option<f64>,
    gs: Option<f64>,
    tas: Option<f64>,
    ias: Option<f64>,
    mach: Option<f64>,
    lat: Option<f64>,
    lon: Option<f64>,
    track: Option<f64>,
    roll: Option<f64>,
    baro_rate: Option<f64>,
    geom_rate: Option<f64>,
    nav_altitude_mcp: Option<f64>,
    nav_heading: Option<f64>,
    nav_qnh: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AltBaro {
    Altitude(f64),
    Ground(String),
}

impl AltBaro {
    fn as_feet(&self) -> f64 {
        match self {
            AltBaro::Altitude(v) => *v,
            AltBaro::Ground(_) => 0.0,
        }
    }
}

// --- IATA to ICAO callsign conversion ---

fn iata_to_icao_callsign(flight_number: &str) -> String {
    // IATA airline codes are 2 characters (may include digits, e.g. B6, F9, G4).
    // The flight number portion is the remaining digits.
    // Try 2-char prefix first, then 1-char, to match codes like "B6".
    let (iata, numeric) = if flight_number.len() >= 2
        && flight_number[2..].chars().all(|c| c.is_ascii_digit())
        && !flight_number[2..].is_empty()
    {
        (&flight_number[..2], &flight_number[2..])
    } else {
        let alpha_len = flight_number
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .count();
        if alpha_len == 0 {
            return flight_number.to_uppercase();
        }
        (&flight_number[..alpha_len], &flight_number[alpha_len..])
    };

    let icao = match iata.to_uppercase().as_str() {
        "AA" => "AAL",
        "UA" => "UAL",
        "DL" => "DAL",
        "WN" => "SWA",
        "AS" => "ASA",
        "B6" => "JBU",
        "NK" => "NKS",
        "F9" => "FFT",
        "HA" => "HAL",
        "SY" => "SCX",
        "G4" => "AAY",
        "BA" => "BAW",
        "LH" => "DLH",
        "AF" => "AFR",
        "KL" => "KLM",
        "AC" => "ACA",
        "QF" => "QFA",
        "EK" => "UAE",
        "SQ" => "SIA",
        "NH" => "ANA",
        "JL" => "JAL",
        "CX" => "CPA",
        "TK" => "THY",
        "LX" => "SWR",
        "AY" => "FIN",
        "IB" => "IBE",
        "QR" => "QTR",
        "EY" => "ETD",
        "VS" => "VIR",
        "AM" => "AMX",
        _ => iata.to_uppercase().leak(),
    };

    format!("{icao}{numeric}")
}

// --- Prometheus metrics ---

struct FlightMetrics {
    speed: GaugeVec,
    altitude: GaugeVec,
    altitude_geom: GaugeVec,
    latitude: GaugeVec,
    longitude: GaugeVec,
    track: GaugeVec,
    tas: GaugeVec,
    ias: GaugeVec,
    mach: GaugeVec,
    roll: GaugeVec,
    baro_rate: GaugeVec,
    geom_rate: GaugeVec,
    nav_altitude_mcp: GaugeVec,
    nav_heading: GaugeVec,
    nav_qnh: GaugeVec,
    registry: Registry,
}

impl FlightMetrics {
    fn new() -> Self {
        let registry = Registry::new();

        macro_rules! gauge {
            ($name:expr, $help:expr) => {{
                let g = GaugeVec::new(Opts::new($name, $help), &["callsign"]).unwrap();
                registry.register(Box::new(g.clone())).unwrap();
                g
            }};
        }

        Self {
            speed: gauge!("flight_speed_knots", "Aircraft ground speed in knots"),
            altitude: gauge!("flight_altitude_feet", "Aircraft barometric altitude in feet"),
            altitude_geom: gauge!("flight_altitude_geom_feet", "Aircraft geometric (GPS) altitude in feet"),
            latitude: gauge!("flight_latitude_degrees", "Aircraft latitude in degrees"),
            longitude: gauge!("flight_longitude_degrees", "Aircraft longitude in degrees"),
            track: gauge!("flight_track_degrees", "Aircraft track angle in degrees"),
            tas: gauge!("flight_true_airspeed_knots", "Aircraft true airspeed in knots"),
            ias: gauge!("flight_indicated_airspeed_knots", "Aircraft indicated airspeed in knots"),
            mach: gauge!("flight_mach_number", "Aircraft Mach number"),
            roll: gauge!("flight_roll_degrees", "Aircraft roll angle in degrees"),
            baro_rate: gauge!("flight_vertical_rate_fpm", "Aircraft barometric vertical rate in feet per minute"),
            geom_rate: gauge!("flight_vertical_rate_geom_fpm", "Aircraft geometric vertical rate in feet per minute"),
            nav_altitude_mcp: gauge!("flight_nav_altitude_feet", "Aircraft selected autopilot altitude in feet"),
            nav_heading: gauge!("flight_nav_heading_degrees", "Aircraft selected autopilot heading in degrees"),
            nav_qnh: gauge!("flight_nav_qnh_hpa", "Aircraft altimeter setting in hectopascals"),
            registry,
        }
    }

    fn update(&self, callsign: &str, ac: &Aircraft) {
        macro_rules! set {
            ($gauge:expr, $val:expr) => {
                if let Some(v) = $val {
                    $gauge.with_label_values(&[callsign]).set(v);
                }
            };
        }

        set!(self.speed, ac.gs);
        set!(self.latitude, ac.lat);
        set!(self.longitude, ac.lon);
        set!(self.track, ac.track);
        set!(self.altitude_geom, ac.alt_geom);
        set!(self.tas, ac.tas);
        set!(self.ias, ac.ias);
        set!(self.mach, ac.mach);
        set!(self.roll, ac.roll);
        set!(self.baro_rate, ac.baro_rate);
        set!(self.geom_rate, ac.geom_rate);
        set!(self.nav_altitude_mcp, ac.nav_altitude_mcp);
        set!(self.nav_heading, ac.nav_heading);
        set!(self.nav_qnh, ac.nav_qnh);

        if let Some(ref alt) = ac.alt_baro {
            self.altitude.with_label_values(&[callsign]).set(alt.as_feet());
        }
    }

    fn clear(&self, callsign: &str) {
        let gauges: &[&GaugeVec] = &[
            &self.speed, &self.altitude, &self.altitude_geom, &self.latitude,
            &self.longitude, &self.track, &self.tas, &self.ias, &self.mach,
            &self.roll, &self.baro_rate, &self.geom_rate, &self.nav_altitude_mcp,
            &self.nav_heading, &self.nav_qnh,
        ];
        for g in gauges {
            let _ = g.remove_label_values(&[callsign]);
        }
    }
}

// --- Poll loop ---

async fn poll_loop(client: reqwest::Client, callsign: String, metrics: Arc<FlightMetrics>) {
    let url = format!("https://api.adsb.lol/v2/callsign/{callsign}");
    loop {
        match client.get(&url).send().await {
            Ok(resp) => match resp.json::<AdsbResponse>().await {
                Ok(data) => {
                    if let Some(ac) = data.ac.first() {
                        eprintln!("[poll] updated metrics for {callsign}");
                        metrics.update(&callsign, ac);
                    } else {
                        eprintln!("[poll] no aircraft found for {callsign}, clearing metrics");
                        metrics.clear(&callsign);
                    }
                }
                Err(e) => {
                    eprintln!("[poll] failed to parse response for {callsign}: {e}");
                }
            },
            Err(e) => {
                eprintln!("[poll] request failed for {callsign}: {e}");
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

// --- Axum handler ---

async fn metrics_handler(
    metrics: axum::extract::State<Arc<FlightMetrics>>,
) -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = metrics.registry.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();

    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        String::from_utf8(buffer).unwrap(),
    )
}

// --- Main ---

#[tokio::main]
async fn main() {
    let flight_number = std::env::var("FLIGHT_NUMBER")
        .expect("FLIGHT_NUMBER env var is required (e.g. UA1234)");

    let callsign = iata_to_icao_callsign(&flight_number);
    eprintln!("Tracking flight {flight_number} as callsign {callsign}");

    let metrics = Arc::new(FlightMetrics::new());
    let client = reqwest::Client::new();

    tokio::spawn(poll_loop(client, callsign, Arc::clone(&metrics)));

    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(metrics);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:9090").await.unwrap();
    eprintln!("Serving metrics on http://0.0.0.0:9090/metrics");
    axum::serve(listener, app).await.unwrap();
}
