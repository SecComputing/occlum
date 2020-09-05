use super::*;

// The protocol family generally is the same as the address family
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum ProtocolFamily {
    PF_UNSPEC = 0,
    PF_LOCAL = 1,
    /* Hide the protocols with the same number
    PF_UNIX       = PF_LOCAL,
    PF_FILE       = PF_LOCAL,
    */
    PF_INET = 2,
    PF_AX25 = 3,
    PF_IPX = 4,
    PF_APPLETALK = 5,
    PF_NETROM = 6,
    PF_BRIDGE = 7,
    PF_ATMPVC = 8,
    PF_X25 = 9,
    PF_INET6 = 10,
    PF_ROSE = 11,
    PF_DECnet = 12,
    PF_NETBEUI = 13,
    PF_SECURITY = 14,
    PF_KEY = 15,
    PF_NETLINK = 16,
    /* Hide the protocol with the same number
    PF_ROUTE      = PF_NETLINK,
    */
    PF_PACKET = 17,
    PF_ASH = 18,
    PF_ECONET = 19,
    PF_ATMSVC = 20,
    PF_RDS = 21,
    PF_SNA = 22,
    PF_IRDA = 23,
    PF_PPPOX = 24,
    PF_WANPIPE = 25,
    PF_LLC = 26,
    PF_IB = 27,
    PF_MPLS = 28,
    PF_CAN = 29,
    PF_TIPC = 30,
    PF_BLUETOOTH = 31,
    PF_IUCV = 32,
    PF_RXRPC = 33,
    PF_ISDN = 34,
    PF_PHONET = 35,
    PF_IEEE802154 = 36,
    PF_CAIF = 37,
    PF_ALG = 38,
    PF_NFC = 39,
    PF_VSOCK = 40,
    PF_KCM = 41,
    PF_QIPCRTR = 42,
    PF_SMC = 43,
    PF_XDP = 44,
    PF_MAX = 45,
}

impl ProtocolFamily {
    pub fn try_from(pf: u16) -> Result<Self> {
        if pf > Self::PF_MAX as u16 {
            return_errno!(EINVAL, "Unknown protocol or address family");
        } else {
            Ok(unsafe { core::mem::transmute(pf) })
        }
    }
}
