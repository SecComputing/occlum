use super::*;
use crate::fs::{AccessMode, File, FileRef, IoctlCmd, StatusFlags};
use crate::util::ring_buf::{ring_buffer, RingBufReader, RingBufWriter};
use rcore_fs::vfs::{FileType, Metadata, Timespec};
use std::any::Any;
use std::collections::btree_map::BTreeMap;
use std::fmt;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::atomic::{spin_loop_hint, AtomicBool, AtomicUsize, Ordering};

/// Path-based cross-worlds socket.
///
/// UnixSocket contain two kinds of unix socket: one that resides only in libos and
/// communicates with the unix socket inside libos; the other communicates with the
/// unix socket in the host. Which socket is used when UnixSocket is called depends on
/// where the path is from. If the path is from host, UnixSocket operates on host.
/// Otherwise, it operates in libos. For example, when UnixSocket calls connect function,
/// it connects to the source of the SockAddr (host or libos). Users can specify the
/// host paths in Occlum.json. By default, UnixSocket operates only inside libos if
/// no host paths are provided.
///
pub struct UnixSocket {
    // Unix socket in libos. Only stream type socket is supported.
    // More types, e.g., datagram and packet, will be supported in the future.
    libos_sock: RwLock<Option<StreamUnixSocket>>,
    // Unix socket that is implemented through ocall to Berkeley socket API in host.
    host_sock: RwLock<Option<SocketFile>>,
    source: RwLock<Path>,
    socket_type: SocketType,
}

unsafe impl Send for UnixSocket {}
unsafe impl Sync for UnixSocket {}

#[derive(Debug, Copy, Clone)]
enum Path {
    Unknown,
    Host,
    Libos,
}

impl File for UnixSocket {
    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        self.recvfrom(buf, RecvFlags::empty(), None)
            .map(|(len, _)| len)
    }

    fn write(&self, buf: &[u8]) -> Result<usize> {
        self.sendto(buf, SendFlags::empty(), None)
    }

    fn writev(&self, bufs: &[&[u8]]) -> Result<usize> {
        // Writev to libos sock first. If it fails, writev to host.
        // It may raise the concern about risks to send libos data to host.
        // The above risk only exits in the situation where libos sock
        // is not properly used.
        let libos_sock = self.libos_sock.read().unwrap();
        let ret = libos_sock.as_ref().map(|s| s.writev(bufs));
        if let Some(Ok(_)) = ret {
            ret.unwrap()
        } else if HOST_UNIX_ADDRS.is_empty() {
            ret.unwrap()
        } else {
            debug!("libos ret is {:?}", ret);
            let host_sock = self.host_sock.read().unwrap();
            host_sock.as_ref().unwrap().writev(bufs)
        }
    }

    fn readv(&self, bufs: &mut [&mut [u8]]) -> Result<usize> {
        let libos_sock = self.libos_sock.read().unwrap();
        let ret = libos_sock.as_ref().map(|s| s.readv(bufs));
        if let Some(Ok(_)) = ret {
            ret.unwrap()
        } else if HOST_UNIX_ADDRS.is_empty() {
            ret.unwrap()
        } else {
            debug!("libos ret is {:?}", ret);
            self.host_sock.read().unwrap().as_ref().unwrap().readv(bufs)
        }
    }

    // Seeking, or calling pread(2) or pwrite(2) with a nonzero position is not supported on sockets.
    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize> {
        if offset != 0 {
            return_errno!(ESPIPE, "socket does not support seek");
        } else {
            self.read(buf)
        }
    }

    fn write_at(&self, offset: usize, buf: &[u8]) -> Result<usize> {
        if offset != 0 {
            return_errno!(ESPIPE, "socket does not support seek");
        } else {
            self.write(buf)
        }
    }

    fn metadata(&self) -> Result<Metadata> {
        Ok(Metadata {
            dev: 0,
            inode: 0,
            size: 0,
            blk_size: 0,
            blocks: 0,
            atime: Timespec { sec: 0, nsec: 0 },
            mtime: Timespec { sec: 0, nsec: 0 },
            ctime: Timespec { sec: 0, nsec: 0 },
            type_: FileType::Socket,
            mode: 0,
            nlinks: 0,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }

    fn ioctl(&self, cmd: &mut IoctlCmd) -> Result<i32> {
        match self.source() {
            Path::Unknown => {
                if !HOST_UNIX_ADDRS.is_empty() {
                    if let Some(sock) = self.libos_sock.read().unwrap().as_ref() {
                        sock.ioctl(cmd)?;
                    }
                    // TODO: restore cmd and check the returned cmd
                    self.host_sock.read().unwrap().as_ref().unwrap().ioctl(cmd)
                } else {
                    self.libos_sock.read().unwrap().as_ref().unwrap().ioctl(cmd)
                }
            }
            Path::Libos => self.libos_sock.read().unwrap().as_ref().unwrap().ioctl(cmd),
            Path::Host => self.host_sock.read().unwrap().as_ref().unwrap().ioctl(cmd),
        }
    }

    fn get_access_mode(&self) -> Result<AccessMode> {
        Ok(AccessMode::O_RDWR)
    }

    fn get_status_flags(&self) -> Result<StatusFlags> {
        match self.source() {
            Path::Unknown => {
                if !HOST_UNIX_ADDRS.is_empty() {
                    if let Some(sock) = self.libos_sock.read().unwrap().as_ref() {
                        sock.get_status_flags()?;
                    }
                    self.host_sock
                        .read()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .get_status_flags()
                } else {
                    self.libos_sock
                        .read()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .get_status_flags()
                }
            }
            Path::Libos => self
                .libos_sock
                .read()
                .unwrap()
                .as_ref()
                .unwrap()
                .get_status_flags(),
            Path::Host => self
                .host_sock
                .read()
                .unwrap()
                .as_ref()
                .unwrap()
                .get_status_flags(),
        }
    }

    fn set_status_flags(&self, new_status_flags: StatusFlags) -> Result<()> {
        match self.source() {
            Path::Unknown => {
                if !HOST_UNIX_ADDRS.is_empty() {
                    if let Some(sock) = self.libos_sock.read().unwrap().as_ref() {
                        sock.set_status_flags(new_status_flags)?;
                    }
                    self.host_sock
                        .read()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .set_status_flags(new_status_flags)
                } else {
                    self.libos_sock
                        .read()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .set_status_flags(new_status_flags)
                }
            }
            Path::Libos => self
                .libos_sock
                .read()
                .unwrap()
                .as_ref()
                .unwrap()
                .set_status_flags(new_status_flags),
            Path::Host => self
                .host_sock
                .read()
                .unwrap()
                .as_ref()
                .unwrap()
                .set_status_flags(new_status_flags),
        }
    }

    fn seek(&self, pos: SeekFrom) -> Result<off_t> {
        return_errno!(ESPIPE, "Socket does not support seek")
    }

    fn poll(&self) -> Result<PollEventFlags> {
        let mut libos_call = false;

        match self.source() {
            Path::Unknown => libos_call = self.libos_sock.read().unwrap().is_some(),
            Path::Libos => libos_call = true,
            Path::Host => {}
        }

        if libos_call {
            self.libos_sock.read().unwrap().as_ref().unwrap().poll()
        } else {
            self.host_sock.read().unwrap().as_ref().unwrap().poll()
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Socket for UnixSocket {
    fn bind(&self, path: SockAddr) -> Result<()> {
        if path.is_from_host() {
            let host_sock = self.host_sock.read().unwrap();
            host_sock.as_ref().unwrap().bind(path)?;
            *self.source.write().unwrap() = Path::Host;
        } else {
            let libos_sock = self.libos_sock.read().unwrap();
            libos_sock.as_ref().unwrap().bind(path)?;
            *self.source.write().unwrap() = Path::Libos;
        }
        Ok(())
    }

    fn listen(&self, backlog: i32) -> Result<()> {
        match self.source() {
            Path::Unknown => {
                return_errno!(EINVAL, "Socket is not bound");
            }
            Path::Libos => {
                let libos_sock = self.libos_sock.read().unwrap();
                libos_sock.as_ref().unwrap().listen(backlog)
            }
            Path::Host => {
                let host_sock = self.host_sock.read().unwrap();
                host_sock.as_ref().unwrap().listen(backlog)
            }
        }
    }

    fn accept(&self, flags: FileFlags, addr: Option<&mut [u8]>) -> Result<(Self, usize)> {
        let socket_type = self.socket_type();
        match self.source() {
            Path::Unknown => {
                return_errno!(EINVAL, "Socket is not listening for connections");
            }
            Path::Libos => {
                let libos_sock = self.libos_sock.read().unwrap();
                let (unix_socket, ret_addr_len) =
                    libos_sock.as_ref().unwrap().accept(flags, addr)?;
                return Ok((
                    Self {
                        libos_sock: RwLock::new(Some(unix_socket)),
                        host_sock: RwLock::new(Some(SocketFile::new(
                            ProtocolFamily::PF_LOCAL,
                            socket_type,
                            flags,
                            0,
                        )?)),
                        source: RwLock::new(Path::Libos),
                        socket_type: socket_type,
                    },
                    ret_addr_len,
                ));
            }
            Path::Host => {
                let host_sock = self.host_sock.read().unwrap();
                let (socket_file, ret_addr_len) =
                    host_sock.as_ref().unwrap().accept(flags, addr)?;
                return Ok((
                    Self {
                        libos_sock: RwLock::new(if socket_type == SocketType::SOCK_STREAM {
                            Some(StreamUnixSocket::new(flags)?)
                        } else {
                            None
                        }),
                        host_sock: RwLock::new(Some(socket_file)),
                        source: RwLock::new(Path::Host),
                        socket_type: socket_type,
                    },
                    ret_addr_len,
                ));
            }
        }
    }

    fn connect(&self, addr: Option<SockAddr>) -> Result<()> {
        let mut host_call = false;
        let mut libos_call = false;

        if addr.is_none() {
            libos_call = self.libos_sock.read().unwrap().is_some();
            // It will not fail to connect to a newly created socket with null addr
            host_call = !HOST_UNIX_ADDRS.is_empty();
        } else {
            host_call = addr.unwrap().is_from_host();
            libos_call = !host_call;
        }

        debug!(
            "addr {:?} host_call {} and libos_call {}",
            addr, host_call, libos_call
        );

        if host_call {
            let host_sock = self.host_sock.read().unwrap();
            host_sock.as_ref().unwrap().connect(addr)?;
        }

        if libos_call {
            let libos_sock = self.libos_sock.read().unwrap();
            libos_sock.as_ref().unwrap().connect(addr)?;
        }

        Ok(())
    }

    fn sendto(&self, buf: &[u8], flags: SendFlags, addr: Option<SockAddr>) -> Result<usize> {
        if addr.is_none() {
            let libos_sock = self.libos_sock.read().unwrap();
            let ret = libos_sock.as_ref().map(|s| s.sendto(buf, flags, addr));
            if let Some(Ok(_)) = ret {
                ret.unwrap()
            } else if HOST_UNIX_ADDRS.is_empty() {
                ret.unwrap()
            } else {
                debug!("sendto in libos error is {:?}", ret);
                drop(libos_sock);
                let host_sock = self.host_sock.read().unwrap();
                host_sock.as_ref().unwrap().sendto(buf, flags, addr)
            }
        } else {
            if addr.unwrap().is_from_host() {
                let host_sock = self.host_sock.read().unwrap();
                host_sock.as_ref().unwrap().sendto(buf, flags, addr)
            } else {
                let libos_sock = self.libos_sock.read().unwrap();
                libos_sock.as_ref().unwrap().sendto(buf, flags, addr)
            }
        }
    }

    fn recvfrom(
        &self,
        buf: &mut [u8],
        flags: RecvFlags,
        addr: Option<&mut [u8]>,
    ) -> Result<(usize, usize)> {
        let mut tmp_addr = if let Some(slice) = &addr {
            Some(vec![0; slice.len()])
        } else {
            None
        };

        let libos_sock = self.libos_sock.read().unwrap();
        let ret = libos_sock
            .as_ref()
            .map(|s| s.recvfrom(buf, flags, tmp_addr.as_mut().map(|vec| vec.as_mut_slice())));
        if let Some(Ok(_)) = ret {
            if let Some(src) = tmp_addr {
                addr.unwrap().copy_from_slice(&src);
            }
            ret.unwrap()
        } else if HOST_UNIX_ADDRS.is_empty() {
            ret.unwrap()
        } else {
            drop(libos_sock);
            debug!("recvfrom in libos error: {:?}", ret);
            let host_sock = self.host_sock.read().unwrap();
            host_sock.as_ref().unwrap().recvfrom(buf, flags, addr)
        }
    }
}

impl UnixSocket {
    pub fn new(socket_type: SocketType, flags: FileFlags, protocol: i32) -> Result<Self> {
        if protocol != 0 && protocol != ProtocolFamily::PF_LOCAL as i32 {
            return_errno!(EPROTONOSUPPORT, "protocol is not supported");
        }

        let libos_sock = if socket_type == SocketType::SOCK_STREAM {
            Some(StreamUnixSocket::new(flags)?)
        } else {
            None
        };

        let host_sock = if !HOST_UNIX_ADDRS.is_empty() {
            Some(SocketFile::new(
                ProtocolFamily::PF_LOCAL,
                socket_type,
                flags,
                protocol,
            )?)
        } else {
            None
        };

        if libos_sock.is_none() && host_sock.is_none() {
            return_errno!(EPROTONOSUPPORT, "protocol is not supported");
        }

        Ok(Self {
            libos_sock: RwLock::new(libos_sock),
            host_sock: RwLock::new(host_sock),
            source: RwLock::new(Path::Unknown),
            socket_type: socket_type,
        })
    }

    fn source(&self) -> Path {
        *self.source.read().unwrap()
    }

    fn socket_type(&self) -> SocketType {
        self.socket_type
    }

    // Only return socket pair in libos.
    pub fn socketpair(socket_type: SocketType, flags: FileFlags) -> Result<(Self, Self)> {
        if socket_type != SocketType::SOCK_STREAM {
            return_errno!(EOPNOTSUPP, "socket type is not supported");
        }

        let (libos_sock_a, libos_sock_b) = StreamUnixSocket::socketpair(flags)?;

        Ok((
            Self {
                libos_sock: RwLock::new(Some(libos_sock_a)),
                host_sock: RwLock::new(None),
                source: RwLock::new(Path::Libos),
                socket_type: socket_type,
            },
            Self {
                libos_sock: RwLock::new(Some(libos_sock_b)),
                host_sock: RwLock::new(None),
                source: RwLock::new(Path::Libos),
                socket_type: socket_type,
            },
        ))
    }
}

impl Debug for UnixSocket {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("StreamUnixSocket")
            .field("libos_sock", &*self.libos_sock.read().unwrap())
            .field("host_sock", &*self.libos_sock.read().unwrap())
            .field("source", &self.source)
            .field("socket_type", &self.socket_type)
            .finish()
    }
}

pub trait UnixSocketType {
    fn as_unix_socket(&self) -> Result<&UnixSocket>;
}

impl UnixSocketType for FileRef {
    fn as_unix_socket(&self) -> Result<&UnixSocket> {
        self.as_any()
            .downcast_ref::<UnixSocket>()
            .ok_or_else(|| errno!(EBADF, "not a unix socket"))
    }
}
