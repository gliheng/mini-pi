use std::path::PathBuf;

use reqwest::blocking;

/// Returns the directory used for application data (`~/.mini-pi`).
pub fn app_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mini-pi")
}

/// Returns the path where the bundled cloudflared binary should live.
pub fn app_data_cloudflared_path() -> PathBuf {
    app_data_dir()
        .join("bin")
        .join(if cfg!(target_os = "windows") {
            "cloudflared.exe"
        } else {
            "cloudflared"
        })
}

/// Returns the official download URL for the current platform and architecture.
/// Replace these placeholders with the exact URLs you want to distribute.
pub fn download_url() -> Result<&'static str, String> {
    let url = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-darwin-arm64.tgz"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-darwin-amd64.tgz"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-arm64"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-amd64.exe"
    } else {
        return Err(format!(
            "unsupported platform: {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
    };
    Ok(url)
}

/// Downloads the cloudflared binary for the current platform, saves it to the
/// app data folder, makes it executable, and returns the absolute path.
pub fn download_and_install() -> Result<PathBuf, String> {
    let target = app_data_cloudflared_path();
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create directory: {}", e))?;
    }

    let url = download_url()?;
    let mut response = blocking::get(url).map_err(|e| format!("download failed: {}", e))?;

    #[cfg(target_os = "macos")]
    {
        // macOS releases are .tgz archives; stream to a temp file and extract.
        let temp_path = app_data_dir().join("bin").join("cloudflared-download.tgz");
        {
            let mut temp_file = std::fs::File::create(&temp_path)
                .map_err(|e| format!("failed to create temp file: {}", e))?;
            response
                .copy_to(&mut temp_file)
                .map_err(|e| format!("download failed: {}", e))?;
        }
        let binary_bytes = extract_cloudflared_tgz(&temp_path)?;
        std::fs::write(&target, binary_bytes)
            .map_err(|e| format!("failed to write binary: {}", e))?;
        let _ = std::fs::remove_file(&temp_path);
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Linux/Windows releases are raw executables; stream directly to disk.
        let mut file =
            std::fs::File::create(&target).map_err(|e| format!("failed to create file: {}", e))?;
        response
            .copy_to(&mut file)
            .map_err(|e| format!("download failed: {}", e))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&target)
            .map_err(|e| format!("failed to read permissions: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&target, perms)
            .map_err(|e| format!("failed to set permissions: {}", e))?;
    }

    Ok(target)
}

#[cfg(target_os = "macos")]
fn extract_cloudflared_tgz(tgz_path: &std::path::Path) -> Result<Vec<u8>, String> {
    let file =
        std::fs::File::open(tgz_path).map_err(|e| format!("failed to open archive: {}", e))?;
    let tar = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(tar);
    let mut entries = archive
        .entries()
        .map_err(|e| format!("failed to read archive: {}", e))?;

    while let Some(entry) = entries.next() {
        let mut entry = entry.map_err(|e| format!("failed to read archive entry: {}", e))?;
        let path = entry
            .path()
            .map_err(|e| format!("failed to read entry path: {}", e))?;
        if path.file_name().and_then(|n| n.to_str()) == Some("cloudflared") {
            let mut buf = Vec::new();
            std::io::copy(&mut entry, &mut buf)
                .map_err(|e| format!("failed to extract binary: {}", e))?;
            return Ok(buf);
        }
    }

    Err("cloudflared binary not found in downloaded archive".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_data_cloudflared_path_uses_mini_pi_bin() {
        let path = app_data_cloudflared_path();
        let parent = path.parent().expect("path has parent");
        assert!(path.to_string_lossy().contains(".mini-pi"));
        assert_eq!(parent.file_name().expect("parent has file name"), "bin");
    }
}
