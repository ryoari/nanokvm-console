use crate::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub endpoints: Vec<String>,
    pub device: String,
    pub certificate_der: String,
    pub username: String,
    pub password: String,
}
pub fn default_path() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or("HOME unavailable.")?;
    Ok(base.join("nanokvm-console/connection.json"))
}
pub fn endpoint(s: &str) -> Result<String> {
    let u = url::Url::parse(s).map_err(|_| "Use an HTTPS device origin.")?;
    if u.scheme() != "https"
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || u.path() != "/"
    {
        return Err("Use an HTTPS device origin without credentials, path, query or fragment.");
    }
    Ok(u.as_str().trim_end_matches('/').to_owned())
}
impl Profile {
    pub fn load(path: &Path) -> Result<Self> {
        let meta = fs::symlink_metadata(path)
            .map_err(|_| "Device profile unavailable. Run --pair first; see --help.")?;
        if !meta.is_file() || meta.permissions().mode() & 0o077 != 0 || meta.len() > 16384 {
            return Err("Profile must be a private regular file (mode 600, at most 16 KiB).");
        }
        let p: Self =
            serde_json::from_slice(&fs::read(path).map_err(|_| "Cannot read device profile.")?)
                .map_err(|_| "Invalid device profile.")?;
        p.validate()?;
        Ok(p)
    }
    pub fn validate(&self) -> Result<()> {
        if self.endpoints.is_empty()
            || self.endpoints.len() > 3
            || self.device.is_empty()
            || self.device.len() > 128
            || self.username.is_empty()
            || self.password.is_empty()
        {
            return Err("Profile needs one to three endpoints, a device identity and login.");
        }
        let mut seen = std::collections::HashSet::new();
        for e in &self.endpoints {
            if endpoint(e)? != *e || !seen.insert(e) {
                return Err("Profile endpoints must be distinct normalized HTTPS origins.");
            }
        }
        self.certificate()?;
        Ok(())
    }
    pub fn certificate(&self) -> Result<native_tls::Certificate> {
        let der = STANDARD
            .decode(&self.certificate_der)
            .map_err(|_| "Invalid profile certificate.")?;
        native_tls::Certificate::from_der(&der).map_err(|_| "Invalid profile certificate.")
    }
    pub fn fingerprint(&self) -> Result<String> {
        Ok(format!(
            "{:x}",
            Sha256::digest(
                self.certificate()?
                    .to_der()
                    .map_err(|_| "Invalid profile certificate.")?
            )
        ))
    }
}
pub fn pair(path: &Path, endpoints: Vec<String>, device: String, certificate: &Path) -> Result<()> {
    if path.exists() {
        return Err("Profile already exists. Use a new --profile path to keep the saved profile.");
    }
    let pem = fs::read(certificate).map_err(|_| "Cannot read certificate file.")?;
    let cert = native_tls::Certificate::from_pem(&pem).map_err(|_| "Invalid PEM certificate.")?;
    let der = cert.to_der().map_err(|_| "Invalid certificate.")?;
    println!(
        "Certificate SHA-256: {:x}\nConfirm this certificate belongs to your device over a trusted local connection.",
        Sha256::digest(&der)
    );
    fn ask(label: &str) -> Result<String> {
        print!("{label}");
        io::stdout().flush().map_err(|_| "Cannot write prompt.")?;
        let mut s = String::new();
        io::stdin()
            .read_line(&mut s)
            .map_err(|_| "Cannot read prompt.")?;
        Ok(s.trim().into())
    }
    if ask("Type trust to save this certificate: ")? != "trust" {
        return Err("Pairing cancelled.");
    }
    let username = ask("Device username: ")?;
    let password = rpassword::prompt_password("Device password: ")
        .map_err(|_| "Cannot read device password.")?;
    let p = Profile {
        endpoints,
        device,
        certificate_der: STANDARD.encode(der),
        username,
        password,
    };
    p.validate()?;
    let parent = path
        .parent()
        .filter(|x| !x.as_os_str().is_empty())
        .ok_or("Use an absolute profile path.")?;
    if !path.is_absolute() {
        return Err("Use an absolute profile path.");
    }
    fs::create_dir_all(parent).map_err(|_| "Cannot create profile directory.")?;
    // Do not change permissions on a user-selected existing parent directory.
    let data = serde_json::to_vec_pretty(&p).map_err(|_| "Cannot encode profile.")?;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| "Cannot create private profile.")?;
    file.write_all(&data)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Cannot save profile.")?;
    println!("Saved device profile. Run nanokvm-console --open.");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origins_reject_credential_and_path_injection() {
        for x in [
            "http://device",
            "https://user:pass@device",
            "https://device/api",
            "https://device?a=b",
            "https://device#x",
        ] {
            assert!(endpoint(x).is_err());
        }
        assert_eq!(
            endpoint("https://127.0.0.1:443/").unwrap(),
            "https://127.0.0.1"
        );
    }
    #[test]
    fn profile_refuses_public_permissions_and_symlinks() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("profile.json");
        fs::write(&p, b"{}").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Profile::load(&p).is_err());
        std::os::unix::fs::symlink(&p, d.path().join("link")).unwrap();
        assert!(Profile::load(&d.path().join("link")).is_err());
    }
}
