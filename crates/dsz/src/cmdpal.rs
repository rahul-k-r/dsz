use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use windows_sys::Win32::Foundation::{HANDLE, HWND};
use windows_sys::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
use windows_sys::Win32::UI::WindowsAndMessaging::SetPropW;
use windows_sys::core::w;

use crate::engine::Engine;

#[derive(Deserialize)]
struct Request {
    token: String,
    profile: Option<String>,
    device: Option<u64>,
    action: Option<DeviceAction>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum DeviceAction {
    Identify,
    PowerOff,
}

fn request(reader: impl Read, token: &str) -> Option<Request> {
    let mut line = Vec::new();
    BufReader::new(reader).take(4097).read_until(b'\n', &mut line).ok()?;
    if line.len() > 4096 || line.last() != Some(&b'\n') {
        return None;
    }
    let request: Request = serde_json::from_slice(&line).ok()?;
    (request.token == token).then_some(request)
}

fn handle(mut stream: TcpStream, engine: &Engine, token: &str) -> std::io::Result<()> {
    let timeout = Some(Duration::from_millis(300));
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(timeout)?;
    let Some(request) = request(&stream, token) else { return Ok(()); };
    let devices = engine.device_list();
    let applied = match (request.device, request.profile.as_deref(), request.action) {
        (None, None, None) => true,
        (None, Some(profile), None) => engine.select_profile(profile),
        (Some(id), profile, action) => devices.iter().find(|device| device.id == id)
            .is_some_and(|device| match (profile, action) {
                (Some(name), None) if engine.profiles.find(name).is_some() => {
                    engine.set_device_profile(device, name);
                    device.identify_light();
                    true
                }
                (None, Some(DeviceAction::Identify)) if device.has_output => {
                    device.identify_light();
                    true
                }
                (None, Some(DeviceAction::PowerOff)) if device.is_bluetooth() => {
                    let mac = device.mac.lock().clone();
                    !mac.is_empty() && std::thread::Builder::new().spawn(move || {
                        crate::platform::bt_disconnect(&mac);
                    }).is_ok()
                }
                _ => false,
            }),
        _ => false,
    };
    let active = if devices.is_empty() {
        vec![engine.settings.read().default_profile.clone()]
    } else {
        devices.iter().map(|device| engine.profile_name_for(device)).collect()
    };
    let profiles = engine.profiles.all();
    let colors: std::collections::BTreeMap<_, _> = profiles.iter()
        .map(|profile| (&profile.name, profile.lighting.color)).collect();
    let controllers: Vec<_> = devices.iter().zip(&active).map(|(device, profile)| {
        let bluetooth = device.is_bluetooth();
        serde_json::json!({
            "id": device.id,
            "name": device.model.name(),
            "battery": device.live.lock().input.battery_shown().min(100),
            "bluetooth": bluetooth,
            "profile": profile,
            "identify": device.has_output,
            "powerOff": bluetooth && !device.mac.lock().is_empty(),
        })
    }).collect();
    let reply = serde_json::json!({
        "active": active,
        "profiles": engine.profiles.names(),
        "colors": colors,
        "controllers": controllers,
        "applied": applied,
    });
    serde_json::to_writer(&mut stream, &reply)?;
    stream.write_all(b"\n")
}

pub fn start(hwnd: HWND, engine: &Arc<Engine>) {
    let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) else { return; };
    let Ok(address) = listener.local_addr() else { return; };
    let mut key = [0usize; 2];
    // The endpoint is reachable only on loopback. The per-run key is shared
    // through the tray window so another Windows session cannot use the API.
    let status = unsafe {
        BCryptGenRandom(std::ptr::null_mut(), key.as_mut_ptr().cast(),
            std::mem::size_of_val(&key) as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG)
    };
    if status != 0 || key.contains(&0) {
        return;
    }
    let token = format!("{:x}:{:x}", key[0], key[1]);
    let engine = Arc::downgrade(engine);
    if std::thread::Builder::new().name("cmdpal".into()).spawn(move || {
        for stream in listener.incoming() {
            let Some(engine) = engine.upgrade() else { break; };
            let Ok(stream) = stream else { break; };
            let _ = handle(stream, &engine, &token);
        }
    }).is_err() {
        return;
    }
    unsafe {
        if SetPropW(hwnd, w!("DSZ.CmdPal.Key1"), key[0] as HANDLE) != 0
            && SetPropW(hwnd, w!("DSZ.CmdPal.Key2"), key[1] as HANDLE) != 0
        {
            SetPropW(hwnd, w!("DSZ.CmdPal.Port"), usize::from(address.port()) as HANDLE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_complete_authenticated_requests_are_accepted() {
        let parse = |line: &str| request(line.as_bytes(), "a:b");
        assert_eq!(parse("{\"token\":\"a:b\",\"profile\":\"Rocket League\"}\n")
            .unwrap().profile.as_deref(), Some("Rocket League"));
        assert!(parse("{\"token\":\"a:b\"}\n").unwrap().profile.is_none());
        assert!(parse("{\"token\":\"a:c\"}\n").is_none());
        assert!(parse("{\"token\":\"a:b\",\"device\":1,\"action\":\"unsupported\"}\n").is_none());
        assert!(parse("{\"token\":\"a:b\"}").is_none());
        assert!(parse(&format!("{{\"token\":\"a:b\",\"profile\":\"{}\"}}\n", "a".repeat(4096))).is_none());
    }

    #[test]
    fn profile_round_trip_reads_and_changes_the_default_without_a_controller() {
        let dir = std::env::temp_dir().join(format!("dsz-cmdpal-test-{}", std::process::id()));
        let engine = Engine::new(dir.clone());
        let initial = engine.settings.read().default_profile.clone();
        let other = engine.profiles.names().into_iter().find(|name| name != &initial).unwrap();
        let exchange = |profile: Option<&str>| {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let (server, _) = listener.accept().unwrap();
            serde_json::to_writer(&mut client, &serde_json::json!({
                "token": "a:b", "profile": profile,
            })).unwrap();
            client.write_all(b"\n").unwrap();
            handle(server, &engine, "a:b").unwrap();
            serde_json::from_reader::<_, serde_json::Value>(client).unwrap()
        };
        assert_eq!(exchange(None)["active"], serde_json::json!([initial]));
        assert!(exchange(None)["colors"][&initial].is_array());
        assert_eq!(exchange(None)["controllers"], serde_json::json!([]));
        assert_eq!(exchange(Some(&other))["active"], serde_json::json!([other]));
        assert_eq!(exchange(Some("missing profile"))["applied"], false);
        assert_eq!(exchange(None)["active"], serde_json::json!([other]));
        let _ = std::fs::remove_dir_all(dir);
    }
}
