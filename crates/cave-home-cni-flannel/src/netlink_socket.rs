// SPDX-License-Identifier: Apache-2.0
//! The real `AF_NETLINK` / `NETLINK_ROUTE` datapath (Linux only).
//!
//! This is the privileged I/O layer the rest of the crate has been building
//! toward: a [`Datapath`] whose operations are the [`crate::netlink`] wire
//! messages written to a kernel netlink socket. The same [`crate::device`] /
//! [`crate::vxlan_network`] / [`crate::route_network`] logic that the unit
//! tests exercise against [`crate::datapath::MockDatapath`] runs unchanged here
//! against the live kernel — only the trait impl differs.
//!
//! The workspace forbids `unsafe`, so all syscalls go through `nix`'s safe
//! socket wrappers. Each request carries `NLM_F_ACK`; we read the kernel's
//! `NLMSG_ERROR` reply and surface a non-zero errno as [`NetError::Netlink`].
//! `link_add` additionally issues an `RTM_GETLINK` by name to learn the
//! kernel-assigned interface index (mirroring upstream's `LinkByName` after
//! `LinkAdd`).
//!
//! The live socket compiles only on Linux; on every other target the crate
//! still ships the codec, the backend logic, the mock and the pure ACK parser
//! below, which is all that is testable off a kernel.

use crate::datapath::NetError;
use crate::netlink::NLMSG_ERROR;

#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, OwnedFd};

#[cfg(target_os = "linux")]
use nix::sys::socket::{
    bind, recv, send, socket, AddressFamily, MsgFlags, NetlinkAddr, SockFlag, SockProtocol,
    SockType,
};

#[cfg(target_os = "linux")]
use crate::datapath::{
    encode_link_del, encode_link_set_up, Datapath, LinkAddr, Neigh, Route, VxlanLink,
};
#[cfg(target_os = "linux")]
use crate::netlink::{
    ifinfomsg, push_attr, NlMsg, AF_UNSPEC, IFLA_IFNAME, NLM_F_CREATE, NLM_F_EXCL, NLM_F_REPLACE,
    RTM_GETLINK,
};

/// A live `NETLINK_ROUTE` socket.
#[cfg(target_os = "linux")]
#[derive(Debug)]
pub struct NetlinkSocket {
    fd: OwnedFd,
    seq: u32,
}

#[cfg(target_os = "linux")]
impl NetlinkSocket {
    /// Open and bind a `NETLINK_ROUTE` socket.
    ///
    /// # Errors
    /// [`NetError::Io`] if the socket cannot be created or bound (e.g. the
    /// process lacks `CAP_NET_ADMIN`).
    pub fn open() -> Result<Self, NetError> {
        let fd = socket(
            AddressFamily::Netlink,
            SockType::Raw,
            SockFlag::empty(),
            SockProtocol::NetlinkRoute,
        )
        .map_err(|e| NetError::Io(format!("socket: {e}")))?;
        bind(fd.as_raw_fd(), &NetlinkAddr::new(0, 0))
            .map_err(|e| NetError::Io(format!("bind: {e}")))?;
        Ok(Self { fd, seq: 0 })
    }

    fn next_seq(&mut self) -> u32 {
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    /// Send a request and read the single reply message.
    fn transact(&self, msg: &[u8], op: &str) -> Result<Vec<u8>, NetError> {
        send(self.fd.as_raw_fd(), msg, MsgFlags::empty())
            .map_err(|e| NetError::Io(format!("send {op}: {e}")))?;
        let mut buf = vec![0u8; 8192];
        let n = recv(self.fd.as_raw_fd(), &mut buf, MsgFlags::empty())
            .map_err(|e| NetError::Io(format!("recv {op}: {e}")))?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Send a request and require a success ACK.
    fn check_ack(&self, msg: &[u8], op: &str) -> Result<(), NetError> {
        let resp = self.transact(msg, op)?;
        parse_ack(&resp, op)
    }

    /// `RTM_GETLINK` by name → the kernel-assigned interface index.
    fn query_link_index(&mut self, name: &str) -> Result<i32, NetError> {
        let seq = self.next_seq();
        let mut m = NlMsg::request(RTM_GETLINK, 0);
        m.seq = seq;
        m.body.extend_from_slice(&ifinfomsg(AF_UNSPEC, 0, 0, 0));
        let mut nbytes = name.as_bytes().to_vec();
        nbytes.push(0);
        push_attr(&mut m.body, IFLA_IFNAME, &nbytes);

        let resp = self.transact(&m.serialize(), "getlink")?;
        if resp.len() < 24 {
            return Err(NetError::Io(format!("getlink: short reply ({} bytes)", resp.len())));
        }
        let mtype = u16::from_le_bytes([resp[4], resp[5]]);
        if mtype == NLMSG_ERROR {
            // Surface the errno; if it parsed as success there's no index.
            parse_ack(&resp, "getlink")?;
            return Err(NetError::Netlink {
                op: "getlink".to_owned(),
                errno: 0,
            });
        }
        // RTM_NEWLINK reply: ifinfomsg.ifi_index at body offset 4 → buf 16+4.
        let mut idx = [0u8; 4];
        idx.copy_from_slice(&resp[20..24]);
        Ok(i32::from_le_bytes(idx))
    }
}

/// Parse a kernel reply as an ACK: `NLMSG_ERROR` with `error == 0` is success;
/// a negative `error` is `-errno`.
///
/// Used by the Linux socket impl and by the unit tests; on a non-Linux build
/// without the socket it is exercised only under `#[cfg(test)]`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_ack(resp: &[u8], op: &str) -> Result<(), NetError> {
    if resp.len() < 20 {
        return Err(NetError::Io(format!("{op}: short ack ({} bytes)", resp.len())));
    }
    let mtype = u16::from_le_bytes([resp[4], resp[5]]);
    if mtype == NLMSG_ERROR {
        let mut e = [0u8; 4];
        e.copy_from_slice(&resp[16..20]);
        let err = i32::from_le_bytes(e);
        if err == 0 {
            Ok(())
        } else {
            Err(NetError::Netlink {
                op: op.to_owned(),
                errno: -err,
            })
        }
    } else {
        // Not an error message (e.g. a multipart reply we don't await here).
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Datapath for NetlinkSocket {
    fn link_add(&mut self, link: &VxlanLink) -> Result<i32, NetError> {
        let seq = self.next_seq();
        self.check_ack(&link.encode(seq), "link_add")?;
        self.query_link_index(&link.name)
    }

    fn link_del(&mut self, index: i32) -> Result<(), NetError> {
        let seq = self.next_seq();
        self.check_ack(&encode_link_del(index, seq), "link_del")
    }

    fn link_set_up(&mut self, index: i32) -> Result<(), NetError> {
        let seq = self.next_seq();
        self.check_ack(&encode_link_set_up(index, seq), "link_set_up")
    }

    fn addr_add(&mut self, addr: &LinkAddr) -> Result<(), NetError> {
        let seq = self.next_seq();
        self.check_ack(&addr.encode(seq), "addr_add")
    }

    fn route_replace(&mut self, route: &Route) -> Result<(), NetError> {
        let seq = self.next_seq();
        self.check_ack(
            &route.encode(true, NLM_F_CREATE | NLM_F_REPLACE, seq),
            "route_replace",
        )
    }

    fn route_add(&mut self, route: &Route) -> Result<(), NetError> {
        let seq = self.next_seq();
        self.check_ack(
            &route.encode(true, NLM_F_CREATE | NLM_F_EXCL, seq),
            "route_add",
        )
    }

    fn route_del(&mut self, route: &Route) -> Result<(), NetError> {
        let seq = self.next_seq();
        self.check_ack(&route.encode(false, 0, seq), "route_del")
    }

    fn neigh_set(&mut self, neigh: &Neigh) -> Result<(), NetError> {
        let seq = self.next_seq();
        self.check_ack(&neigh.encode(true, seq), "neigh_set")
    }

    fn neigh_del(&mut self, neigh: &Neigh) -> Result<(), NetError> {
        let seq = self.next_seq();
        self.check_ack(&neigh.encode(false, seq), "neigh_del")
    }
}

#[cfg(test)]
mod tests {
    use super::parse_ack;
    use crate::datapath::NetError;
    use crate::netlink::NLMSG_ERROR;

    /// Build a synthetic `NLMSG_ERROR` reply carrying `errno`.
    fn ack_message(error: i32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&36u32.to_le_bytes()); // len
        b.extend_from_slice(&NLMSG_ERROR.to_le_bytes()); // type
        b.extend_from_slice(&0u16.to_le_bytes()); // flags
        b.extend_from_slice(&1u32.to_le_bytes()); // seq
        b.extend_from_slice(&0u32.to_le_bytes()); // pid
        b.extend_from_slice(&error.to_le_bytes()); // nlmsgerr.error
        b.extend_from_slice(&[0u8; 16]); // the echoed header (ignored)
        b
    }

    #[test]
    fn zero_errno_is_success() {
        assert!(parse_ack(&ack_message(0), "route_add").is_ok());
    }

    #[test]
    fn negative_errno_is_surfaced() {
        // Kernel reports -EEXIST (-17).
        let err = parse_ack(&ack_message(-17), "route_add").expect_err("err");
        assert_eq!(
            err,
            NetError::Netlink {
                op: "route_add".to_owned(),
                errno: 17
            }
        );
    }

    #[test]
    fn short_reply_is_an_io_error() {
        assert!(matches!(parse_ack(&[0u8; 4], "x"), Err(NetError::Io(_))));
    }
}
