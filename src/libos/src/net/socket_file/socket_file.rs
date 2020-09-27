use super::*;

use crate::fs::{
    occlum_ocall_ioctl, AccessMode, CreationFlags, File, FileRef, IoctlCmd, StatusFlags,
};
use std::any::Any;
use std::io::{Read, Seek, SeekFrom, Write};

/// Native Linux socket
#[derive(Debug)]
pub struct SocketFile {
    host_fd: c_int,
}

impl SocketFile {
    pub fn new(
        domain: ProtocolFamily,
        socket_type: SocketType,
        file_flags: FileFlags,
        protocol: i32,
    ) -> Result<Self> {
        let ret = try_libc!(libc::ocall::socket(
            domain as i32,
            socket_type as i32 | file_flags.bits(),
            protocol
        ));
        Ok(Self { host_fd: ret })
    }

    pub fn get_sockname(
        &self,
        addr: *mut libc::sockaddr,
        addr_len: *mut libc::socklen_t,
    ) -> Result<()> {
        try_libc!(libc::ocall::getsockname(self.host_fd(), addr, addr_len));
        Ok(())
    }

    pub fn shutdown(&self, how: c_int) -> Result<()> {
        try_libc!(libc::ocall::shutdown(self.host_fd(), how));
        Ok(())
    }
    pub fn bind(&self, addr: SockAddr) -> Result<()> {
        let (addr_ptr, addr_len) = addr.as_ptr_and_len();

        let ret = try_libc!(libc::ocall::bind(
            self.host_fd(),
            addr_ptr as *const libc::sockaddr,
            addr_len as u32
        ));
        Ok(())
    }

    pub fn listen(&self, backlog: i32) -> Result<()> {
        let ret = try_libc!(libc::ocall::listen(self.host_fd(), backlog));
        Ok(())
    }

    pub fn accept(&self, flags: FileFlags, addr: Option<&mut [u8]>) -> Result<(Self, usize)> {
        let mut len = 0;
        let addr_len_ptr = if let Some(addr_buffer) = &addr {
            len = addr_buffer.len() as libc::socklen_t;
            &mut len as *mut libc::socklen_t
        } else {
            std::ptr::null_mut()
        };

        let untrusted_addr: &mut [u8] = &mut vec![0; len as usize];
        let addr_ptr = if len != 0 {
            untrusted_addr.as_mut_ptr() as *mut libc::sockaddr
        } else {
            std::ptr::null_mut()
        };

        let ret = try_libc!(libc::ocall::accept4(
            self.host_fd(),
            addr_ptr,
            addr_len_ptr,
            flags.bits()
        ));

        if let Some(dst) = addr {
            let copy_len = std::cmp::min(len as usize, dst.len());
            dst[..copy_len].copy_from_slice(&untrusted_addr[0..copy_len]);
        }

        Ok((Self { host_fd: ret }, len as usize))
    }

    pub fn connect(&self, addr: Option<SockAddr>) -> Result<()> {
        debug!("host_fd: {} addr {:?}", self.host_fd(), addr);
        // used to dissolve addr association
        let unspec_addr = libc::sockaddr {
            sa_family: 0,
            sa_data: [0; 14],
        };

        let (addr_ptr, addr_len) = if let Some(ref addr_in) = addr {
            addr_in.as_ptr_and_len()
        } else {
            (
                &unspec_addr as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr>(),
            )
        };

        let ret = try_libc!(libc::ocall::connect(
            self.host_fd(),
            addr_ptr,
            addr_len as u32
        ));
        Ok(())
    }

    pub fn host_fd(&self) -> c_int {
        self.host_fd
    }
}

impl Drop for SocketFile {
    fn drop(&mut self) {
        let ret = unsafe { libc::ocall::close(self.host_fd) };
        assert!(ret == 0);
    }
}

//TODO: refactor write syscall to allow zero length with non-zero buffer
impl File for SocketFile {
    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        self.recv(buf, RecvFlags::empty())
    }

    fn write(&self, buf: &[u8]) -> Result<usize> {
        self.send(buf, SendFlags::empty())
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

    // TODO: use sendmsg to impl readv
    fn readv(&self, bufs: &mut [&mut [u8]]) -> Result<usize> {
        let mut total_len = 0;
        for buf in bufs {
            match self.read(buf) {
                Ok(len) => {
                    total_len += len;
                }
                Err(_) if total_len != 0 => break,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(total_len)
    }

    fn writev(&self, bufs: &[&[u8]]) -> Result<usize> {
        let mut total_len = 0;
        for buf in bufs {
            match self.write(buf) {
                Ok(len) => {
                    total_len += len;
                }
                Err(_) if total_len != 0 => break,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(total_len)
    }

    fn seek(&self, pos: SeekFrom) -> Result<off_t> {
        return_errno!(ESPIPE, "Socket does not support seek")
    }

    fn ioctl(&self, cmd: &mut IoctlCmd) -> Result<i32> {
        let cmd_num = cmd.cmd_num() as c_int;
        let cmd_arg_ptr = cmd.arg_ptr() as *mut c_void;
        let ret = try_libc!({
            let mut retval: i32 = 0;
            let status = occlum_ocall_ioctl(
                &mut retval as *mut i32,
                self.host_fd(),
                cmd_num,
                cmd_arg_ptr,
                cmd.arg_len(),
            );
            assert!(status == sgx_status_t::SGX_SUCCESS);
            retval
        });
        // FIXME: add sanity checks for results returned for socket-related ioctls
        cmd.validate_arg_and_ret_vals(ret)?;
        Ok(ret)
    }

    fn get_access_mode(&self) -> Result<AccessMode> {
        Ok(AccessMode::O_RDWR)
    }

    fn get_status_flags(&self) -> Result<StatusFlags> {
        let ret = try_libc!(libc::ocall::fcntl_arg0(self.host_fd(), libc::F_GETFL));
        Ok(StatusFlags::from_bits_truncate(ret as u32))
    }

    fn set_status_flags(&self, new_status_flags: StatusFlags) -> Result<()> {
        let valid_flags_mask = StatusFlags::O_APPEND
            | StatusFlags::O_ASYNC
            | StatusFlags::O_DIRECT
            | StatusFlags::O_NOATIME
            | StatusFlags::O_NONBLOCK;
        let raw_status_flags = (new_status_flags & valid_flags_mask).bits();
        try_libc!(libc::ocall::fcntl_arg1(
            self.host_fd(),
            libc::F_SETFL,
            raw_status_flags as c_int
        ));
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub trait SocketFileType {
    fn as_socket(&self) -> Result<&SocketFile>;
}

impl SocketFileType for FileRef {
    fn as_socket(&self) -> Result<&SocketFile> {
        self.as_any()
            .downcast_ref::<SocketFile>()
            .ok_or_else(|| errno!(EBADF, "not a socket file"))
    }
}
