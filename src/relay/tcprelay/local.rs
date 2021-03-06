// The MIT License (MIT)

// Copyright (c) 2014 Y. T. CHUNG <zonyitoo@gmail.com>

// Permission is hereby granted, free of charge, to any person obtaining a copy of
// this software and associated documentation files (the "Software"), to deal in
// the Software without restriction, including without limitation the rights to
// use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software is furnished to do so,
// subject to the following conditions:

// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
// FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
// COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
// IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
// CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

//! TcpRelay server that running on local environment

use std::io::{Listener, TcpListener, Acceptor, TcpStream};
use std::io::{
    IoResult,
    IoError,
    EndOfFile,
    ConnectionFailed,
    ConnectionRefused,
    ConnectionReset,
    ConnectionAborted,
    BrokenPipe,
    OtherIoError,
};
use std::io::net::ip::{SocketAddr, IpAddr};
use std::io::net::addrinfo::get_host_addresses;
use std::io::{self, BufferedStream};
use std::thread::Thread;
use std::collections::BTreeMap;

use config::Config;

use relay::Relay;
use relay::socks5;
use relay::loadbalancing::server::{LoadBalancer, RoundRobin};
use relay::tcprelay::stream::{EncryptedWriter, DecryptedReader};

use crypto::cipher;
use crypto::cipher::CipherType;
use crypto::CryptoMode;

#[derive(Clone)]
pub struct TcpRelayLocal {
    config: Config,
}

#[inline]
fn make_io_error(desc: &'static str, detail: Option<String>) -> IoError {
    IoError {
        kind: OtherIoError,
        desc: desc,
        detail: detail,
    }
}

macro_rules! try_result{
    ($res:expr) => ({
        let res = $res;
        match res {
            Ok(r) => { r },
            Err(err) => {
                error!("{}", err);
                return;
            }
        }
    });
    ($res:expr, prefix: $prefix:expr) => ({
        let res = $res;
        let prefix = $prefix;
        match res {
            Ok(r) => { r },
            Err(err) => {
                error!("{} {}", prefix, err);
                return;
            }
        }
    });
    ($res:expr, message: $message:expr) => ({
        let res = $res;
        let message = $message;
        match res {
            Ok(r) => { r },
            Err(..) => {
                error!("{}", message);
                return;
            }
        }
    });
}

impl TcpRelayLocal {
    pub fn new(c: Config) -> TcpRelayLocal {
        if c.server.is_empty() || c.local.is_none() {
            panic!("You have to provide configuration for server and local");
        }

        TcpRelayLocal {
            config: c,
        }
    }

    fn do_handshake(stream: &mut TcpStream) -> IoResult<()> {
        // Read the handshake header
        let req = try!(socks5::HandshakeRequest::read_from(stream));

        if !req.methods.contains(&socks5::SOCKS5_AUTH_METHOD_NONE) {
            let resp = socks5::HandshakeResponse::new(socks5::SOCKS5_AUTH_METHOD_NOT_ACCEPTABLE);
            try!(resp.write_to(stream));
            warn!("Currently shadowsocks-rust does not support authentication");
            return Err(make_io_error("Currently shadowsocks-rust does not support authentication", None));
        }

        // Reply to client
        let resp = socks5::HandshakeResponse::new(socks5::SOCKS5_AUTH_METHOD_NONE);
        try!(resp.write_to(stream));
        Ok(())
    }

    fn handle_udp_associate_local(stream: &mut TcpStream, _: &socks5::Address) -> IoResult<()> {
        let sockname = try!(stream.socket_name());

        let reply = socks5::TcpResponseHeader::new(socks5::Reply::Succeeded,
                                                   socks5::Address::SocketAddress(sockname.ip, sockname.port));
        try!(reply.write_to(stream));

        // TODO: record this client's information for udprelay local server to validate
        //       whether the client has already authenticated

        Ok(())
    }

    fn handle_client(mut stream: TcpStream,
                     server_addr: SocketAddr,
                     password: Vec<u8>,
                     encrypt_method: CipherType,
                     enable_udp: bool) {
        try_result!(TcpRelayLocal::do_handshake(&mut stream), prefix: "Error occurs while doing handshake:");

        let sockname = try_result!(stream.socket_name(), prefix: "Failed to get socket name:");

        let header = match socks5::TcpRequestHeader::read_from(&mut stream) {
            Ok(h) => { h },
            Err(err) => {
                socks5::TcpResponseHeader::new(err.reply,
                                               socks5::Address::SocketAddress(sockname.ip, sockname.port));
                error!("Failed to read request header: {}", err);
                return;
            }
        };

        let addr = header.address;

        match header.command {
            socks5::Command::TcpConnect => {
                info!("CONNECT {}", addr);

                let mut remote_stream = match TcpStream::connect((server_addr.ip, server_addr.port)) {
                    Err(err) => {
                        match err.kind {
                            ConnectionAborted | ConnectionReset | ConnectionRefused | ConnectionFailed => {
                                socks5::TcpResponseHeader::new(socks5::Reply::HostUnreachable, addr.clone())
                                    .write_to(&mut stream).unwrap();
                            },
                            _ => {
                                socks5::TcpResponseHeader::new(socks5::Reply::NetworkUnreachable, addr.clone())
                                    .write_to(&mut stream).unwrap();
                            }
                        }
                        error!("Failed to connect remote server: {}", err);
                        return;
                    },
                    Ok(s) => { s },
                };

                let mut buffered_local_stream = BufferedStream::new(stream.clone());

                let iv = encrypt_method.gen_init_vec();
                let encryptor = cipher::with_type(encrypt_method,
                                                  password.as_slice(),
                                                  iv.as_slice(),
                                                  CryptoMode::Encrypt);
                try_result!(remote_stream.write(iv.as_slice()));
                let mut encrypt_stream = EncryptedWriter::new(remote_stream.clone(), encryptor);

                {
                    try_result!(socks5::TcpResponseHeader::new(
                                                    socks5::Reply::Succeeded,
                                                    socks5::Address::SocketAddress(sockname.ip, sockname.port))
                                .write_to(&mut buffered_local_stream),
                        prefix: "Error occurs while writing header to local stream:");
                    try_result!(buffered_local_stream.flush());
                    try_result!(addr.write_to(&mut encrypt_stream));
                }

                let addr_cloned = addr.clone();
                let mut remote_stream_cloned = remote_stream.clone();
                let mut local_stream_cloned = stream.clone();
                Thread::spawn(move || {
                    match io::util::copy(&mut buffered_local_stream, &mut encrypt_stream) {
                        Ok(..) => {},
                        Err(err) => {
                            match err.kind {
                                EndOfFile | BrokenPipe => {
                                    debug!("{} relay from local to remote stream: {}", addr_cloned, err)
                                },
                                _ => {
                                    error!("{} relay from local to remote stream: {}", addr_cloned, err)
                                }
                            }
                            remote_stream_cloned.close_write().or(Ok(())).unwrap();
                            local_stream_cloned.close_read().or(Ok(())).unwrap();
                        }
                    }
                });

                let remote_iv = try_result!(remote_stream.read_exact(encrypt_method.block_size()));
                let decryptor = cipher::with_type(encrypt_method,
                                                  password.as_slice(),
                                                  remote_iv.as_slice(),
                                                  CryptoMode::Decrypt);
                let mut decrypt_stream = DecryptedReader::new(remote_stream.clone(), decryptor);
                match io::util::copy(&mut decrypt_stream, &mut stream) {
                    Err(err) => {
                        match err.kind {
                            EndOfFile | BrokenPipe => {
                                debug!("{} relay from local to remote stream: {}", addr, err)
                            },
                            _ => {
                                error!("{} relay from local to remote stream: {}", addr, err)
                            }
                        }
                        remote_stream.close_write().or(Ok(())).unwrap();
                        stream.close_read().or(Ok(())).unwrap();
                    },
                    Ok(..) => {},
                }
            },
            socks5::Command::TcpBind => {
                warn!("BIND is not supported");
                try_result!(socks5::TcpResponseHeader::new(socks5::Reply::CommandNotSupported, addr)
                    .write_to(&mut stream),
                    prefix: "Failed to write BIND response:");
            },
            socks5::Command::UdpAssociate => {
                let sockname = stream.peer_name().unwrap();
                info!("{} requests for UDP ASSOCIATE", sockname);
                if cfg!(feature = "enable-udp") && enable_udp {
                    try_result!(TcpRelayLocal::handle_udp_associate_local(&mut stream, &addr),
                                prefix: "Failed to write UDP ASSOCIATE response:");
                } else {
                    warn!("UDP ASSOCIATE is disabled");
                    try_result!(socks5::TcpResponseHeader::new(socks5::Reply::CommandNotSupported, addr)
                        .write_to(&mut stream),
                        prefix: "Failed to write UDP ASSOCIATE response:");
                }
            }
        }
    }
}

impl Relay for TcpRelayLocal {
    fn run(&self) {
        let mut server_load_balancer = RoundRobin::new(self.config.server.clone());

        let local_conf = self.config.local.expect("need local configuration");

        let mut acceptor = match TcpListener::bind(
                format!("{}:{}", local_conf.ip, local_conf.port).as_slice()).listen() {
            Ok(acpt) => acpt,
            Err(e) => {
                panic!("Error occurs while listening local address: {}", e.to_string());
            }
        };

        info!("Shadowsocks listening on {}", local_conf);

        let mut cached_proxy: BTreeMap<String, Vec<IpAddr>> = BTreeMap::new();

        for s in acceptor.incoming() {
            let mut stream = s.unwrap();
            stream.set_timeout(self.config.timeout);

            let mut succeed = false;
            for _ in range(0, server_load_balancer.total()) {
                let ref server_cfg = server_load_balancer.pick_server();
                let addrs = {
                    match cached_proxy.get(server_cfg.addr.as_slice()).map(|x| x.clone()) {
                        Some(addr) => addr,
                        None => {
                            match get_host_addresses(server_cfg.addr.as_slice()) {
                                Ok(addr) => {
                                    if addr.is_empty() {
                                        error!("cannot resolve proxy server `{}`", server_cfg.addr);
                                        continue;
                                    }
                                    cached_proxy.insert(server_cfg.addr.clone(), addr.clone());
                                    addr
                                },
                                Err(err) => {
                                    error!("cannot resolve proxy server `{}`: {}", server_cfg.addr, err);
                                    continue;
                                }
                            }
                        }
                    }
                };

                let server_addr = SocketAddr {
                    ip: addrs.first().unwrap().clone(),
                    port: server_cfg.port,
                };
                debug!("Using proxy `{}:{}` (`{}`)", server_cfg.addr, server_cfg.port, server_addr);
                let encrypt_method = server_cfg.method.clone();
                let pwd = encrypt_method.bytes_to_key(server_cfg.password.as_bytes());
                let enable_udp = self.config.enable_udp;

                Thread::spawn(move ||
                    TcpRelayLocal::handle_client(stream,
                                                 server_addr,
                                                 pwd,
                                                 encrypt_method,
                                                 enable_udp));
                succeed = true;
                break;
            }
            if !succeed {
                panic!("All proxy servers are failed!");
            }
        }
    }
}
