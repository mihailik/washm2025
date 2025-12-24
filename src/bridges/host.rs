use crate::bridges::eval::eval_blocking;
use serde::Deserialize;

const BRIDGE_JS: &str = include_str!("host.js");

pub fn ensure_bridge() -> Result<(), String> {
    let js = format!("{}\n//# sourceURL=bridges/host.js\n", BRIDGE_JS);
    eval_blocking(&js).map(|_| ())
}

#[derive(Debug, Deserialize, Default, Clone)]
struct UserInfo {
    username: Option<String>,
    uid: Option<u32>,
    gid: Option<u32>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct StartupJson {
    hostname: Option<String>,
    #[serde(rename = "userInfo")]
    user_info: Option<UserInfo>,
}

fn parse_startup_str(s: &str) -> Option<StartupJson> {
    serde_json::from_str::<StartupJson>(s).ok()
}

fn load_startup_from_fs() -> Option<StartupJson> {
    std::fs::read_to_string("/dev/host/washm/startup")
        .ok()
        .and_then(|s| parse_startup_str(&s))
}

fn ensure_cache() -> &'static StartupJson {
    static mut CACHE: Option<StartupJson> = None;
    unsafe {
        if CACHE.is_none() {
            CACHE = load_startup_from_fs().or_else(|| Some(StartupJson::default()));
            // Process bridge is installed automatically via protocol.js bootstrap
        }
        CACHE.as_ref().unwrap()
    }
}

pub(crate) fn startup_username() -> String {
    let v = ensure_cache();
    v.user_info
        .as_ref()
        .and_then(|ui| ui.username.clone())
        .unwrap_or_else(|| "user".to_string())
}

pub(crate) fn startup_hostname() -> String {
    let v = ensure_cache();
    v.hostname
        .clone()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "localhost".to_string())
}

pub(crate) fn startup_uid() -> u32 {
    let v = ensure_cache();
    v.user_info.as_ref().and_then(|ui| ui.uid).unwrap_or(1000)
}

pub(crate) fn startup_gid() -> u32 {
    let v = ensure_cache();
    v.user_info.as_ref().and_then(|ui| ui.gid).unwrap_or(1000)
}

pub fn startup_pwd() -> String {
    std::fs::read_to_string("/dev/host/washm/start_pwd")
        .unwrap_or_else(|_| String::from("/"))
        .trim()
        .to_string()
}

// Environment variables are now provided by the host at instantiation time (WASI env).
// The protocol no longer injects environment variables post-startup, so this helper has been removed.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_json() {
        let s = r#"{"userInfo":{"username":"alice","uid":42,"gid":43}}"#;
        let got = parse_startup_str(s).unwrap();
        assert_eq!(
            got.user_info.as_ref().unwrap().username.as_ref().unwrap(),
            "alice"
        );
        assert_eq!(got.user_info.as_ref().unwrap().uid.unwrap(), 42);
        assert_eq!(got.user_info.as_ref().unwrap().gid.unwrap(), 43);
    }

    #[test]
    fn parse_partial_json() {
        let s = r#"{"userInfo":{"username":"bob"}}"#;
        let got = parse_startup_str(s).unwrap();
        assert_eq!(
            got.user_info.as_ref().unwrap().username.as_ref().unwrap(),
            "bob"
        );
        assert!(got.user_info.as_ref().unwrap().uid.is_none());
    }

    #[test]
    fn parse_with_hostname() {
        let s = r#"{"hostname":"host.local"}"#;
        let got = parse_startup_str(s).unwrap();
        assert_eq!(got.hostname.as_ref().unwrap(), "host.local");
    }
}
