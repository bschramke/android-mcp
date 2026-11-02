#[derive(Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
    Online,
    Offline,
    Unauthorized,
    PermissionDenied,
    Unknown,
}

impl DeviceStatus {
    pub(super) fn from_adb_state(state: &str) -> Self {
        match state {
            "device" => Self::Online,
            "offline" => Self::Offline,
            "unauthorized" => Self::Unauthorized,
            "no permissions" => Self::PermissionDenied,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, PartialEq, serde::Serialize)]
pub struct AndroidDevice {
    pub serial: String,
    pub status: DeviceStatus,
    pub adb_state: String,
    pub model: Option<String>,
    pub product: Option<String>,
    pub device: Option<String>,
    pub transport_id: Option<String>,
}

#[derive(Debug, PartialEq, serde::Serialize)]
pub struct DeviceList {
    pub devices: Vec<AndroidDevice>,
}
