//! Reading a connection URL into a [`ConnectionConfig`], so a link pasted from
//! Neon, Supabase or a `.env` file fills the connection form.
//!
//! `postgres://user:password@host:port/database?sslmode=require&...`
//! (`postgresql://` too), and the same shape for SQL Server as `sqlserver://`
//! (or `mssql://`), where a URL without a user logs in as the Windows user.
//! `sslmode` becomes the TLS mode; every other query
//! parameter goes to `options` unchanged, for the driver to read.

use std::collections::BTreeMap;

use crate::DriverError;
use crate::config::{ConnectionConfig, Secret, Target, TlsMode};

pub fn parse(url: &str) -> Result<ConnectionConfig, DriverError> {
    let url = url.trim();
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| DriverError::config("the URL has no scheme"))?;
    let engine = match scheme.to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" => "postgres",
        "sqlserver" | "mssql" => "mssql",
        other => return Err(DriverError::unsupported(format!("no driver reads {other}:// URLs"))),
    };

    let (before_query, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (authority, path) = before_query.split_once('/').unwrap_or((before_query, ""));
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((userinfo, hostport)) => (Some(userinfo), hostport),
        None => (None, authority),
    };
    let (user, password) = match userinfo {
        Some(info) => match info.split_once(':') {
            Some((user, password)) => (Some(decode(user)?), Some(decode(password)?)),
            None => (Some(decode(info)?), None),
        },
        None => (None, None),
    };
    let (host, port) = split_host_port(hostport)?;

    let mut options = BTreeMap::new();
    let mut tls = TlsMode::Prefer;
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let (key, value) = (decode(key)?, decode(value)?);
        if key == "sslmode" {
            tls = match value.as_str() {
                "disable" => TlsMode::Disable,
                "allow" | "prefer" => TlsMode::Prefer,
                "require" => TlsMode::Require,
                "verify-ca" | "verify-full" => TlsMode::VerifyFull,
                other => return Err(DriverError::config(format!("unknown sslmode {other}"))),
            };
        } else {
            options.insert(key, value);
        }
    }

    let database = Some(decode(path)?).filter(|database| !database.is_empty());
    Ok(ConnectionConfig {
        engine: engine.into(),
        target: Target::Network {
            host,
            port,
            user: user.filter(|user| !user.is_empty()),
            password: Secret::new(password.unwrap_or_default()),
            database,
            tls,
        },
        options,
    })
}

fn split_host_port(hostport: &str) -> Result<(String, Option<u16>), DriverError> {
    if hostport.contains(',') {
        return Err(DriverError::unsupported("a URL with several hosts"));
    }
    let (host, port) = if let Some(rest) = hostport.strip_prefix('[') {
        // [IPv6]:port
        let (host, after) = rest
            .split_once(']')
            .ok_or_else(|| DriverError::config("an IPv6 host is not closed"))?;
        (host, after.strip_prefix(':'))
    } else {
        match hostport.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (hostport, None),
        }
    };
    let host = decode(host)?;
    if host.is_empty() {
        return Err(DriverError::config("the URL has no host"));
    }
    let port = match port.filter(|port| !port.is_empty()) {
        Some(port) => Some(
            port.parse::<u16>()
                .map_err(|_| DriverError::config(format!("{port} is not a port")))?,
        ),
        None => None,
    };
    Ok((host, port))
}

/// Percent-decoding. `+` stays a plus, as in a URL's path and userinfo.
fn decode(text: &str) -> Result<String, DriverError> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text
                .get(i + 1..i + 3)
                .ok_or_else(|| DriverError::config("a % escape is cut short"))?;
            let byte =
                u8::from_str_radix(hex, 16).map_err(|_| DriverError::config(format!("%{hex} is not an escape")))?;
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| DriverError::config("an escape is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;

    fn network(config: &ConnectionConfig) -> (&str, Option<u16>, Option<&str>, &str, Option<&str>, TlsMode) {
        match &config.target {
            Target::Network {
                host,
                port,
                user,
                password,
                database,
                tls,
            } => (
                host,
                *port,
                user.as_deref(),
                password.expose(),
                database.as_deref(),
                *tls,
            ),
            other => panic!("not a network target: {other:?}"),
        }
    }

    #[test]
    fn a_full_url_fills_every_field() {
        let config = parse("postgresql://alice:s%40cret@db.example.com:6543/app?sslmode=require&application_name=x")
            .expect("a valid URL");
        assert_eq!(config.engine, "postgres");
        assert_eq!(
            network(&config),
            (
                "db.example.com",
                Some(6543),
                Some("alice"),
                "s@cret",
                Some("app"),
                TlsMode::Require
            )
        );
        assert_eq!(config.options.get("application_name").map(String::as_str), Some("x"));
    }

    #[test]
    fn a_neon_url_keeps_its_endpoint_host_and_verify_mode() {
        let config = parse(
            "postgres://neondb_owner:pw@ep-cool-name-123456.ap-southeast-1.aws.neon.tech/neondb?sslmode=verify-full",
        )
        .expect("a valid URL");
        let (host, port, _, _, database, tls) = network(&config);
        assert_eq!(
            (host, port, database, tls),
            (
                "ep-cool-name-123456.ap-southeast-1.aws.neon.tech",
                None,
                Some("neondb"),
                TlsMode::VerifyFull
            )
        );
    }

    #[test]
    fn a_sql_server_url_without_a_user_is_a_windows_login() {
        let config = parse("sqlserver://localhost/demo").expect("a valid URL");
        assert_eq!(config.engine, "mssql");
        assert_eq!(
            network(&config),
            ("localhost", None, None, "", Some("demo"), TlsMode::Prefer)
        );
        assert_eq!(parse("mssql://sa:pw@db:1433").expect("a valid URL").engine, "mssql");
    }

    #[test]
    fn missing_parts_stay_empty() {
        let config = parse("postgres://localhost").expect("a valid URL");
        assert_eq!(network(&config), ("localhost", None, None, "", None, TlsMode::Prefer));
    }

    #[test]
    fn ipv6_hosts_are_bracketed() {
        let config = parse("postgres://u@[::1]:5433/db").expect("a valid URL");
        assert_eq!(network(&config).0, "::1");
        assert_eq!(network(&config).1, Some(5433));
    }

    #[test]
    fn bad_urls_say_what_is_wrong() {
        assert_eq!(
            parse("localhost:5432").map_err(|e| e.code),
            Err(ErrorCode::ConfigInvalid)
        );
        assert_eq!(parse("mysql://h/db").map_err(|e| e.code), Err(ErrorCode::Unsupported));
        assert_eq!(
            parse("postgres://h:99999/db").map_err(|e| e.code),
            Err(ErrorCode::ConfigInvalid)
        );
        assert_eq!(
            parse("postgres://h/db?sslmode=maybe").map_err(|e| e.code),
            Err(ErrorCode::ConfigInvalid)
        );
        assert_eq!(
            parse("postgres://a,b/db").map_err(|e| e.code),
            Err(ErrorCode::Unsupported)
        );
    }
}
