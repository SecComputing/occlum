use super::*;

// The trait contains the network syscall functions. It applies to all the socket types.
// SocketFile has the same functions but are not in the Socket trait form.
// Addtional work is needed to put the functions in the trait and we leave it for future work.
// Also left for future work are the missing syscall functions.
pub trait Socket {
    fn bind(&self, addr: SockAddr) -> Result<()>;
    fn listen(&self, backlog: i32) -> Result<()>;
    fn accept(&self, flags: FileFlags, addr: Option<&mut [u8]>) -> Result<(Self, usize)>
    where
        Self: Sized;
    // None stands for sockaddr whose sa_family member is set to AF_UNSPEC or null address
    fn connect(&self, addr: Option<SockAddr>) -> Result<()>;
    fn sendto(&self, buf: &[u8], flags: SendFlags, addr: Option<SockAddr>) -> Result<usize>;
    fn recvfrom(
        &self,
        buf: &mut [u8],
        flags: RecvFlags,
        addr: Option<&mut [u8]>,
    ) -> Result<(usize, usize)>;
}
