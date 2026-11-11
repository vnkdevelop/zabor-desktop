use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Mutex;

pub const MAX_LOG_SIZE_BYTES: u64 = 10 * 1024 * 1024; // 10 MB

pub fn sanitize(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let bytes = input.as_bytes();
    let len = bytes.len();

    while i < len {
        if i + 4 <= len && &bytes[i..i + 3] == b"eyJ" {
            let mut j = i + 3;
            let mut dots = 0;
            while j < len {
                let b = bytes[j];
                if b == b'.' {
                    dots += 1;
                    j += 1;
                } else if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
                    j += 1;
                } else {
                    break;
                }
            }
            if dots == 2 && j - i > 20 {
                out.push_str("[REDACTED_JWT]");
                i = j;
                continue;
            }
        }

        if i + 10 <= len && &bytes[i..i + 10] == b"\"password\"" {
            let mut j = i + 10;
            while j < len && (bytes[j] == b' ' || bytes[j] == b':') {
                j += 1;
            }
            if j < len && bytes[j] == b'"' {
                j += 1;
                while j < len && bytes[j] != b'"' {
                    j += 1;
                }
                if j < len && bytes[j] == b'"' {
                    j += 1;
                    out.push_str("\"password\":\"***\"");
                    i = j;
                    continue;
                }
            }
        }

        if i + 3 <= len && &bytes[i..i + 3] == b"\"d\"" {
            let mut j = i + 3;
            while j < len && (bytes[j] == b' ' || bytes[j] == b':') {
                j += 1;
            }
            if j < len && bytes[j] == b'"' {
                j += 1;
                while j < len && bytes[j] != b'"' {
                    j += 1;
                }
                if j < len && bytes[j] == b'"' {
                    j += 1;
                    out.push_str("\"d\":\"***\"");
                    i = j;
                    continue;
                }
            }
        }

        if bytes[i].is_ascii_hexdigit() {
            let mut j = i;
            while j < len && bytes[j].is_ascii_hexdigit() {
                j += 1;
            }
            if j - i >= 32 {
                out.push_str("[REDACTED_PCM_BUFFER]");
                i = j;
                continue;
            }
        }

        out.push(bytes[i] as char);
        i += 1;
    }

    out
}

pub struct RotatingLogger {
    path: PathBuf,
    backup_path: PathBuf,
    file: Mutex<Option<File>>,
    max_size: u64,
}

impl RotatingLogger {
    pub fn new(path: PathBuf) -> io::Result<Self> {
        let backup_path = if let Some(ext) = path.extension() {
            let mut name = path.file_stem().unwrap_or_default().to_os_string();
            name.push(".1.");
            name.push(ext);
            path.with_file_name(name)
        } else {
            let mut name = path.as_os_str().to_os_string();
            name.push(".1");
            PathBuf::from(name)
        };

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        Ok(Self {
            path,
            backup_path,
            file: Mutex::new(Some(file)),
            max_size: MAX_LOG_SIZE_BYTES,
        })
    }

    pub fn log(&self, message: &str) -> io::Result<()> {
        let sanitized = sanitize(message);
        let mut guard = self.file.lock().unwrap();

        if let Ok(meta) = fs::metadata(&self.path) {
            if meta.len() >= self.max_size {
                drop(guard.take());
                let _ = fs::remove_file(&self.backup_path);
                let _ = fs::rename(&self.path, &self.backup_path);
                let new_file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)?;
                *guard = Some(new_file);
            }
        }

        if let Some(ref mut file) = *guard {
            writeln!(file, "{}", sanitized)?;
            file.flush()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwt_redaction() {
        let raw = "User authenticated with eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jV_fakeToken in session";
        let sanitized = sanitize(raw);
        assert!(!sanitized.contains("eyJhbGci"));
        assert!(sanitized.contains("[REDACTED_JWT]"));
        assert_eq!(
            sanitized,
            "User authenticated with [REDACTED_JWT] in session"
        );
    }

    #[test]
    fn test_password_and_jwk_redaction() {
        let raw = r#"{"username":"alice","password":"SuperSecret123!","key":{"kty":"EC","d":"secret_d_param"}}"#;
        let sanitized = sanitize(raw);
        assert!(!sanitized.contains("SuperSecret123!"));
        assert!(!sanitized.contains("secret_d_param"));
        assert!(sanitized.contains(r#""password":"***""#));
        assert!(sanitized.contains(r#""d":"***""#));
    }

    #[test]
    fn test_pcm_hex_buffer_redaction() {
        let hex = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
        let raw = format!("Audio packet dump: {} received", hex);
        let sanitized = sanitize(&raw);
        assert!(!sanitized.contains(hex));
        assert!(sanitized.contains("[REDACTED_PCM_BUFFER]"));
    }

    #[test]
    fn test_rotating_logger() {
        let temp = tempfile::tempdir().unwrap();
        let log_file = temp.path().join("test.log");
        let mut logger = RotatingLogger::new(log_file.clone()).unwrap();
        logger.max_size = 50;

        for i in 0..10 {
            logger.log(&format!("Message line {}", i)).unwrap();
        }

        assert!(log_file.exists());
        assert!(logger.backup_path.exists());
    }
}
