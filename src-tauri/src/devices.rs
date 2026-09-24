//! Device discovery: Android devices (adb), iPhones/iPads (devicectl, macOS),
//! computers in the local network (mDNS/Bonjour SSH services) and hosts from
//! ~/.ssh/config.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::proc;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// adb serial, iOS UDID, or host name.
    pub id: String,
    pub name: String,
    /// android | ios | network | ssh-config
    pub kind: String,
    pub connection: String,
    pub state: String,
    /// Hint for the user when the device needs an action (e.g. allow USB debugging).
    pub hint: Option<String>,
    pub details: HashMap<String, String>,
}

#[tauri::command]
pub async fn list_devices(include_network: bool) -> Result<Vec<Device>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let android = std::thread::spawn(android_devices);
        let ios = std::thread::spawn(ios_devices);
        let net = include_network.then(|| std::thread::spawn(network_hosts));
        let mut all = vec![];
        all.extend(android.join().unwrap_or_default());
        all.extend(ios.join().unwrap_or_default());
        if let Some(n) = net {
            all.extend(n.join().unwrap_or_default());
        }
        all.extend(ssh_config_hosts());
        all
    })
    .await
    .map_err(|e| e.to_string())
}

fn android_devices() -> Vec<Device> {
    let Some(out) = proc::run("adb", &["devices", "-l"], Duration::from_secs(8)) else {
        return vec![];
    };
    out.text
        .lines()
        .skip_while(|l| !l.starts_with("List of devices"))
        .skip(1)
        .filter_map(|line| {
            // Skip adb's own status lines ("* daemon started successfully").
            if line.starts_with('*') {
                return None;
            }
            let mut parts = line.split_whitespace();
            let serial = parts.next()?.to_string();
            let state = parts.next()?.to_string();
            if !["device", "unauthorized", "offline", "no", "recovery", "sideload", "bootloader"].contains(&state.as_str()) {
                return None;
            }
            let mut details = HashMap::new();
            for p in parts {
                if let Some((k, v)) = p.split_once(':') {
                    details.insert(k.to_string(), v.to_string());
                }
            }
            let model = details.get("model").cloned().unwrap_or_else(|| serial.clone()).replace('_', " ");
            let connection = if serial.contains(':') || serial.contains("._adb-tls") { "wlan" } else { "usb" };
            let emulator = serial.starts_with("emulator-");
            let hint = match state.as_str() {
                "unauthorized" => Some("Auf dem Gerät „USB-Debugging zulassen“ bestätigen.".to_string()),
                "offline" => Some("Gerät ist offline – Kabel neu verbinden oder Gerät entsperren.".to_string()),
                "no" => Some("Keine Berechtigung (Linux: udev-Regeln prüfen).".to_string()),
                _ => None,
            };
            Some(Device {
                id: serial,
                name: if emulator { format!("{model} (Emulator)") } else { model },
                kind: "android".into(),
                connection: if emulator { "emulator".into() } else { connection.into() },
                state,
                hint,
                details,
            })
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn ios_devices() -> Vec<Device> {
    let tmp = std::env::temp_dir().join(format!("easydeploy-devicectl-{}.json", std::process::id()));
    let path = tmp.to_string_lossy().to_string();
    let ran = proc::run(
        "xcrun",
        &["devicectl", "list", "devices", "--quiet", "--json-output", &path],
        Duration::from_secs(15),
    );
    if ran.is_none() {
        return vec![];
    }
    let Ok(text) = std::fs::read_to_string(&tmp) else { return vec![] };
    let _ = std::fs::remove_file(&tmp);
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return vec![] };
    v["result"]["devices"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|d| {
            let hw = &d["hardwareProperties"];
            let udid = hw["udid"].as_str()?.to_string();
            let name = d["deviceProperties"]["name"].as_str().unwrap_or("iOS-Gerät").to_string();
            let transport = d["connectionProperties"]["transportType"].as_str().unwrap_or("");
            let pairing = d["connectionProperties"]["pairingState"].as_str().unwrap_or("");
            let tunnel = d["connectionProperties"]["tunnelState"].as_str().unwrap_or("");
            let dev_mode = d["deviceProperties"]["developerModeStatus"].as_str().unwrap_or("");
            let mut details = HashMap::new();
            for (k, key) in [("model", "marketingName"), ("platform", "platform"), ("productType", "productType")] {
                if let Some(s) = hw[key].as_str() {
                    details.insert(k.to_string(), s.to_string());
                }
            }
            if let Some(os) = d["deviceProperties"]["osVersionNumber"].as_str() {
                details.insert("osVersion".into(), os.into());
            }
            let connection = match transport {
                "wired" => "usb",
                "localNetwork" => "wlan",
                _ => "offline",
            };
            let hint = if pairing != "paired" {
                Some("Gerät ist nicht gekoppelt – entsperren und „Diesem Computer vertrauen“ bestätigen.".to_string())
            } else if dev_mode == "disabled" {
                Some("Entwicklermodus ist aus: Einstellungen → Datenschutz & Sicherheit → Entwicklermodus.".to_string())
            } else {
                None
            };
            let state = if connection == "offline" || tunnel == "unavailable" { "offline" } else { "device" };
            Some(Device {
                id: udid,
                name,
                kind: "ios".into(),
                connection: connection.into(),
                state: state.into(),
                hint,
                details,
            })
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn ios_devices() -> Vec<Device> {
    vec![]
}

/// Browses Bonjour/mDNS for SSH services (macOS "Entfernte Anmeldung",
/// Linux avahi). This also finds laptops directly connected via a
/// USB-C / Thunderbolt network bridge.
fn network_hosts() -> Vec<Device> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};
    let Ok(daemon) = ServiceDaemon::new() else { return vec![] };
    let mut found: HashMap<String, Device> = HashMap::new();
    let receivers: Vec<_> = ["_ssh._tcp.local.", "_sftp-ssh._tcp.local."]
        .iter()
        .filter_map(|s| daemon.browse(s).ok())
        .collect();
    let deadline = Instant::now() + Duration::from_millis(2500);
    while Instant::now() < deadline {
        for rx in &receivers {
            while let Ok(ev) = rx.try_recv() {
                if let ServiceEvent::ServiceResolved(info) = ev {
                    let host = info.get_hostname().trim_end_matches('.').to_string();
                    let name = info.get_fullname().split("._").next().unwrap_or(&host).to_string();
                    let addrs: Vec<String> = info
                        .get_addresses_v4()
                        .iter()
                        .map(|a| a.to_string())
                        .collect();
                    let mut details = HashMap::new();
                    details.insert("port".into(), info.get_port().to_string());
                    if let Some(a) = addrs.first() {
                        details.insert("address".into(), a.clone());
                    }
                    let direct = addrs.iter().any(|a| a.starts_with("169.254."));
                    found.entry(host.clone()).or_insert(Device {
                        id: host,
                        name,
                        kind: "network".into(),
                        connection: if direct { "kabel".into() } else { "lan".into() },
                        state: "device".into(),
                        hint: None,
                        details,
                    });
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = daemon.shutdown();
    found.into_values().collect()
}

fn ssh_config_hosts() -> Vec<Device> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    let Ok(text) = std::fs::read_to_string(home.join(".ssh").join("config")) else { return vec![] };
    let mut out: Vec<Device> = vec![];
    let mut current: Vec<Device> = vec![];
    let flush = |current: &mut Vec<Device>, out: &mut Vec<Device>| out.append(current);
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, v)) => (k.to_lowercase(), v.trim().trim_start_matches('=').trim().to_string()),
            None => continue,
        };
        match key.as_str() {
            "host" => {
                flush(&mut current, &mut out);
                for alias in value.split_whitespace() {
                    if alias.contains('*') || alias.contains('?') || alias.starts_with('!') {
                        continue;
                    }
                    current.push(Device {
                        id: alias.to_string(),
                        name: alias.to_string(),
                        kind: "ssh-config".into(),
                        connection: "ssh".into(),
                        state: "device".into(),
                        hint: None,
                        details: HashMap::new(),
                    });
                }
            }
            "match" => flush(&mut current, &mut out),
            "hostname" | "user" | "port" | "identityfile" => {
                for d in current.iter_mut() {
                    d.details.insert(key.clone(), value.clone());
                }
            }
            _ => {}
        }
    }
    flush(&mut current, &mut out);
    out
}
