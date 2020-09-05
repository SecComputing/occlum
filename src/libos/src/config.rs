use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ffi::CString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sgxfs::SgxFile;

lazy_static! {
    pub static ref LIBOS_CONFIG: Config = {
        fn load_config(config_path: &str) -> Result<Config> {
            let mut config_file = {
                let config_file =
                    SgxFile::open_integrity_only(config_path).map_err(|e| errno!(e))?;

                let actual_mac = config_file.get_mac().map_err(|e| errno!(e))?;
                let expected_mac = conf_get_hardcoded_file_mac();
                if actual_mac != expected_mac {
                    return_errno!(EINVAL, "unexpected file MAC");
                }

                config_file
            };
            let config_json = {
                let mut config_json = String::new();
                config_file
                    .read_to_string(&mut config_json)
                    .map_err(|e| errno!(e))?;
                config_json
            };
            let config_input: InputConfig =
                serde_json::from_str(&config_json).map_err(|e| errno!(e))?;
            let config = Config::from_input(&config_input)
                .cause_err(|e| errno!(EINVAL, "invalid config JSON"))?;
            Ok(config)
        }
        let config_path = unsafe { format!("{}{}", INSTANCE_DIR, "/build/Occlum.json.protected") };
        match load_config(&config_path) {
            Err(e) => {
                error!("failed to load config: {}", e.backtrace());
                panic!();
            }
            Ok(config) => config,
        }
    };
}

// This value will be modified during occlum build
#[no_mangle]
#[link_section = ".builtin_config"]
static OCCLUM_JSON_MAC: [i8; 48] = [0; 48];

fn conf_get_hardcoded_file_mac() -> sgx_aes_gcm_128bit_tag_t {
    assert!(
        *OCCLUM_JSON_MAC.last().unwrap() == 0,
        "must be a null-terminated C string"
    );

    let mac_str = unsafe {
        CStr::from_ptr(&OCCLUM_JSON_MAC as *const i8)
            .to_str()
            .expect("MAC contains non UTF-8 characters")
    };

    let mac = parse_mac(mac_str).expect("MAC string cannot be converted to numbers");
    mac
}

fn parse_mac(mac_str: &str) -> Result<sgx_aes_gcm_128bit_tag_t> {
    let bytes_str_vec = {
        let bytes_str_vec: Vec<&str> = mac_str.split("-").collect();
        if bytes_str_vec.len() != 16 {
            return_errno!(EINVAL, "The length or format of MAC string is invalid");
        }
        bytes_str_vec
    };

    let mut mac: sgx_aes_gcm_128bit_tag_t = Default::default();
    for (byte_i, byte_str) in bytes_str_vec.iter().enumerate() {
        mac[byte_i] = u8::from_str_radix(byte_str, 16).map_err(|e| errno!(e))?;
    }
    Ok(mac)
}

#[derive(Debug)]
pub struct Config {
    pub resource_limits: ConfigResourceLimits,
    pub process: ConfigProcess,
    pub env: ConfigEnv,
    pub entry_points: Vec<PathBuf>,
    pub mount: Vec<ConfigMount>,
    pub networking: ConfigNet,
}

#[derive(Debug)]
pub struct ConfigResourceLimits {
    pub user_space_size: usize,
}

#[derive(Debug)]
pub struct ConfigProcess {
    pub default_stack_size: usize,
    pub default_heap_size: usize,
    pub default_mmap_size: usize,
}

#[derive(Debug)]
pub struct ConfigEnv {
    pub default: Vec<CString>,
    pub untrusted: HashSet<String>,
}

#[derive(Debug)]
pub struct ConfigMount {
    pub type_: ConfigMountFsType,
    pub target: PathBuf,
    pub source: Option<PathBuf>,
    pub options: ConfigMountOptions,
}

#[derive(Debug, PartialEq)]
#[allow(non_camel_case_types)]
pub enum ConfigMountFsType {
    TYPE_SEFS,
    TYPE_HOSTFS,
    TYPE_RAMFS,
    TYPE_UNIONFS,
}

#[derive(Debug)]
pub struct ConfigMountOptions {
    pub integrity_only: bool,
    pub mac: Option<sgx_aes_gcm_128bit_tag_t>,
    pub layers: Option<Vec<ConfigMount>>,
    pub temporary: bool,
}

#[derive(Debug)]
pub struct ConfigNet {
    pub host_paths: Vec<String>,
}

impl Config {
    fn from_input(input: &InputConfig) -> Result<Config> {
        let resource_limits = ConfigResourceLimits::from_input(&input.resource_limits)?;
        let process = ConfigProcess::from_input(&input.process)?;
        let env = ConfigEnv::from_input(&input.env)?;
        let entry_points = {
            let mut entry_points = Vec::new();
            for ep in &input.entry_points {
                let ep_path = Path::new(ep).to_path_buf();
                if !ep_path.is_absolute() {
                    return_errno!(EINVAL, "entry point must be an absolute path")
                }
                entry_points.push(ep_path);
            }
            entry_points
        };
        let mount = {
            let mut mount = Vec::new();
            for input_mount in &input.mount {
                mount.push(ConfigMount::from_input(&input_mount)?);
            }
            mount
        };
        let networking = ConfigNet::from_input(&input.networking);

        Ok(Config {
            resource_limits,
            process,
            env,
            entry_points,
            mount,
            networking,
        })
    }
}

impl ConfigResourceLimits {
    fn from_input(input: &InputConfigResourceLimits) -> Result<ConfigResourceLimits> {
        let user_space_size = parse_memory_size(&input.user_space_size)?;
        Ok(ConfigResourceLimits { user_space_size })
    }
}

impl ConfigProcess {
    fn from_input(input: &InputConfigProcess) -> Result<ConfigProcess> {
        let default_stack_size = parse_memory_size(&input.default_stack_size)?;
        let default_heap_size = parse_memory_size(&input.default_heap_size)?;
        let default_mmap_size = parse_memory_size(&input.default_mmap_size)?;
        Ok(ConfigProcess {
            default_stack_size,
            default_heap_size,
            default_mmap_size,
        })
    }
}

impl ConfigEnv {
    fn from_input(input: &InputConfigEnv) -> Result<ConfigEnv> {
        Ok(ConfigEnv {
            default: input.default.clone(),
            untrusted: input.untrusted.clone(),
        })
    }
}

impl ConfigMount {
    fn from_input(input: &InputConfigMount) -> Result<ConfigMount> {
        const ALL_FS_TYPES: [&str; 4] = ["sefs", "hostfs", "ramfs", "unionfs"];

        let type_ = match input.type_.as_str() {
            "sefs" => ConfigMountFsType::TYPE_SEFS,
            "hostfs" => ConfigMountFsType::TYPE_HOSTFS,
            "ramfs" => ConfigMountFsType::TYPE_RAMFS,
            "unionfs" => ConfigMountFsType::TYPE_UNIONFS,
            _ => {
                return_errno!(EINVAL, "Unsupported file system type");
            }
        };
        let target = {
            let target = PathBuf::from(&input.target);
            if !target.starts_with("/") {
                return_errno!(EINVAL, "Target must be an absolute path");
            }
            target
        };
        let source = input.source.as_ref().map(|s| PathBuf::from(s));
        let options = ConfigMountOptions::from_input(&input.options)?;
        Ok(ConfigMount {
            type_,
            target,
            source,
            options,
        })
    }
}

impl ConfigMountOptions {
    fn from_input(input: &InputConfigMountOptions) -> Result<ConfigMountOptions> {
        let (integrity_only, mac) = if !input.integrity_only {
            (false, None)
        } else {
            if input.mac.is_none() {
                return_errno!(EINVAL, "MAC is expected");
            }
            (true, Some(parse_mac(&input.mac.as_ref().unwrap())?))
        };
        let layers = if let Some(layers) = &input.layers {
            let layers = layers
                .iter()
                .map(|config| ConfigMount::from_input(config).expect("invalid mount config"))
                .collect();
            Some(layers)
        } else {
            None
        };
        Ok(ConfigMountOptions {
            integrity_only,
            mac,
            layers,
            temporary: input.temporary,
        })
    }
}

impl ConfigNet {
    fn from_input(input: &InputConfigNet) -> Self {
        Self {
            host_paths: input.host_paths.clone(),
        }
    }
}

fn parse_memory_size(mem_str: &str) -> Result<usize> {
    const UNIT2FACTOR: [(&str, usize); 5] = [
        ("KB", 1024),
        ("MB", 1024 * 1024),
        ("GB", 1024 * 1024 * 1024),
        ("TB", 1024 * 1024 * 1024 * 1024),
        ("B", 1),
    ];

    let mem_str = mem_str.trim();
    let (unit, factor) = UNIT2FACTOR
        .iter()
        .position(|(unit, _)| mem_str.ends_with(unit))
        .ok_or_else(|| errno!(EINVAL, "No unit"))
        .map(|unit_i| &UNIT2FACTOR[unit_i])?;
    let number = match mem_str[0..mem_str.len() - unit.len()]
        .trim()
        .parse::<usize>()
    {
        Err(_) => return_errno!(EINVAL, "No number"),
        Ok(number) => number,
    };
    Ok(number * factor)
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct InputConfig {
    #[serde(default)]
    pub resource_limits: InputConfigResourceLimits,
    #[serde(default)]
    pub process: InputConfigProcess,
    #[serde(default)]
    pub env: InputConfigEnv,
    #[serde(default)]
    pub entry_points: Vec<String>,
    #[serde(default)]
    pub mount: Vec<InputConfigMount>,
    #[serde(default)]
    pub networking: InputConfigNet,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct InputConfigResourceLimits {
    #[serde(default = "InputConfigResourceLimits::get_user_space_size")]
    pub user_space_size: String,
}

impl InputConfigResourceLimits {
    fn get_user_space_size() -> String {
        "128MB".to_string()
    }
}

impl Default for InputConfigResourceLimits {
    fn default() -> InputConfigResourceLimits {
        InputConfigResourceLimits {
            user_space_size: InputConfigResourceLimits::get_user_space_size(),
        }
    }
}
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct InputConfigProcess {
    #[serde(default = "InputConfigProcess::get_default_stack_size")]
    pub default_stack_size: String,
    #[serde(default = "InputConfigProcess::get_default_heap_size")]
    pub default_heap_size: String,
    #[serde(default = "InputConfigProcess::get_default_mmap_size")]
    pub default_mmap_size: String,
}

impl InputConfigProcess {
    fn get_default_stack_size() -> String {
        "8MB".to_string()
    }

    fn get_default_heap_size() -> String {
        "16MB".to_string()
    }

    fn get_default_mmap_size() -> String {
        "32MB".to_string()
    }
}

impl Default for InputConfigProcess {
    fn default() -> InputConfigProcess {
        InputConfigProcess {
            default_stack_size: InputConfigProcess::get_default_stack_size(),
            default_heap_size: InputConfigProcess::get_default_heap_size(),
            default_mmap_size: InputConfigProcess::get_default_mmap_size(),
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct InputConfigEnv {
    pub default: Vec<CString>,
    pub untrusted: HashSet<String>,
}

impl Default for InputConfigEnv {
    fn default() -> InputConfigEnv {
        InputConfigEnv {
            default: Vec::new(),
            untrusted: HashSet::new(),
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct InputConfigMount {
    #[serde(rename = "type")]
    pub type_: String,
    pub target: String,
    pub source: Option<String>,
    #[serde(default)]
    pub options: InputConfigMountOptions,
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
struct InputConfigMountOptions {
    #[serde(default)]
    pub integrity_only: bool,
    #[serde(rename = "MAC")]
    #[serde(default)]
    pub mac: Option<String>,
    #[serde(default)]
    pub layers: Option<Vec<InputConfigMount>>,
    #[serde(default)]
    pub temporary: bool,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct InputConfigNet {
    pub host_paths: Vec<String>,
}

impl Default for InputConfigNet {
    fn default() -> Self {
        Self {
            host_paths: Vec::new(),
        }
    }
}
