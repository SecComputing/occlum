use super::*;

impl SocketFile {
    pub fn send(&self, buf: &[u8], flags: SendFlags) -> Result<usize> {
        self.sendto(buf, flags, None)
    }

    // TODO: use sendmsg to impl sendto
    pub fn sendto(&self, buf: &[u8], flags: SendFlags, addr: Option<SockAddr>) -> Result<usize> {
        let (addr_ptr, addr_len) = if let Some(addr_in) = addr {
            addr_in.as_ptr_and_len()
        } else {
            (std::ptr::null(), 0)
        };

        let ret = try_libc!(libc::ocall::sendto(
            self.host_fd(),
            buf.as_ptr() as *mut c_void,
            buf.len(),
            flags.bits(),
            addr_ptr as *const _,
            addr_len as libc::socklen_t
        ));
        Ok(ret as usize)
    }

    pub fn sendmsg<'a, 'b>(&self, msg: &'b MsgHdr<'a>, flags: SendFlags) -> Result<usize> {
        // Copy message's iovecs into untrusted iovecs
        let msg_iov = msg.get_iovs();
        let u_slice_alloc = UntrustedSliceAlloc::new(msg_iov.total_bytes())?;
        let u_slices = msg_iov
            .as_slices()
            .iter()
            .map(|src_slice| {
                u_slice_alloc
                    .new_slice(src_slice)
                    .expect("unexpected out of memory")
            })
            .collect();
        let u_iovs = Iovs::new(u_slices);

        self.do_sendmsg(u_iovs.as_slices(), flags, msg.get_name(), msg.get_control())
    }

    fn do_sendmsg(
        &self,
        data: &[&[u8]],
        flags: SendFlags,
        name: Option<&[u8]>,
        control: Option<&[u8]>,
    ) -> Result<usize> {
        // Prepare the arguments for OCall
        let mut retval: isize = 0;
        // Host socket fd
        let host_fd = self.host_fd();
        // Name
        let (msg_name, msg_namelen) = name.as_ptr_and_len();
        let msg_name = msg_name as *const c_void;
        // Iovs
        let raw_iovs: Vec<libc::iovec> = data.iter().map(|slice| slice.as_libc_iovec()).collect();
        let (msg_iov, msg_iovlen) = raw_iovs.as_slice().as_ptr_and_len();
        // Control
        let (msg_control, msg_controllen) = control.as_ptr_and_len();
        let msg_control = msg_control as *const c_void;
        // Flags
        let flags = flags.bits();

        let bytes_sent = try_libc!({
            // Do OCall
            let status = occlum_ocall_sendmsg(
                &mut retval as *mut isize,
                host_fd,
                msg_name,
                msg_namelen as u32,
                msg_iov,
                msg_iovlen,
                msg_control,
                msg_controllen,
                flags,
            );
            assert!(status == sgx_status_t::SGX_SUCCESS);

            retval
        });
        debug_assert!(bytes_sent >= 0);
        Ok(bytes_sent as usize)
    }
}

extern "C" {
    fn occlum_ocall_sendmsg(
        ret: *mut ssize_t,
        fd: c_int,
        msg_name: *const c_void,
        msg_namelen: libc::socklen_t,
        msg_data: *const libc::iovec,
        msg_datalen: size_t,
        msg_control: *const c_void,
        msg_controllen: size_t,
        flags: c_int,
    ) -> sgx_status_t;
}
