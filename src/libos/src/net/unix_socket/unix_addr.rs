use super::*;
use std::collections::HashSet;
use std::fmt;

const MAX_PATH_LEN: usize = 108;

lazy_static! {
    pub static ref HOST_UNIX_ADDRS: Vec<SockAddr> = config::LIBOS_CONFIG
        .networking
        .host_paths
        .iter()
        .map(|path| SockAddr::UnixSocket(UnixAddr::new(&path).unwrap()))
        .collect();
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct UnixAddr {
    sun_family: ProtocolFamily,
    sun_path: [u8; MAX_PATH_LEN],
    path_len: u16,
}

impl UnixAddr {
    pub fn new(path: &str) -> Result<Self> {
        let path_len = path.len();
        if path_len > MAX_PATH_LEN {
            return_errno!(ENAMETOOLONG, "the path is too long");
        }

        let sun_family = ProtocolFamily::PF_LOCAL;

        let mut sun_path = [0; 108];
        sun_path[..path_len].copy_from_slice(&path.as_bytes());

        let path_len = path_len as u16;
        Ok(Self {
            sun_family,
            sun_path,
            path_len,
        })
    }

    pub fn path(&self) -> &str {
        std::str::from_utf8(&self.sun_path[0..self.path_len as usize]).unwrap()
    }

    // Return the length of sun_family and part of sun_path that contains data.
    pub fn len(&self) -> usize {
        // TODO: parse the string length inside sun_path and remember to consider abstract name
        self.path_len as usize + std::mem::size_of::<ProtocolFamily>()
    }

    pub fn try_from(addr: &SockAddr) -> Result<Self> {
        if let SockAddr::UnixSocket(addr_un) = addr {
            Ok(*addr_un)
        } else {
            return_errno!(EINVAL, "not an address for unix");
        }
    }
}

impl Debug for UnixAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "UnixAddr {{ family: {:?}, sun_path: ", self.sun_family)?;
        self.sun_path[..self.path_len as usize].fmt(f)?;
        write!(f, ", length: {}}}", self.path_len)
    }
}

impl PartialEq for UnixAddr {
    fn eq(&self, other: &Self) -> bool {
        // FIXME: for bind abstract address, diffrent lengths means different address.
        self.sun_family == other.sun_family
            && self
                .sun_path
                .iter()
                .zip(other.sun_path.iter())
                .all(|(x, y)| x == y)
    }
}

impl Eq for UnixAddr {}
