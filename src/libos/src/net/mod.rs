mod io_multiplexing;
mod socket_file;
mod syscalls;
mod unix_socket;

use super::*;
use std;
use untrusted::{SliceAsMutPtrAndLen, SliceAsPtrAndLen, UntrustedSliceAlloc};

pub use self::io_multiplexing::{
    clear_notifier_status, notify_thread, wait_for_notification, EpollEvent, IoEvent, PollEvent,
    PollEventFlags, THREAD_NOTIFIERS,
};
pub use self::socket_file::{
    msghdr, msghdr_mut, FileFlags, IPv4SockAddr, Iovs, IovsMut, MsgHdr, MsgHdrFlags, MsgHdrMut,
    ProtocolFamily, RecvFlags, SendFlags, SliceAsLibcIovec, SockAddr, SocketFile, SocketFileType,
    SocketType,
};
pub use self::syscalls::*;
pub use self::unix_socket::{Socket, UnixAddr, UnixSocket, UnixSocketType, HOST_UNIX_ADDRS};
