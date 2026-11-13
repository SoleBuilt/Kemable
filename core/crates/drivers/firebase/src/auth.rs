//! Signing in to Google's APIs with a service account
//! (`docs/decisions/0027-firebase.md`): a JWT signed with the account's RSA
//! key (RS256, by ring) is traded at its `token_uri` for an access token,
//! kept until a minute before it ends.
//!
//! The emulators take no token: a connection to one signs in as `owner`.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use kemable_core::config::Secret;
use kemable_core::{DriverError, ErrorCode, Result};
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use rustls::pki_types::PrivateKeyDer;
use rustls::pki_types::pem::PemObject;
use serde::Deserialize;
use serde_json::{Value as Json, json};

/// What Firestore and the Realtime Database need, and the account's email
/// for the Realtime Database's rules.
const SCOPES: &str = "https://www.googleapis.com/auth/datastore \
                      https://www.googleapis.com/auth/firebase.database \
                      https://www.googleapis.com/auth/userinfo.email";

/// A token is asked for again this long before Google says it ends.
const EARLY: Duration = Duration::from_secs(60);

/// The fields of a service account's JSON key file that signing in reads.
#[derive(Deserialize)]
pub(crate) struct ServiceAccount {
    pub(crate) project_id: Option<String>,
    client_email: String,
    private_key: String,
    #[serde(default = "default_token_uri")]
    token_uri: String,
}

fn default_token_uri() -> String {
    "https://oauth2.googleapis.com/token".to_owned()
}

impl ServiceAccount {
    /// Reads a key file's text. Its private key never leaves this value.
    pub(crate) fn read(text: &Secret) -> Result<Self> {
        serde_json::from_str(text.expose())
            .map_err(|error| DriverError::config(format!("the service account key is not a key file's JSON: {error}")))
    }
}

/// Base64 without padding, `-` and `_` for `+` and `/`, as JWT writes it.
pub(crate) fn base64_url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &byte)| n | (u32::from(byte) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
        }
    }
    out
}

/// The signed JWT that asks `token_uri` for an access token.
pub(crate) fn assertion(account: &ServiceAccount, now: u64) -> Result<String> {
    let key = PrivateKeyDer::from_pem_slice(account.private_key.as_bytes())
        .map_err(|error| DriverError::config(format!("the service account's private key cannot be read: {error}")))?;
    let PrivateKeyDer::Pkcs8(der) = key else {
        return Err(DriverError::config("the service account's private key is not PKCS #8"));
    };
    let pair = RsaKeyPair::from_pkcs8(der.secret_pkcs8_der())
        .map_err(|error| DriverError::config(format!("the service account's private key is not RSA: {error}")))?;
    let header = base64_url(br#"{"alg":"RS256","typ":"JWT"}"#);
    let claims = json!({
        "iss": account.client_email,
        "scope": SCOPES,
        "aud": account.token_uri,
        "iat": now,
        "exp": now + 3600,
    });
    let claims = base64_url(claims.to_string().as_bytes());
    let signed = format!("{header}.{claims}");
    let mut signature = vec![0u8; pair.public().modulus_len()];
    pair.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        signed.as_bytes(),
        &mut signature,
    )
    .map_err(|_| DriverError::internal("the JWT could not be signed"))?;
    Ok(format!("{signed}.{}", base64_url(&signature)))
}

/// How requests sign in: a service account's token, or none for an emulator.
pub(crate) enum Login {
    Account {
        account: ServiceAccount,
        token: Mutex<Option<(Secret, Instant)>>,
    },
    Emulator,
}

impl Login {
    pub(crate) fn account(account: ServiceAccount) -> Self {
        Self::Account {
            account,
            token: Mutex::new(None),
        }
    }

    /// The `Authorization` header's value: a token still good, or a new one.
    pub(crate) async fn bearer(&self, http: &reqwest::Client) -> Result<String> {
        let (account, kept) = match self {
            Self::Emulator => return Ok("Bearer owner".to_owned()),
            Self::Account { account, token } => (account, token),
        };
        let held = kept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|(_, ends)| Instant::now() + EARLY < *ends)
            .map(|(token, _)| token.expose().to_owned());
        if let Some(token) = held {
            return Ok(format!("Bearer {token}"));
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let form = format!(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={}",
            assertion(account, now)?
        );
        let response = http
            .post(&account.token_uri)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form)
            .send()
            .await
            .map_err(|error| DriverError::new(ErrorCode::Connect, error.to_string()))?;
        let status = response.status();
        let body: Json = response
            .json()
            .await
            .map_err(|error| DriverError::new(ErrorCode::Connect, error.to_string()))?;
        if !status.is_success() {
            let words = body.get("error_description").or_else(|| body.get("error")).map_or_else(
                || status.to_string(),
                |words| words.as_str().unwrap_or_default().to_owned(),
            );
            return Err(DriverError::new(ErrorCode::Auth, words));
        }
        let token = body
            .get("access_token")
            .and_then(Json::as_str)
            .ok_or_else(|| DriverError::new(ErrorCode::Auth, "Google gave no access token"))?
            .to_owned();
        let lasts = Duration::from_secs(body.get("expires_in").and_then(Json::as_u64).unwrap_or(3600));
        *kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((Secret::new(token.clone()), Instant::now() + lasts));
        Ok(format!("Bearer {token}"))
    }
}
