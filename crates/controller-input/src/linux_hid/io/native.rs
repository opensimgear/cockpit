use super::*;
pub(super) struct Native;
impl Calls for Native {
    fn ioctl(&self, fd: i32, request: u32, bytes: &mut [u8]) -> std::io::Result<usize> {
        let result = unsafe { libc::ioctl(fd, request as _, bytes.as_mut_ptr()) };
        if result < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(result as usize)
        }
    }
    fn poll(&self, fd: i32, timeout: i32) -> std::io::Result<i16> {
        let mut descriptor = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut descriptor, 1, timeout) };
        if result < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(descriptor.revents)
        }
    }
    fn read(&self, fd: i32, bytes: &mut [u8]) -> std::io::Result<usize> {
        let result = unsafe { libc::read(fd, bytes.as_mut_ptr().cast(), bytes.len()) };
        if result < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(result as usize)
        }
    }
}
