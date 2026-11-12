use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::*;

fn value_wide(name: &str) -> windows::core::Result<Vec<u16>> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
            0x80070057_u32 as i32,
        )));
    }
    Ok(name.encode_utf16().chain(Some(0)).collect())
}

pub fn command(name: &str) -> windows::core::Result<Option<String>> {
    let value = value_wide(name)?;
    let mut buffer = [0_u16; 4096];
    let mut bytes = std::mem::size_of_val(&buffer) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    result.ok()?;
    let end = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    String::from_utf16(&buffer[..end]).map(Some).map_err(|_| {
        windows::core::Error::from_hresult(windows::core::HRESULT(0x8007000d_u32 as i32))
    })
}

pub fn configure(name: &str, command: Option<&str>) -> windows::core::Result<()> {
    let value = value_wide(name)?;
    let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let result = match command {
        Some(command) => {
            if command.contains('\0') {
                return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                    0x80070057_u32 as i32,
                )));
            }
            let data: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key,
                    PCWSTR(value.as_ptr()),
                    REG_SZ.0,
                    Some(data.as_ptr().cast()),
                    (data.len() * 2) as u32,
                )
            }
        }
        None => unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key, PCWSTR(value.as_ptr())) },
    };
    if command.is_none() && result == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        result.ok()
    }
}
