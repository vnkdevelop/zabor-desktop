use windows::core::w;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::*;

pub fn command() -> windows::core::Result<Option<String>> {
    let mut buffer = [0_u16; 4096];
    let mut bytes = std::mem::size_of_val(&buffer) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            w!("zabor-native-beta"),
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

pub fn configure(command: Option<&str>) -> windows::core::Result<()> {
    let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let value = w!("zabor-native-beta");
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
                    value,
                    REG_SZ.0,
                    Some(data.as_ptr().cast()),
                    (data.len() * 2) as u32,
                )
            }
        }
        None => unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key, value) },
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
