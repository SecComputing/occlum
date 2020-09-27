use super::*;
use alloc::sync::{Arc, Weak};
use fs::{AccessMode, File, FileRef, IoctlCmd, StatusFlags};
use rcore_fs::vfs::{FileType, Metadata, Timespec};
use std::any::Any;
use std::collections::btree_map::BTreeMap;
use std::fmt;
use std::sync::atomic::{spin_loop_hint, AtomicBool, AtomicUsize, Ordering};
use std::sync::SgxMutex as Mutex;
use util::ring_buf::{ring_buffer, RingBufReader, RingBufWriter};

pub struct StreamUnixSocket {
    path: RwLock<Option<String>>,                  // Set after bind
    channel: SgxMutex<Option<Arc<EndPoint>>>,      // Set after connection
    server: RwLock<Option<Arc<UnixSocketServer>>>, // Set after listen
    is_blocking: AtomicBool,
}

impl Socket for StreamUnixSocket {
    fn bind(&self, addr: SockAddr) -> Result<()> {
        // TODO: create the corresponding file in the fs
        if self.path().is_some() {
            return_errno!(EINVAL, "the socket is already bound to an address.");
        }

        if let SockAddr::UnixSocket(addr_un) = addr {
            *self.path.write().unwrap() = Some(addr_un.path().to_string());
            if let Some(ref end) = *self.channel.lock().unwrap() {
                end.set_name(addr_un.path());
            }
            Ok(())
        } else {
            return_errno!(EINVAL, "not a valid address for this socket's domain.");
        }
    }

    //TODO: add backlog support
    fn listen(&self, backlog: i32) -> Result<()> {
        let path = self
            .path()
            .ok_or_else(|| errno!(EINVAL, "the socket is not bound"))?;

        if self.server.read().unwrap().is_none() {
            *self.server.write().unwrap() = Some(UnixSocketServer::create_server(&path)?);
        }

        Ok(())
    }

    // A non-blocking accept
    fn accept(&self, flags: FileFlags, addr: Option<&mut [u8]>) -> Result<(Self, usize)> {
        let path = self
            .path()
            .ok_or_else(|| errno!(EINVAL, "the socket is not bound"))?;
        let server = UnixSocketServer::get_server(&path)
            .ok_or_else(|| errno!(EINVAL, "the socket is not listening"))?;

        let sock = server
            .pop_pending()
            .ok_or_else(|| errno!(EAGAIN, "No pending connection in the non-blocking accept"))?;

        if flags.contains(FileFlags::SOCK_NONBLOCK) {
            sock.set_non_blocking();
        }

        debug!("the accepted socket is {:?}", sock);

        let mut addr_len = 0;
        if let Some(dst) = addr {
            let channel = self.channel.lock().unwrap();
            if let Some(path) = channel.as_ref().map(|c| c.peer_name()).flatten() {
                addr_len = SockAddr::UnixSocket(UnixAddr::new(&path)?).copy_to_slice(dst);
            }
        }

        Ok((sock, addr_len))
    }

    // Backlog is temparily not supported so connect will not block
    fn connect(&self, addr: Option<SockAddr>) -> Result<()> {
        if addr.is_none() {
            *self.channel.lock().unwrap() = None;
            return Ok(());
        }

        let path = if let SockAddr::UnixSocket(ref addr_un) = addr.unwrap() {
            addr_un.path().to_string()
        } else {
            return_errno!(EAFNOSUPPORT, "invalid sa_family field");
        };

        let server = UnixSocketServer::get_server(&path)
            .ok_or_else(|| errno!(ECONNREFUSED, "no one's listening on the remote address"))?;

        let (channel_a, channel_b) = EndPoint::new_duplex_channel()?;
        channel_a.set_name(&path);

        if !self.is_blocking() {
            channel_b.set_non_blocking();
        }
        *self.channel.lock().unwrap() = Some(channel_b);

        let server_socket = StreamUnixSocket {
            path: RwLock::new(Some(path.to_string())),
            channel: SgxMutex::new(Some(channel_a)),
            server: RwLock::new(Some(server.clone())),
            is_blocking: AtomicBool::new(true),
        };

        server.push_pending(server_socket);
        Ok(())
    }

    // TODO: handle flags
    fn sendto(&self, buf: &[u8], flags: SendFlags, addr: Option<SockAddr>) -> Result<usize> {
        self.write(buf)
    }

    // TODO: handle flags
    fn recvfrom(
        &self,
        buf: &mut [u8],
        flags: RecvFlags,
        addr: Option<&mut [u8]>,
    ) -> Result<(usize, usize)> {
        let data_len = self.read(buf)?;

        let mut addr_len = 0;
        if let Some(dst) = addr {
            let channel = self.channel.lock().unwrap();
            if let Some(path) = channel.as_ref().map(|c| c.peer_name()).flatten() {
                addr_len = SockAddr::UnixSocket(UnixAddr::new(&path)?).copy_to_slice(dst);
            }
        }

        Ok((data_len, addr_len))
    }
}

impl File for StreamUnixSocket {
    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let mut channel = self.channel.lock().unwrap();
        channel
            .as_mut()
            .ok_or_else(|| errno!(ENOTCONN, "unconnected socket"))?
            .read(buf)
    }

    fn write(&self, buf: &[u8]) -> Result<usize> {
        let mut channel = self.channel.lock().unwrap();
        channel
            .as_mut()
            .ok_or_else(|| errno!(ENOTCONN, "unconnected socket"))?
            .write(buf)
    }

    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize> {
        if offset != 0 {
            return_errno!(ESPIPE, "a nonzero position is not supported");
        }
        self.read(buf)
    }

    fn write_at(&self, offset: usize, buf: &[u8]) -> Result<usize> {
        if offset != 0 {
            return_errno!(ESPIPE, "a nonzero position is not supported");
        }
        self.write(buf)
    }

    fn readv(&self, bufs: &mut [&mut [u8]]) -> Result<usize> {
        let mut channel = self.channel.lock().unwrap();
        channel
            .as_mut()
            .ok_or_else(|| errno!(ENOTCONN, "unconnected socket"))?
            .readv(bufs)
    }

    fn writev(&self, bufs: &[&[u8]]) -> Result<usize> {
        let mut channel = self.channel.lock().unwrap();
        channel
            .as_mut()
            .ok_or_else(|| errno!(ENOTCONN, "unconnected socket"))?
            .writev(bufs)
    }

    fn ioctl(&self, cmd: &mut IoctlCmd) -> Result<i32> {
        match cmd {
            IoctlCmd::FIONREAD(arg) => {
                let channel = self.channel.lock().unwrap();
                let bytes_to_read = channel
                    .as_ref()
                    .map(|c| c.bytes_to_read().min(std::i32::MAX as usize) as i32)
                    .ok_or_else(|| errno!(ENOTCONN, "unconnected socket"))?;
                **arg = bytes_to_read;
            }
            _ => return_errno!(EINVAL, "unknown ioctl cmd for unix socket"),
        }
        Ok(0)
    }

    fn get_access_mode(&self) -> Result<AccessMode> {
        Ok(AccessMode::O_RDWR)
    }

    fn get_status_flags(&self) -> Result<StatusFlags> {
        if self.is_blocking() {
            Ok(StatusFlags::empty())
        } else {
            Ok(StatusFlags::O_NONBLOCK)
        }
    }

    fn set_status_flags(&self, new_status_flags: StatusFlags) -> Result<()> {
        // Only O_NONBLOCK, O_ASYNC and O_DIRECT can be set
        let status_flags = new_status_flags
            & (StatusFlags::O_NONBLOCK | StatusFlags::O_ASYNC | StatusFlags::O_DIRECT);

        // Only O_NONBLOCK is supported
        if new_status_flags.contains(StatusFlags::O_NONBLOCK) {
            self.set_non_blocking();
        } else {
            self.set_blocking();
        }
        Ok(())
    }

    fn poll(&self) -> Result<PollEventFlags> {
        if let Some(ref channel) = *self.channel.lock().unwrap() {
            channel.poll()
        } else {
            if self.path().is_some() && self.server.read().unwrap().is_some() {
                // Result on linux for listening socket
                Ok(PollEventFlags::empty())
            } else {
                // Result for a unconnected socket (0x314)
                Ok(PollEventFlags::POLLHUP
                    | PollEventFlags::POLLOUT
                    | PollEventFlags::POLLWRBAND
                    | PollEventFlags::POLLWRNORM)
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

static SOCKETPAIR_NUM: AtomicUsize = AtomicUsize::new(0);
const SOCK_PATH_PREFIX: &str = "socketpair_";

impl StreamUnixSocket {
    pub fn new(flags: FileFlags) -> Result<Self> {
        Ok(Self {
            path: RwLock::new(None),
            channel: SgxMutex::new(None),
            server: RwLock::new(None),
            is_blocking: AtomicBool::new(!flags.contains(FileFlags::SOCK_NONBLOCK)),
        })
    }

    pub fn path(&self) -> Option<String> {
        self.path.read().unwrap().clone()
    }

    pub fn socketpair(flags: FileFlags) -> Result<(Self, Self)> {
        let mut listen_socket = Self::new(flags)?;
        let bound_path = listen_socket.bind_until_success();
        listen_socket.listen(1)?;
        let mut client_socket = Self::new(flags)?;
        client_socket.connect(Some(bound_path))?;
        let (accepted_socket, _) = listen_socket.accept(flags, None)?;
        Ok((client_socket, accepted_socket))
    }

    fn bind_until_success(&self) -> SockAddr {
        loop {
            let sock_path_suffix = SOCKETPAIR_NUM.fetch_add(1, Ordering::SeqCst);
            let sock_path = format!("{}{}", SOCK_PATH_PREFIX, sock_path_suffix);
            let addr_un = UnixAddr::new(&sock_path);
            if addr_un.is_err() {
                continue;
            }

            let sock_addr = SockAddr::UnixSocket(addr_un.unwrap());
            if self.bind(sock_addr).is_ok() {
                return sock_addr;
            }
        }
    }

    pub fn is_connected(&self) -> bool {
        self.channel.lock().unwrap().is_some()
    }

    pub fn is_blocking(&self) -> bool {
        self.is_blocking.load(Ordering::SeqCst)
    }

    pub fn set_non_blocking(&self) {
        self.is_blocking.store(false, Ordering::SeqCst);
        let channel = self.channel.lock().unwrap();
        channel.as_ref().map(|c| c.set_non_blocking());
    }

    pub fn set_blocking(&self) {
        self.is_blocking.store(true, Ordering::SeqCst);
        let channel = self.channel.lock().unwrap();
        channel.as_ref().map(|c| c.set_blocking());
    }

    pub fn get_sockname(
        &self,
        addr: *mut libc::sockaddr,
        addr_len: *mut libc::socklen_t,
    ) -> Result<()> {
        if let Some(str) = self.path() {
            let mut dst = unsafe {
                std::slice::from_raw_parts_mut(addr as *mut _ as *mut u8, *addr_len as usize)
            };
            let unix = UnixAddr::new(&str)?;
            let addr = SockAddr::UnixSocket(unix);
            addr.copy_to_slice(dst);
            unsafe {
                *addr_len = unix.len() as u32;
            }
        }
        Ok(())
    }
}

impl Debug for StreamUnixSocket {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("StreamUnixSocket")
            .field("path", &self.path())
            .finish()
    }
}

impl Drop for StreamUnixSocket {
    fn drop(&mut self) {
        if let Some(ref server) = *self.server.read().unwrap() {
            UnixSocketServer::remove_server(server.path());
        }
    }
}

pub struct UnixSocketServer {
    path: String,
    pending_connections: SgxMutex<VecDeque<StreamUnixSocket>>,
}

impl UnixSocketServer {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            pending_connections: SgxMutex::new(VecDeque::new()),
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn push_pending(&self, stream_socket: StreamUnixSocket) {
        let mut queue = self.pending_connections.lock().unwrap();
        queue.push_back(stream_socket);
    }

    pub fn pop_pending(&self) -> Option<StreamUnixSocket> {
        let mut queue = self.pending_connections.lock().unwrap();
        queue.pop_front()
    }

    pub fn get_server(path: &str) -> Option<Arc<Self>> {
        let mut servers = UNIX_SOCKET_SERVERS.lock().unwrap();
        servers.get(path).map(|obj| obj.clone())
    }

    pub fn create_server(path: &str) -> Result<Arc<Self>> {
        let mut servers = UNIX_SOCKET_SERVERS.lock().unwrap();
        if servers.contains_key(path) {
            return_errno!(EADDRINUSE, "the path is already listened");
        }

        let server = Arc::new(Self {
            path: path.to_string(),
            pending_connections: Mutex::new(VecDeque::new()),
        });
        servers.insert(path.to_string(), server.clone());
        Ok(server)
    }

    pub fn remove_server(path: &str) {
        let mut paths = UNIX_SOCKET_SERVERS.lock().unwrap();
        paths.remove(path);
    }
}

// One end of the connected sockets
struct EndPoint {
    name: RwLock<Option<String>>,
    reader: SgxMutex<RingBufReader>,
    writer: SgxMutex<RingBufWriter>,
    peer: Weak<Self>,
}

impl EndPoint {
    pub fn new_duplex_channel() -> Result<(Arc<Self>, Arc<Self>)> {
        let (reader_a, writer_a) = ring_buffer(DEFAULT_BUF_SIZE)?;
        let (reader_b, writer_b) = ring_buffer(DEFAULT_BUF_SIZE)?;
        let mut end_a = Arc::new(Self {
            name: RwLock::new(None),
            reader: SgxMutex::new(reader_a),
            writer: SgxMutex::new(writer_b),
            peer: Weak::default(),
        });
        let end_b = Arc::new(Self {
            name: RwLock::new(None),
            reader: SgxMutex::new(reader_b),
            writer: SgxMutex::new(writer_a),
            peer: Arc::downgrade(&end_a),
        });

        // Only end_b which will not change end_a references end_a
        unsafe {
            Arc::get_mut_unchecked(&mut end_a).peer = Arc::downgrade(&end_b);
        }

        Ok((end_a, end_b))
    }

    pub fn set_name(&self, name: &str) {
        *self.name.write().unwrap() = Some(name.to_string());
    }

    pub fn peer_name(&self) -> Option<String> {
        self.peer
            .upgrade()
            .map(|end| end.name.read().unwrap().clone())
            .flatten()
    }

    pub fn set_non_blocking(&self) {
        self.reader.lock().unwrap().set_non_blocking();
        self.writer.lock().unwrap().set_non_blocking();
    }

    pub fn set_blocking(&self) {
        self.reader.lock().unwrap().set_blocking();
        self.writer.lock().unwrap().set_blocking();
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        self.reader.lock().unwrap().read_from_buffer(buf)
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize> {
        self.writer.lock().unwrap().write_to_buffer(buf)
    }

    pub fn readv(&self, bufs: &mut [&mut [u8]]) -> Result<usize> {
        self.reader.lock().unwrap().read_from_vector(bufs)
    }

    pub fn writev(&self, bufs: &[&[u8]]) -> Result<usize> {
        self.writer.lock().unwrap().write_to_vector(bufs)
    }

    pub fn bytes_to_read(&self) -> usize {
        self.reader.lock().unwrap().bytes_to_read()
    }

    pub fn poll(&self) -> Result<PollEventFlags> {
        let reader = self.reader.lock().unwrap();
        let writer = self.writer.lock().unwrap();
        let readable = reader.can_read() && !reader.is_peer_closed();
        let writable = writer.can_write() && !writer.is_peer_closed();
        let events = if readable ^ writable {
            if reader.can_read() {
                PollEventFlags::POLLRDHUP | PollEventFlags::POLLIN | PollEventFlags::POLLRDNORM
            } else {
                PollEventFlags::POLLRDHUP
            }
        // both readable and writable
        } else if readable {
            PollEventFlags::POLLIN
                | PollEventFlags::POLLOUT
                | PollEventFlags::POLLRDNORM
                | PollEventFlags::POLLWRNORM
        } else {
            PollEventFlags::POLLHUP
        };
        Ok(events)
    }
}

// TODO: Add SO_SNDBUF and SO_RCVBUF to set/getsockopt to dynamcally change the size.
// This value is got from /proc/sys/net/core/rmem_max and wmem_max that are same on linux.
pub const DEFAULT_BUF_SIZE: usize = 208 * 1024;

lazy_static! {
    static ref UNIX_SOCKET_SERVERS: Mutex<BTreeMap<String, Arc<UnixSocketServer>>> =
        Mutex::new(BTreeMap::new());
}
