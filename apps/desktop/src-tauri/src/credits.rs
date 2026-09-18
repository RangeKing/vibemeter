//! API credit balances, which are a different thing from subscription quota.
//!
//! A subscription window says how much of a plan's allowance is left and when
//! it resets. An API balance says how much money is left on a key and never
//! resets — it goes down until someone tops it up. Both belong in the same
//! sidebar, but conflating them would put a reset countdown on a figure that
//! has none and a percentage on one with no denominator.
//!
//! Endpoints and response shapes follow steipete/codexbar, which is the
//! reference for which providers expose a documented balance and where.
//! See THIRD_PARTY_NOTICES.md.
//!
//! Keys are never written to VibeMeter's database, its logs or its exports.
//! One lives in the login keychain per account; the row the app keeps holds
//! only an id, a provider and a label the user typed.
use crate::errors::{AppError, AppResult};
use crate::models::{CreditBalance, Provenance};
use serde_json::Value;

/// Providers with a documented, key-authenticated balance endpoint.
pub(crate) const API_PROVIDERS: [&str; 2] = ["deepseek", "moonshot"];

const KEYCHAIN_SERVICE: &str = "com.vibemeter.desktop.api-key";
const TIMEOUT_SECONDS: u64 = 15;

pub(crate) fn is_api_provider(provider: &str) -> bool {
    API_PROVIDERS.contains(&provider)
}

/// Environment variables that stand in for an account the user never added.
///
/// A GUI app inherits almost nothing, so this only finds a key when VibeMeter
/// was launched from a shell that had one. It costs nothing to look, and when
/// it works the provider needs no setup at all.
pub(crate) fn environment_key(provider: &str) -> Option<(&'static str, String)> {
    let names: &[&str] = match provider {
        "deepseek" => &["DEEPSEEK_API_KEY", "DEEPSEEK_KEY"],
        "moonshot" => &["MOONSHOT_API_KEY", "KIMI_CODE_API_KEY", "MOONSHOT_KEY"],
        _ => &[],
    };
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(|value| (*name, value))
    })
}

/// Which Moonshot region a key belongs to. The two are separate accounts with
/// separate balances, and a key for one is rejected by the other.
pub(crate) fn moonshot_region(label: &str) -> &'static str {
    let hint = std::env::var("MOONSHOT_REGION").unwrap_or_default();
    if hint.eq_ignore_ascii_case("cn")
        || hint.eq_ignore_ascii_case("china")
        || label.contains(".cn")
        || label.contains("cn")
    {
        "cn"
    } else {
        "ai"
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn store_key(account_id: &str, key: &str) -> AppResult<()> {
    security_framework::passwords::set_generic_password(
        KEYCHAIN_SERVICE,
        account_id,
        key.as_bytes(),
    )
    .map_err(|_| AppError::InvalidRequest("platform secure storage is unavailable".into()))
}

#[cfg(target_os = "macos")]
pub(crate) fn load_key(account_id: &str) -> AppResult<Option<String>> {
    match security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, account_id) {
        Ok(bytes) => Ok(String::from_utf8(bytes).ok().filter(|key| !key.is_empty())),
        Err(error) if error.code() == security_framework_sys::base::errSecItemNotFound => Ok(None),
        Err(_) => Err(AppError::InvalidRequest(
            "platform secure storage is unavailable".into(),
        )),
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn delete_key(account_id: &str) -> AppResult<()> {
    match security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, account_id) {
        Ok(()) => Ok(()),
        Err(error) if error.code() == security_framework_sys::base::errSecItemNotFound => Ok(()),
        Err(_) => Err(AppError::InvalidRequest(
            "platform secure storage is unavailable".into(),
        )),
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn store_key(_account_id: &str, _key: &str) -> AppResult<()> {
    Err(AppError::InvalidRequest(
        "platform secure storage is unavailable".into(),
    ))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn load_key(_account_id: &str) -> AppResult<Option<String>> {
    Ok(None)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn delete_key(_account_id: &str) -> AppResult<()> {
    Ok(())
}

pub(crate) fn fetch_balance(
    client: &reqwest::blocking::Client,
    provider: &str,
    key: &str,
    label: &str,
) -> AppResult<CreditBalance> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::ProviderUnavailable("missing API key".into()));
    }
    match provider {
        "deepseek" => {
            let body = get_json(client, "https://api.deepseek.com/user/balance", key)?;
            parse_deepseek_balance(&body)
        }
        "moonshot" => {
            let host = if moonshot_region(label) == "cn" {
                "https://api.moonshot.cn"
            } else {
                "https://api.moonshot.ai"
            };
            let body = get_json(client, &format!("{host}/v1/users/me/balance"), key)?;
            parse_moonshot_balance(&body)
        }
        _ => Err(AppError::ProviderUnavailable(
            "provider has no balance endpoint".into(),
        )),
    }
}

fn get_json(client: &reqwest::blocking::Client, url: &str, key: &str) -> AppResult<Value> {
    let response = client
        .get(url)
        .bearer_auth(key)
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECONDS))
        .send()?;
    if !response.status().is_success() {
        return Err(AppError::ProviderUnavailable(format!(
            "HTTP {}",
            response.status().as_u16()
        )));
    }
    Ok(response.json()?)
}

/// DeepSeek returns its amounts as strings, and more than one currency when an
/// account holds both. The first entry is the one the API bills against.
fn parse_deepseek_balance(body: &Value) -> AppResult<CreditBalance> {
    let info = body
        .get("balance_infos")
        .and_then(Value::as_array)
        .and_then(|infos| infos.first())
        .ok_or_else(|| AppError::ProviderUnavailable("balance_infos missing".into()))?;
    let total = number(info.get("total_balance"))
        .ok_or_else(|| AppError::ProviderUnavailable("total_balance missing".into()))?;
    Ok(CreditBalance {
        currency: info
            .get("currency")
            .and_then(Value::as_str)
            .unwrap_or("USD")
            .to_string(),
        total,
        granted: number(info.get("granted_balance")),
        topped_up: number(info.get("topped_up_balance")),
        // `is_available` is the API's own word for whether the key can still
        // spend, which is not the same as the balance being above zero.
        spendable: body.get("is_available").and_then(Value::as_bool),
        provenance: Provenance::Observed,
    })
}

/// Moonshot bills in the currency of its region and reports a voucher balance
/// separately; a negative cash balance is a real state, not a parse failure.
fn parse_moonshot_balance(body: &Value) -> AppResult<CreditBalance> {
    let data = body
        .get("data")
        .ok_or_else(|| AppError::ProviderUnavailable("data missing".into()))?;
    let total = number(data.get("available_balance"))
        .ok_or_else(|| AppError::ProviderUnavailable("available_balance missing".into()))?;
    Ok(CreditBalance {
        currency: "CNY".into(),
        total,
        granted: number(data.get("voucher_balance")),
        topped_up: number(data.get("cash_balance")),
        spendable: body.get("status").and_then(Value::as_bool),
        provenance: Provenance::Observed,
    })
}

/// Accepts either a JSON number or the string form these APIs also use.
fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_deepseek_string_amounts_and_its_own_spendable_flag() {
        let body = serde_json::json!({
            "is_available": true,
            "balance_infos": [
                {
                    "currency": "CNY",
                    "total_balance": "110.00",
                    "granted_balance": "10.00",
                    "topped_up_balance": "100.00"
                },
                { "currency": "USD", "total_balance": "5.00" }
            ]
        });
        let balance = parse_deepseek_balance(&body).expect("balance");
        assert_eq!(balance.currency, "CNY");
        assert_eq!(balance.total, 110.0);
        assert_eq!(balance.granted, Some(10.0));
        assert_eq!(balance.topped_up, Some(100.0));
        assert_eq!(balance.spendable, Some(true));
    }

    #[test]
    fn a_key_that_cannot_spend_is_not_the_same_as_an_empty_one() {
        let body = serde_json::json!({
            "is_available": false,
            "balance_infos": [{ "currency": "CNY", "total_balance": "42.00" }]
        });
        let balance = parse_deepseek_balance(&body).expect("balance");
        assert_eq!(balance.total, 42.0);
        assert_eq!(balance.spendable, Some(false));
        assert_eq!(balance.granted, None);
    }

    #[test]
    fn keeps_a_moonshot_deficit_rather_than_clamping_it() {
        let body = serde_json::json!({
            "code": 0,
            "status": true,
            "data": { "available_balance": 8.5, "voucher_balance": 12.0, "cash_balance": -3.5 }
        });
        let balance = parse_moonshot_balance(&body).expect("balance");
        assert_eq!(balance.total, 8.5);
        assert_eq!(balance.topped_up, Some(-3.5));
    }

    #[test]
    fn a_response_without_a_total_is_unavailable_rather_than_zero() {
        assert!(parse_deepseek_balance(&serde_json::json!({ "balance_infos": [] })).is_err());
        assert!(parse_deepseek_balance(&serde_json::json!({})).is_err());
        assert!(parse_moonshot_balance(&serde_json::json!({ "data": {} })).is_err());
    }

    #[test]
    fn the_two_moonshot_regions_are_separate_accounts() {
        assert_eq!(moonshot_region("api.moonshot.cn key"), "cn");
        assert_eq!(moonshot_region("work key"), "ai");
    }
}
