//! Player accounts (`accounts.json`: name and Argon2 password hash) and login tokens
//! (`tokens.json`, 30 days), so a browser stays signed in across server restarts. A guest gets a
//! token with no account behind it (a day long), under the name it asked for or a `Guest` name.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use serde::{Deserialize, Serialize};

const TOKEN_DAYS: u64 = 30;
const GUEST_DAYS: u64 = 1;

#[derive(Serialize, Deserialize, Clone)]
struct Account {
    /// The name as typed at sign-up; accounts are keyed by its lowercase form.
    name: String,
    hash: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct Token {
    key: String,
    expires: u64,
    /// A guest's name; the token has no account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    guest: Option<String>,
}

pub struct Accounts {
    dir: PathBuf,
    accounts: HashMap<String, Account>,
    tokens: HashMap<String, Token>,
}

#[derive(Debug, PartialEq)]
pub enum LoginError {
    BadName,
    ShortPassword,
    WrongPassword,
    Closed,
    WrongJoinPassword,
    /// A guest asked for a name that has an account, or that another guest holds.
    NameTaken,
    GuestsOff,
}

impl LoginError {
    pub fn message(&self) -> &'static str {
        match self {
            LoginError::BadName => "Names are 3 to 16 letters, digits or underscores.",
            LoginError::ShortPassword => "Passwords need at least 4 characters.",
            LoginError::WrongPassword => "Wrong name or password.",
            LoginError::Closed => "This server is not taking new players.",
            LoginError::WrongJoinPassword => "That server password is not right.",
            LoginError::NameTaken => "That name is taken here: sign in with its password, or pick another.",
            LoginError::GuestsOff => "This server does not take guests: sign in with a name and password.",
        }
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub fn valid_name(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).expect("system random source");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl Accounts {
    pub fn load(dir: &Path) -> Accounts {
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok();
        let accounts = read("accounts.json").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let mut tokens: HashMap<String, Token> = read("tokens.json").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let t = now();
        tokens.retain(|_, v| v.expires > t);
        Accounts { dir: dir.to_path_buf(), accounts, tokens }
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = std::fs::write(self.dir.join("accounts.json"), serde_json::to_string_pretty(&self.accounts).unwrap());
        let _ = std::fs::write(self.dir.join("tokens.json"), serde_json::to_string(&self.tokens).unwrap());
    }

    pub fn count(&self) -> usize {
        self.accounts.len()
    }

    /// Signs in, or signs up a name nobody has; returns the display name and a new token.
    pub fn login(&mut self, name: &str, password: &str, open: bool, join_password: &str, given_join: &str) -> Result<(String, String), LoginError> {
        let name = name.trim();
        if !valid_name(name) {
            return Err(LoginError::BadName);
        }
        let key = name.to_ascii_lowercase();
        let display = match self.accounts.get(&key) {
            Some(acc) => {
                let ok = PasswordHash::new(&acc.hash).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok());
                if !ok {
                    return Err(LoginError::WrongPassword);
                }
                acc.name.clone()
            }
            None => {
                if !open {
                    return Err(LoginError::Closed);
                }
                if !join_password.is_empty() && given_join != join_password {
                    return Err(LoginError::WrongJoinPassword);
                }
                if password.chars().count() < 4 {
                    return Err(LoginError::ShortPassword);
                }
                let mut salt = [0u8; 16];
                getrandom::fill(&mut salt).expect("system random source");
                let salt = SaltString::encode_b64(&salt).expect("16-byte salt");
                let hash = Argon2::default().hash_password(password.as_bytes(), &salt).map_err(|_| LoginError::WrongPassword)?.to_string();
                self.accounts.insert(key.clone(), Account { name: name.to_string(), hash });
                name.to_string()
            }
        };
        let token = random_hex(24);
        self.tokens.insert(token.clone(), Token { key, expires: now() + TOKEN_DAYS * 86400, guest: None });
        self.save();
        Ok((display, token))
    }

    /// Plays without an account: `name` when it is free (no account has it and no other guest
    /// holds it), a `Guest` name with four digits when it is empty. Guests are new players, so a
    /// closed server or a wrong server password refuses them as it refuses a sign-up.
    pub fn guest(&mut self, name: &str, allowed: bool, join_password: &str, given_join: &str) -> Result<(String, String), LoginError> {
        if !allowed {
            return Err(LoginError::GuestsOff);
        }
        if !join_password.is_empty() && given_join != join_password {
            return Err(LoginError::WrongJoinPassword);
        }
        let t = now();
        self.tokens.retain(|_, v| v.expires > t);
        let name = name.trim();
        let name = if name.is_empty() {
            let mut b = [0u8; 2];
            loop {
                getrandom::fill(&mut b).expect("system random source");
                let n = format!("Guest{:04}", u16::from_le_bytes(b) % 10000);
                if self.name_free(&n) {
                    break n;
                }
            }
        } else if !valid_name(name) {
            return Err(LoginError::BadName);
        } else if !self.name_free(name) {
            return Err(LoginError::NameTaken);
        } else {
            name.to_string()
        };
        let token = random_hex(24);
        let key = format!("guest:{}", name.to_ascii_lowercase());
        self.tokens.insert(token.clone(), Token { key, expires: t + GUEST_DAYS * 86400, guest: Some(name.clone()) });
        self.save();
        Ok((name, token))
    }

    /// No account and no live guest token has this name.
    fn name_free(&self, name: &str) -> bool {
        let key = name.to_ascii_lowercase();
        !self.accounts.contains_key(&key) && !self.tokens.values().any(|t| t.guest.as_deref().is_some_and(|g| g.eq_ignore_ascii_case(name)))
    }

    /// The display name a live token belongs to.
    pub fn check(&self, token: &str) -> Option<String> {
        let t = self.tokens.get(token).filter(|t| t.expires > now())?;
        if let Some(g) = &t.guest {
            return Some(g.clone());
        }
        self.accounts.get(&t.key).map(|a| a.name.clone())
    }

    /// Whether `name` has an account (a guest has none).
    pub fn is_account(&self, name: &str) -> bool {
        self.accounts.contains_key(&name.to_ascii_lowercase())
    }

    pub fn logout(&mut self, token: &str) {
        if self.tokens.remove(token).is_some() {
            self.save();
        }
    }

    /// Creates or resets an account from the command line.
    pub fn set_password(&mut self, name: &str, password: &str) -> Result<(), LoginError> {
        if !valid_name(name) {
            return Err(LoginError::BadName);
        }
        let key = name.to_ascii_lowercase();
        self.accounts.remove(&key);
        self.tokens.retain(|_, t| t.key != key);
        self.login(name, password, true, "", "").map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("mcrs-accounts-{}", random_hex(6)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn sign_up_then_sign_in_and_tokens_survive_a_reload() {
        let d = dir();
        let mut a = Accounts::load(&d);
        let (name, token) = a.login("Steve_1", "hunter2", true, "", "").unwrap();
        assert_eq!(name, "Steve_1");
        assert_eq!(a.check(&token).as_deref(), Some("Steve_1"));
        assert_eq!(a.login("steve_1", "nope", true, "", ""), Err(LoginError::WrongPassword));
        let (again, _) = a.login("STEVE_1", "hunter2", true, "", "").unwrap();
        assert_eq!(again, "Steve_1", "names match without case, the first spelling stays");
        let b = Accounts::load(&d);
        assert_eq!(b.check(&token).as_deref(), Some("Steve_1"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn closed_servers_and_join_passwords_refuse_new_names() {
        let d = dir();
        let mut a = Accounts::load(&d);
        assert_eq!(a.login("ab", "pass", true, "", ""), Err(LoginError::BadName));
        assert_eq!(a.login("new_one", "pass", false, "", ""), Err(LoginError::Closed));
        assert_eq!(a.login("new_one", "pass", true, "secret", "guess"), Err(LoginError::WrongJoinPassword));
        assert!(a.login("new_one", "pass", true, "secret", "secret").is_ok());
        assert_eq!(a.login("other", "abc", true, "", ""), Err(LoginError::ShortPassword));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn guests_play_without_an_account() {
        let d = dir();
        let mut a = Accounts::load(&d);
        a.login("Steve_1", "hunter2", true, "", "").unwrap();
        let (name, token) = a.guest("", true, "", "").unwrap();
        assert!(name.starts_with("Guest") && name.len() == 9, "{name}");
        assert_eq!(a.check(&token).as_deref(), Some(name.as_str()));
        assert!(!a.is_account(&name) && a.is_account("steve_1"));
        assert_eq!(a.count(), 1, "a guest makes no account");
        let (alex, alex_token) = a.guest(" Alex ", true, "", "").unwrap();
        assert_eq!(alex, "Alex");
        assert_eq!(a.guest("alex", true, "", ""), Err(LoginError::NameTaken), "another guest holds it");
        assert_eq!(a.guest("STEVE_1", true, "", ""), Err(LoginError::NameTaken), "an account has it");
        assert_eq!(a.guest("a b", true, "", ""), Err(LoginError::BadName));
        assert_eq!(a.guest("", false, "", ""), Err(LoginError::GuestsOff));
        assert_eq!(a.guest("", true, "secret", "guess"), Err(LoginError::WrongJoinPassword));
        // The token outlives a restart; after a sign-out the name is free again.
        assert_eq!(Accounts::load(&d).check(&alex_token).as_deref(), Some("Alex"));
        a.logout(&alex_token);
        assert!(a.check(&alex_token).is_none());
        assert_eq!(a.guest("alex", true, "", "").unwrap().0, "alex");
        std::fs::remove_dir_all(d).unwrap();
    }
}
