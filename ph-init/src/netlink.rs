use std::cell::Cell;
use std::convert::TryInto;
use std::ffi::CString;
use std::net::Ipv4Addr;
use std::{mem, result, fmt, io};
use std::os::unix::io::RawFd;
use std::path::Path;

use libc::{
    PF_NETLINK, SOCK_RAW, SOCK_CLOEXEC, SOCK_NONBLOCK
};

const NETLINK_ROUTE: i32 = 0;

const IFLA_IFNAME: u16 = 3;
const IFLA_MASTER: u16 = 10;
const IFLA_LINKINFO: u16 = 18;
const IFLA_INFO_KIND: u16 = 1;

const NLA_F_NESTED: u16 = 1 << 15;
const IFA_ADDRESS: u16 = 1;
const IFA_LOCAL: u16 = 2;

const NLMSG_ERROR: u16 = 2;

pub const NLM_F_REQUEST: u16 = 1;
pub const NLM_F_ACK: u16 = 4;
pub const NLM_F_EXCL: u16 = 512;
pub const NLM_F_CREATE: u16 = 1024;

pub const RTM_NEWLINK: u16 = 16;
pub const RTM_SETLINK: u16 = 19;
pub const RTM_NEWADDR: u16 = 20;

pub const AF_UNSPEC: u8 = 0;
pub const AF_INET: u8 = 2;

const NL_HDRLEN: usize = 16;
const ATTR_HDRLEN: usize = 4;

const RTM_NEWROUTE: u16 = 24;

const RT_TABLE_MAIN: u8 = 254;
const RT_SCOPE_UNIVERSE: u8 = 0;
const RTPROT_BOOT: u8 = 3;
const RTN_UNICAST: u8 = 1;

const RTA_GATEWAY: u16 = 5;


const MESSAGE_ALIGN: usize = 4;

pub const IFF_UP: u32 = libc::IFF_UP as u32;

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Socket(io::Error),
    SocketBind(io::Error),
    SocketSend(io::Error),
    SocketRecv(io::Error),
    NameToIndex(io::Error),
    ErrorResponse(io::Error),
    UnexpectedResponse,
    ShortSend,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Error::*;
        match self {
            Socket(e) => write!(f, "failed to create netlink socket: {}", e),
            SocketBind(e) => write!(f, "failed calling bind() on netlink socket: {}", e),
            SocketSend(e) => write!(f, "failed calling sendto() on netlink socket: {}", e),
            SocketRecv(e) => write!(f, "failed calling recv() on netlink socket: {}", e),
            NameToIndex(e) => write!(f, "failed to convert interface name to index: {}", e),
            ErrorResponse(e) => write!(f, "error response to netlink request: {}", e),
            UnexpectedResponse => write!(f, "could not parse response message from netlink"),
            ShortSend => write!(f, "failed to transmit entire netlink message"),
        }
    }
}

pub struct NetlinkSocket {
    sock: RawFd,
    seq: Cell<u32>,
}


impl NetlinkSocket {
    pub fn open() -> Result<NetlinkSocket> {
        Self::open_protocol(NETLINK_ROUTE)
    }

    #[allow(dead_code)]
    pub fn add_default_route(&self, gateway: Ipv4Addr) -> Result<()> {
        let msg = self.message_create(RTM_NEWROUTE)
            .with_rtmsg(AF_INET, |hdr| {
                hdr.table(RT_TABLE_MAIN)
                    .scope(RT_SCOPE_UNIVERSE)
                    .protocol(RTPROT_BOOT)
                    .rtype(RTN_UNICAST);
            })
            .append_attr(RTA_GATEWAY, &gateway.octets())
            .done();

        self.send_message(msg)
    }

    #[allow(dead_code)]
    pub fn add_interface_to_bridge(&self, iface: &str, bridge: &str) -> Result<()> {
        let bridge_idx = self.name_to_index(bridge)?;
        let msg = self.message(RTM_SETLINK)
            .with_ifinfomsg(AF_UNSPEC, |hdr| {
                hdr.set_flags(IFF_UP)
                    .set_change(IFF_UP);
            })
            .attr_u32(IFLA_MASTER, bridge_idx)
            .attr_str(IFLA_IFNAME, iface)
            .done();

        self.send_message(msg)
    }

    #[allow(dead_code)]
    pub fn create_bridge(&self, name: &str) -> Result<()> {
        let msg =  self.message_create(RTM_NEWLINK)
            .ifinfomsg(AF_UNSPEC)
            .attr_str(IFLA_IFNAME, name)
            .with_nested(IFLA_LINKINFO, |a| {
                a.attr_str(IFLA_INFO_KIND, "bridge")
                    .align();
            })
            .done();

        self.send_message(msg)
    }

    #[allow(dead_code)]
    pub fn set_interface_up(&self, iface: &str) -> Result<()> {
        let idx = self.name_to_index(iface)?;
        let msg = self.message(RTM_NEWLINK)
            .with_ifinfomsg(AF_UNSPEC, |hdr| {
                hdr.set_flags(IFF_UP)
                    .set_change(IFF_UP)
                    .index(idx);
            })
            .done();

        self.send_message(msg)
    }

    #[allow(dead_code)]
    pub fn add_ip_address(&self, iface: &str, ip: Ipv4Addr, netmask_bits: u32) -> Result<()> {
        let idx = self.name_to_index(iface)?;
        let msg = self.message_create(RTM_NEWADDR)
            .with_ifaddrmsg(|hdr| {
                hdr.family(AF_INET)
                    .prefixlen(netmask_bits as u8)
                    .scope(RT_SCOPE_UNIVERSE)
                    .index(idx);
            })
            .append_attr(IFA_ADDRESS, &ip.octets())
            .append_attr(IFA_LOCAL, &ip.octets())
            .done();

        self.send_message(msg)
    }

    fn open_protocol(protocol: i32) -> Result<NetlinkSocket> {
        let sock = sys_socket(PF_NETLINK,
                                SOCK_RAW | SOCK_CLOEXEC | SOCK_NONBLOCK,
                                protocol)?;

        let mut sockaddr: libc::sockaddr_nl = unsafe { mem::zeroed() };
        sockaddr.nl_family = PF_NETLINK as u16;
        let addrlen = mem::size_of::<libc::sockaddr_nl>();
        sys_bind(sock,
                 &sockaddr as *const libc::sockaddr_nl as *const libc::sockaddr,
                 addrlen)?;

        Ok(NetlinkSocket{ sock, seq: Cell::new(1) } )
    }

    fn name_to_index(&self, name: &str) -> Result<u32> {
        let name = CString::new(name).unwrap();
        let ret = unsafe { libc::if_nametoindex(name.as_ptr()) };
        if ret == 0 {
            Err(Error::NameToIndex(io::Error::last_os_error()))
        } else {
            Ok(ret as u32)
        }
    }

    pub fn interface_exists(&self, name: &str) -> bool {
        Path::new("/sys/class/net")
            .join(name)
            .exists()
    }

    fn seq(&self) -> u32 {
        let seq = self.seq.get();
        self.seq.set(seq + 1);
        seq
    }

    fn message(&self, mtype: u16) -> NetlinkMessage {
        let flags = NLM_F_REQUEST | NLM_F_ACK;
        NetlinkMessage::new(mtype, flags, self.seq())
    }

    fn message_create(&self, mtype: u16) -> NetlinkMessage {
        let flags = NLM_F_REQUEST | NLM_F_ACK | NLM_F_CREATE | NLM_F_EXCL;
        NetlinkMessage::new(mtype, flags, self.seq())
    }

    fn send_message(&self, msg: NetlinkMessage) -> Result<()> {
        self.send(msg.as_bytes())?;
        let mut recv_buffer = vec![0u8; 4096];
        let n = sys_recv(self.sock, &mut recv_buffer, 0)?;
        if n < NL_HDRLEN + 4 {
            return Err(Error::UnexpectedResponse);
        }
        recv_buffer.truncate(n);
        self.process_response(InBuffer::new(&recv_buffer))
    }

    fn process_response(&self, mut resp: InBuffer) -> Result<()> {
        resp.skip(4);
        let mtype = resp.read_u16();
        resp.skip(10);
        if mtype == NLMSG_ERROR {
            match resp.read_u32() {
                0 => Ok(()),
                errno => {
                    let e = io::Error::from_raw_os_error(errno as i32);
                    Err(Error::ErrorResponse(e))
                }
            }
        } else {
            Err(Error::UnexpectedResponse)
        }
    }

    fn send(&self, buf: &[u8]) -> Result<()> {
        let mut sockaddr: libc::sockaddr_nl = unsafe { mem::zeroed() };
        sockaddr.nl_family = PF_NETLINK as u16;
        let addrlen = mem::size_of::<libc::sockaddr_nl>();
        let n = sys_sendto(self.sock,
                           buf,
                           &sockaddr as *const libc::sockaddr_nl as *const libc::sockaddr,
                           addrlen)?;

        if n != buf.len() {
            Err(Error::ShortSend)
        } else {
            Ok(())
        }
    }
}

impl Drop for NetlinkSocket {
    fn drop(&mut self) {
        let _ = unsafe { libc::close(self.sock) };
    }
}

pub struct NetlinkMessage(Buffer<Vec<u8>>);

impl NetlinkMessage {

    pub fn new(mtype: u16, flags: u16, seq: u32) -> Self {
        let mut msg = NetlinkMessage(Buffer::new_empty());
        NetlinkHeader::new(msg.0.next(NL_HDRLEN), mtype)
            .flags(flags)
            .seq(seq);
        msg
    }

    fn ifinfomsg(self, family: u8) -> Self {
        self.with_ifinfomsg(family, |_| { })
    }

    fn with_ifinfomsg<F>(mut self, family: u8, mut f: F) -> Self
    where F: FnMut(IfInfoMsgHdr)
    {
        const IF_INFOHDRLEN: usize = 16;

        f(IfInfoMsgHdr::new(self.0.next(IF_INFOHDRLEN), family));
        self
    }

    fn with_rtmsg<F>(mut self, family: u8, mut f: F) -> Self
    where F: FnMut(RtMsg)
    {
        const RTMSG_HDRLEN: usize = 12;

        f(RtMsg::new(self.0.next(RTMSG_HDRLEN), family));
        self
    }

    fn with_ifaddrmsg<F>(mut self, mut f: F) -> Self
    where F: FnMut(IfAddrMsg)
    {
        const IFADDRMSG_LEN: usize = 8;
        f(IfAddrMsg::new(self.0.next(IFADDRMSG_LEN)));
        self

    }

    fn with_nested<F>(self, atype: u16, mut f: F) -> Self
    where F: FnMut(&mut Buffer<Vec<u8>>)
    {
        let mut nested = Buffer::new_empty();
        nested.write_u16(0);
        nested.write_u16(atype | NLA_F_NESTED);
        f(&mut nested);
        nested.align();
        nested.write_u16_at(0, nested.len() as u16);
        self.write(nested.as_bytes())
    }

    fn attr_u32(mut self, atype: u16, val: u32) -> Self {
        self.0.attr_u32(atype, val);
        self
    }

    fn attr_str(mut self, atype: u16, val: &str) -> Self {
        self.0.attr_str(atype, val);
        self
    }

    fn append_attr(mut self, atype: u16, data: &[u8]) -> Self {
        self.0.append_attr(atype, data);
        self
    }

    fn write(mut self, data: &[u8]) -> Self {
        self.0.write(data).align();
        self
    }

    fn update_len(&mut self) {
        self.0.align();
        self.0.write_u32_at(0, self.0.len() as u32);
    }

    fn done(mut self) -> Self {
        self.update_len();
        self
    }

    fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

struct Buffer<T: AsMut<[u8]>+AsRef<[u8]>>(T);

impl <T: AsMut<[u8]>+AsRef<[u8]>> Buffer<T> {

    fn write_u8_at(&mut self, offset: usize, val: u8) -> &mut Self {
        self.write_at(offset, &val.to_ne_bytes())
    }

    fn write_u16_at(&mut self, offset: usize, val: u16) -> &mut Self {
        self.write_at(offset, &val.to_ne_bytes())
    }

    fn write_u32_at(&mut self, offset: usize, val: u32) -> &mut Self {
        self.write_at(offset, &val.to_ne_bytes())
    }

    fn slice_at(&mut self, off: usize, len: usize) -> &mut [u8] {
        &mut self.0.as_mut()[off..off+len]
    }

    fn write_at(&mut self, offset: usize, bytes: &[u8]) -> &mut Self {
        self.slice_at(offset, bytes.len())
            .copy_from_slice(bytes);
        self
    }

    fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl <'a> Buffer<&'a mut [u8]> {
    fn bytes(bytes: &'a mut [u8]) -> Self {
        Buffer(bytes)
    }
}

impl Buffer<Vec<u8>> {

    fn new_empty() -> Self {
        Buffer(Vec::new())
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn next(&mut self, size: usize) -> &mut [u8] {
        let off = self.0.len();
        self.0.resize(off + size, 0);
        self.slice_at(off, size)
    }

    fn align(&mut self) {
        let aligned_len = self.0.len() + (MESSAGE_ALIGN - 1) & !(MESSAGE_ALIGN - 1);
        self.0.resize(aligned_len, 0);
    }

    fn _write_u8(&mut self, val: u8) -> &mut Self {
        self.write(&val.to_ne_bytes())
    }

    fn write_u16(&mut self, val: u16) -> &mut Self {
        self.write(&val.to_ne_bytes())
    }

    fn _write_u32(&mut self, val: u32) -> &mut Self {
        self.write(&val.to_ne_bytes())
    }

    fn write(&mut self, bytes: &[u8]) -> &mut Self {
        self.next(bytes.len())
            .copy_from_slice(bytes);
        self
    }

    fn attr_u32(&mut self, atype: u16, val: u32) -> &mut Self {
        self.append_attr(atype, &val.to_ne_bytes())
    }

    fn attr_str(&mut self, atype: u16, val: &str) -> &mut Self {
        self.append_attr(atype, val.as_bytes())
    }

    fn append_attr(&mut self, atype: u16, data: &[u8]) -> &mut Self {
        let attrlen = data.len() + ATTR_HDRLEN;
        assert!(attrlen <= u16::max_value() as usize);
        self.write_u16(attrlen as u16)
            .write_u16(atype)
            .write(data)
            .align();
        self
    }
}

struct InBuffer {
    bytes: Vec<u8>,
    offset: usize,
}

impl InBuffer {
    fn new(bytes: &[u8]) -> Self {
        let mut v = Vec::new();
        v.extend_from_slice(bytes);
        InBuffer { bytes: v, offset: 0}
    }

    fn next(&mut self, size: usize) -> &[u8] {
        assert!(self.offset + size <= self.bytes.len());
        let off = self.skip(size);
        &self.bytes[off..off+size]
    }

    fn read_u16(&mut self) -> u16 {
        u16::from_ne_bytes(self.next(2).try_into().unwrap())
    }

    fn read_u32(&mut self) -> u32 {
        u32::from_ne_bytes(self.next(4).try_into().unwrap())
    }

    fn skip(&mut self, n: usize) -> usize {
        let off = self.offset;
        self.offset += n;
        off
    }
}

pub struct RtMsg<'a>(Buffer<&'a mut [u8]>);

impl <'a> RtMsg <'a> {
    pub fn new(bytes: &'a mut [u8], family: u8) -> Self {
        let mut buffer = Buffer::bytes(bytes);
        buffer.write_u8_at(0, family);
        RtMsg(buffer)
    }

    pub fn table(mut self, table: u8) -> Self {
        self.0.write_u8_at(4, table);
        self
    }

    pub fn protocol(mut self, proto: u8) -> Self {
        self.0.write_u8_at(5, proto);
        self
    }

    pub fn scope(mut self, scope: u8) -> Self {
        self.0.write_u8_at(6, scope);
        self
    }

    pub fn rtype(mut self, rtype: u8) -> Self {
        self.0.write_u8_at(7, rtype);
        self
    }
}

pub struct IfInfoMsgHdr<'a>(Buffer<&'a mut [u8]>);

impl <'a> IfInfoMsgHdr <'a> {
    pub fn new(bytes: &'a mut [u8], family: u8) -> Self {
        let mut buffer = Buffer::bytes(bytes);
        buffer.write_u8_at(0, family);
        IfInfoMsgHdr(buffer)
    }

    fn index(mut self, index: u32) -> Self {
        self.0.write_u32_at(4, index);
        self
    }

    pub fn set_flags(mut self, flags: u32) -> Self {
        self.0.write_u32_at(8, flags);
        self
    }

    pub fn set_change(mut self, flags: u32) -> Self {
        self.0.write_u32_at(12, flags);
        self
    }
}

pub struct IfAddrMsg<'a>(Buffer<&'a mut [u8]>);

impl <'a> IfAddrMsg <'a> {
    fn new(bytes: &'a mut [u8]) -> Self {
        IfAddrMsg(Buffer::bytes(bytes))
    }

    pub fn family(mut self, family: u8) -> Self {
        self.0.write_u8_at(0, family);
        self
    }
    pub fn prefixlen(mut self, prefixlen: u8) -> Self {
        self.0.write_u8_at(1, prefixlen);
        self
    }
    pub fn _flags(mut self, flags: u8) -> Self {
        self.0.write_u8_at(2, flags);
        self
    }
    pub fn scope(mut self, scope: u8) -> Self {
        self.0.write_u8_at(3, scope);
        self
    }
    pub fn index(mut self, index: u32) -> Self {
        self.0.write_u32_at(4, index);
        self
    }
}

pub struct NetlinkHeader<'a>(Buffer<&'a mut [u8]>);

impl <'a> NetlinkHeader <'a> {
    pub fn new(bytes: &'a mut [u8], mtype: u16) -> Self {
        let mut buffer = Buffer::bytes(bytes);
        buffer.write_u16_at(4, mtype);
        NetlinkHeader(buffer)
    }

    pub fn flags(mut self, flags: u16) -> Self {
        self.0.write_u16_at(6, flags);
        self
    }

    fn seq(mut self, seq: u32) -> Self {
        self.0.write_u32_at(8, seq);
        self
    }

    fn _portid(mut self, portid: u32) -> Self {
        self.0.write_u32_at(12, portid);
        self
    }
}

fn sys_socket(domain: i32, stype: i32, protocol: i32) -> Result<RawFd> {
    unsafe {
        let fd = libc::socket(domain, stype, protocol);
        if fd < 0 {
            Err(Error::Socket(io::Error::last_os_error()))
        } else {
            Ok(fd)
        }
    }
}

fn sys_bind(sockfd: RawFd, addr: *const libc::sockaddr, addrlen: usize) -> Result<()> {
    unsafe {
        if libc::bind(sockfd, addr, addrlen as u32) < 0 {
            Err(Error::SocketBind(io::Error::last_os_error()))
        } else {
            Ok(())
        }
    }
}

fn sys_sendto(sockfd: RawFd, buf: &[u8], addr: *const libc::sockaddr, addrlen: usize) -> Result<usize> {
    let len = buf.len();
    let buf = buf.as_ptr() as *const libc::c_void;
    let flags = 0;
    unsafe {
        let size = libc::sendto(sockfd, buf, len, flags, addr, addrlen as u32);
        if size < 0 {
            Err(Error::SocketSend(io::Error::last_os_error()))
        } else {
            Ok(size as usize)
        }
    }
}

fn sys_recv(sockfd: RawFd, buf: &mut [u8], flags: i32) -> Result<usize> {
    let len = buf.len();
    let buf = buf.as_mut_ptr() as *mut libc::c_void;
    unsafe {
        let size = libc::recv(sockfd, buf, len, flags);
        if size < 0 {
            Err(Error::SocketRecv(io::Error::last_os_error()))
        } else {
            Ok(size as usize)
        }
    }
}
