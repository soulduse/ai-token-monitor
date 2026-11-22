//! OS 지역 설정의 국가 코드(ISO 3166, 예: "KR") — "API였다면?" 기본 통화 추정용.
//!
//! 네트워크·위치 권한 없이 OS 설정만 읽는다. 언어와 지역은 따로 설정되므로(예: 영어 UI +
//! 한국 지역) UI 언어보다 실제 거주지에 가깝다. 실패하면 None → 프론트가 시간대로 추정한다.

use std::sync::OnceLock;

static REGION: OnceLock<Option<String>> = OnceLock::new();

pub fn get_region() -> Option<String> {
    REGION.get_or_init(read_region).clone()
}

fn normalize(code: &str) -> Option<String> {
    let code = code.trim().to_ascii_uppercase();
    (code.len() == 2 && code.bytes().all(|b| b.is_ascii_uppercase())).then_some(code)
}

/// macOS `AppleLocale` 값("ko_KR", "en_US@rg=krzzzz", "en_KR@calendar=…")에서 국가를 뽑는다.
/// `@rg=` 지역 재정의가 있으면 그것이 우선이다.
fn region_from_apple_locale(value: &str) -> Option<String> {
    if let Some(rg) = value.split(['@', ';']).find_map(|part| part.strip_prefix("rg=")) {
        if let Some(code) = rg.get(..2).and_then(normalize) {
            return Some(code);
        }
    }
    let base = value.split('@').next()?;
    normalize(base.split('_').nth(1)?)
}

/// POSIX 로캘("ko_KR.UTF-8", "en_GB@euro")에서 국가를 뽑는다.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn region_from_posix_locale(value: &str) -> Option<String> {
    let base = value.split(['.', '@']).next()?;
    normalize(base.split('_').nth(1)?)
}

/// `reg query …\Geo /v Name` 출력의 "Name    REG_SZ    KR" 줄에서 국가를 뽑는다.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn region_from_reg_output(output: &str) -> Option<String> {
    output
        .lines()
        .find(|line| line.contains("REG_SZ"))
        .and_then(|line| line.split_whitespace().last())
        .and_then(normalize)
}

#[cfg(target_os = "macos")]
fn read_region() -> Option<String> {
    let output = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleLocale"])
        .output()
        .ok()?;
    region_from_apple_locale(String::from_utf8_lossy(&output.stdout).trim())
}

#[cfg(target_os = "windows")]
fn read_region() -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("reg")
        .args(["query", r"HKCU\Control Panel\International\Geo", "/v", "Name"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    region_from_reg_output(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(target_os = "linux")]
fn read_region() -> Option<String> {
    ["LC_ALL", "LC_MONETARY", "LANG"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .find_map(|value| region_from_posix_locale(&value))
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn read_region() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apple_locale_variants() {
        assert_eq!(region_from_apple_locale("ko_KR").as_deref(), Some("KR"));
        assert_eq!(region_from_apple_locale("en_GB").as_deref(), Some("GB"));
        assert_eq!(region_from_apple_locale("en_US@rg=krzzzz").as_deref(), Some("KR"));
        assert_eq!(region_from_apple_locale("en_KR@calendar=gregorian").as_deref(), Some("KR"));
        assert_eq!(region_from_apple_locale("zh-Hant_TW").as_deref(), Some("TW"));
        assert_eq!(region_from_apple_locale("en"), None);
        assert_eq!(region_from_apple_locale(""), None);
    }

    #[test]
    fn posix_locale_variants() {
        assert_eq!(region_from_posix_locale("ko_KR.UTF-8").as_deref(), Some("KR"));
        assert_eq!(region_from_posix_locale("en_GB@euro").as_deref(), Some("GB"));
        assert_eq!(region_from_posix_locale("C.UTF-8"), None);
        assert_eq!(region_from_posix_locale("POSIX"), None);
    }

    #[test]
    fn windows_reg_output() {
        let out = "\r\nHKEY_CURRENT_USER\\Control Panel\\International\\Geo\r\n    Name    REG_SZ    KR\r\n\r\n";
        assert_eq!(region_from_reg_output(out).as_deref(), Some("KR"));
        assert_eq!(region_from_reg_output("ERROR: not found"), None);
    }
}
