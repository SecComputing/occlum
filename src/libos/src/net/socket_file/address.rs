use super::*;

// TODO: add more addr types from man2 bind(2) and use macros to simplify the addition
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SockAddr {
    UnixSocket(UnixAddr),
    IPv4(IPv4SockAddr),
    IPv6(IPv6SockAddr),
}

impl SockAddr {
    // Caller should guarentee the sockaddr and addr_len are valid
    pub unsafe fn try_from_raw(
        sockaddr: *const libc::sockaddr,
        addr_len: libc::socklen_t,
    ) -> Result<Option<Self>> {
        if addr_len <= std::mem::size_of::<sa_family_t>() as u32 {
            return_errno!(EINVAL, "the address is too short.");
        }

        match ProtocolFamily::try_from((*sockaddr).sa_family)? {
            ProtocolFamily::PF_UNSPEC => Ok(None),
            ProtocolFamily::PF_LOCAL => {
                let path = std::str::from_utf8(std::slice::from_raw_parts(
                    (*sockaddr).sa_data.as_ptr() as *const u8,
                    addr_len as usize - std::mem::size_of::<sa_family_t>(),
                ))
                .map_err(|e| errno!(EINVAL, "the path is not valid UTF-8"))?;
                Ok(Some(Self::UnixSocket(UnixAddr::new(path)?)))
            }
            ProtocolFamily::PF_INET => {
                if addr_len < std::mem::size_of::<IPv4SockAddr>() as u32 {
                    return_errno!(EINVAL, "short address.");
                }

                Ok(Some(Self::IPv4(*(sockaddr as *const IPv4SockAddr))))
            }
            ProtocolFamily::PF_INET6 => {
                let ipv6_addr_len = std::mem::size_of::<IPv6SockAddr>() as u32;

                // Omit sin6_scope_id when it is not fully provided
                // 4 represents the size of sin6_scope_id which is not a must
                if addr_len < ipv6_addr_len - 4 {
                    return_errno!(EINVAL, "wrong address length.");
                }

                if addr_len >= ipv6_addr_len {
                    Ok(Some(Self::IPv6(*(sockaddr as *const IPv6SockAddr))))
                } else {
                    // sin6_scope_id in the passed buffer is not valid
                    // and should not be used
                    let addr = *(sockaddr as *const IPv6SockAddr);
                    Ok(Some(Self::IPv6(IPv6SockAddr {
                        sin6_family: addr.sin6_family,
                        sin6_port: addr.sin6_port,
                        sin6_flowinfo: addr.sin6_flowinfo,
                        sin6_addr: addr.sin6_addr,
                        sin6_scope_id: 0,
                    })))
                }
            }
            _ => return_errno!(EINVAL, "address type not supported"),
        }
    }

    pub fn as_ptr_and_len(&self) -> (*const libc::sockaddr, usize) {
        match self {
            SockAddr::UnixSocket(ref addr) => {
                (addr as *const _ as *const libc::sockaddr, addr.len())
            }
            SockAddr::IPv4(ref addr) => (
                addr as *const _ as *const libc::sockaddr,
                std::mem::size_of::<IPv4SockAddr>(),
            ),
            SockAddr::IPv6(ref addr) => (
                addr as *const _ as *const libc::sockaddr,
                std::mem::size_of::<IPv6SockAddr>(),
            ),
        }
    }

    pub fn copy_to_slice(&self, dst: &mut [u8]) -> usize {
        let (addr_ptr, addr_len) = self.as_ptr_and_len();
        let copy_len = std::cmp::min(addr_len, dst.len());

        dst[0..copy_len].copy_from_slice(unsafe {
            std::slice::from_raw_parts(addr_ptr as *const u8, copy_len)
        });
        addr_len
    }

    pub fn is_from_host(&self) -> bool {
        HOST_UNIX_ADDRS
            .iter()
            .find(|addr| **addr == *self)
            .is_some()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct IPv4SockAddr {
    sin_family: sa_family_t,
    sin_port: in_port_t,
    sin_addr: in_addr,
    sin_zero: [u8; 8],
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
struct in_addr {
    s_addr: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct IPv6SockAddr {
    sin6_family: sa_family_t,
    sin6_port: in_port_t,
    sin6_flowinfo: u32,
    sin6_addr: in6_addr,
    sin6_scope_id: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
struct in6_addr {
    s6_addr: [u8; 16],
}

#[allow(non_camel_case_types)]
type in_addr_t = u32;
#[allow(non_camel_case_types)]
type in_port_t = u16;
#[allow(non_camel_case_types)]
type sa_family_t = u16;
#[allow(non_camel_case_types)]
type socklen_t = u32;
#[allow(non_camel_case_types)]
type ino64_t = u64;
