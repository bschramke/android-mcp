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

#[derive(Debug, PartialEq, serde::Serialize)]
pub struct CpuInfo {
    pub abi: Option<String>,
    pub abilist: Vec<String>,
    pub abilist32: Vec<String>,
    pub abilist64: Vec<String>,
}

#[derive(Debug, PartialEq, serde::Serialize)]
pub struct HardwareInfo {
    pub board: Option<String>,
    pub hardware: Option<String>,
    pub soc_manufacturer: Option<String>,
    pub soc_model: Option<String>,
    pub cpu: CpuInfo,
}

#[derive(Debug, PartialEq, serde::Serialize)]
pub struct ProductInfo {
    pub brand: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub device: Option<String>,
    pub name: Option<String>,
    pub locale: Option<String>,
    pub first_api_level: Option<u32>,
}

#[derive(Debug, PartialEq, serde::Serialize)]
pub struct AndroidInfo {
    pub release: Option<String>,
    pub api_level: Option<u32>,
    pub security_patch: Option<String>,
}

#[derive(Debug, PartialEq, serde::Serialize)]
pub struct BuildInfo {
    pub id: Option<String>,
    pub fingerprint: Option<String>,
    pub incremental: Option<String>,
    pub build_type: Option<String>,
    pub tags: Option<String>,
}

#[derive(Debug, PartialEq, serde::Serialize)]
pub struct DeviceInfo {
    pub serial: String,
    pub product: ProductInfo,
    pub hardware: HardwareInfo,
    pub android: AndroidInfo,
    pub build: BuildInfo,
    pub boot_completed: Option<bool>,
    pub is_emulator: Option<bool>,
}
