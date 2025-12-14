use serde::Deserialize;

#[derive(Debug, Deserialize, Default, Clone)]
struct StartupJson {
    env: Option<std::collections::HashMap<String, String>>,
    user: Option<String>,
    hostname: Option<String>,
    uid: Option<u32>,
    gid: Option<u32>,
}

fn parse_startup_str(s: &str) -> Option<StartupJson> {
    serde_json::from_str::<StartupJson>(s).ok()
}

fn load_startup_from_fs() -> Option<StartupJson> {
    std::fs::read_to_string("/dev/host/washm/startup").ok().and_then(|s| parse_startup_str(&s))
}

fn ensure_cache() -> &'static StartupJson {
    static mut CACHE: Option<StartupJson> = None;
    unsafe {
        if CACHE.is_none() {
            CACHE = load_startup_from_fs().or_else(|| Some(StartupJson::default()));
        }
        CACHE.as_ref().unwrap()
    }
}

pub(crate) fn startup_username() -> String {
    let v = ensure_cache();
    v.user.clone().unwrap_or_else(|| "user".to_string())
}

pub(crate) fn startup_hostname() -> String {
    let v = ensure_cache();
    v.hostname.clone().or_else(|| std::env::var("HOSTNAME").ok()).unwrap_or_else(|| "localhost".to_string())
}

pub(crate) fn startup_uid() -> u32 {
    let v = ensure_cache();
    v.uid.unwrap_or(1000)
}

pub(crate) fn startup_gid() -> u32 {
    let v = ensure_cache();
    v.gid.unwrap_or(1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_json() {
        let s = r#"{"user":"alice","uid":42,"gid":43}"#;
        let got = parse_startup_str(s).unwrap();
        assert_eq!(got.user.unwrap(), "alice");
        assert_eq!(got.uid.unwrap(), 42);
        assert_eq!(got.gid.unwrap(), 43);
    }

    #[test]
    fn parse_partial_json() {
        let s = r#"{"user":"bob"}"#;
        let got = parse_startup_str(s).unwrap();
        assert_eq!(got.user.unwrap(), "bob");
        assert!(got.uid.is_none());
    }

    #[test]
    fn parse_with_hostname() {
        let s = r#"{"hostname":"host.local"}"#;
        let got = parse_startup_str(s).unwrap();
        assert_eq!(got.hostname.unwrap(), "host.local");
    }
}
