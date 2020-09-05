use super::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
#[allow(non_camel_case_types)]
pub enum SocketType {
    SOCK_STREAM = 1,
    SOCK_DGRAM = 2,
    SOCK_RAW = 3,
    SOCK_RDM = 4,
    SOCK_SEQPACKET = 5,
    SOCK_DCCP = 6,
    SOCK_PACKET = 10,
}

impl SocketType {
    pub fn try_from(sock_type: i32) -> Result<Self> {
        match sock_type {
            1 => Ok(SocketType::SOCK_STREAM),
            2 => Ok(SocketType::SOCK_DGRAM),
            3 => Ok(SocketType::SOCK_RAW),
            4 => Ok(SocketType::SOCK_RDM),
            5 => Ok(SocketType::SOCK_SEQPACKET),
            6 => Ok(SocketType::SOCK_DCCP),
            10 => Ok(SocketType::SOCK_PACKET),
            _ => return_errno!(EINVAL, "invalid socket type"),
        }
    }
}
