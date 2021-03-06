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

// SOCKS5 UDP Request
// +----+------+------+----------+----------+----------+
// |RSV | FRAG | ATYP | DST.ADDR | DST.PORT |   DATA   |
// +----+------+------+----------+----------+----------+
// | 2  |  1   |  1   | Variable |    2     | Variable |
// +----+------+------+----------+----------+----------+

// SOCKS5 UDP Response
// +----+------+------+----------+----------+----------+
// |RSV | FRAG | ATYP | DST.ADDR | DST.PORT |   DATA   |
// +----+------+------+----------+----------+----------+
// | 2  |  1   |  1   | Variable |    2     | Variable |
// +----+------+------+----------+----------+----------+

// shadowsocks UDP Request (before encrypted)
// +------+----------+----------+----------+
// | ATYP | DST.ADDR | DST.PORT |   DATA   |
// +------+----------+----------+----------+
// |  1   | Variable |    2     | Variable |
// +------+----------+----------+----------+

// shadowsocks UDP Response (before encrypted)
// +------+----------+----------+----------+
// | ATYP | DST.ADDR | DST.PORT |   DATA   |
// +------+----------+----------+----------+
// |  1   | Variable |    2     | Variable |
// +------+----------+----------+----------+

// shadowsocks UDP Request and Response (after encrypted)
// +-------+--------------+
// |   IV  |    PAYLOAD   |
// +-------+--------------+
// | Fixed |   Variable   |
// +-------+--------------+

use std::sync::{Arc, Mutex};
use std::io::net::udp::UdpSocket;
use std::io::net::ip::SocketAddr;
use std::io::net::addrinfo::get_host_addresses;
use std::collections::HashMap;
use std::io::{BufReader, MemWriter, self};
use std::thread::Thread;

use collect::LruCache;

use crypto::{cipher, CryptoMode};
use crypto::cipher::Cipher;
use config::{Config, ServerConfig};
use relay::Relay;
use relay::socks5;
use relay::loadbalancing::server::{LoadBalancer, RoundRobin};
use relay::udprelay::UDP_RELAY_LOCAL_LRU_CACHE_CAPACITY;

#[derive(Clone)]
pub struct UdpRelayLocal {
    config: Config,
}

impl UdpRelayLocal {
    pub fn new(config: Config) -> UdpRelayLocal {
        UdpRelayLocal {
            config: config,
        }
    }
}

impl Relay for UdpRelayLocal {
    fn run(&self) {
        let addr = self.config.local.expect("Local configuration should not be None");

        let mut server_load_balancer = RoundRobin::new(self.config.server.clone());

        let (server_set, server_addr) = {
            let mut server_set = HashMap::new();
            let mut server_addr = HashMap::new();
            for s in self.config.server.iter() {
                let addrs = match get_host_addresses(s.addr.as_slice()) {
                    Ok(addr) => addr,
                    Err(..) => continue,
                };

                if !addrs.is_empty() {
                    let addr = SocketAddr {
                        ip: addrs.first().unwrap().clone(),
                        port: s.port,
                    };

                    server_set.insert(addr, s.clone());
                    server_addr.insert(s.addr.clone(), addr);
                }
            }
            (server_set, server_addr)
        };

        let client_map_arc = Arc::new(Mutex::new(
                    LruCache::<socks5::Address, SocketAddr>::new(UDP_RELAY_LOCAL_LRU_CACHE_CAPACITY)));

        let mut socket = UdpSocket::bind(addr).ok().expect("Failed to bind udp socket");

        let mut buf = [0u8; 0xffff];
        loop {
            match socket.recv_from(&mut buf) {
                Ok((len, source_addr)) => {
                    if len < 4 {
                        error!("UDP request is too short");
                        continue;
                    }

                    let request_message = buf[..len].to_vec();
                    let move_socket = socket.clone();
                    let client_map = client_map_arc.clone();

                    match server_set.get(&source_addr) {
                        Some(sref) => {
                            let s = sref.clone();
                            Thread::spawn(move ||
                                handle_response(move_socket,
                                               request_message.as_slice(),
                                               source_addr,
                                               &s,
                                               client_map));
                        }
                        None => {
                            let s = server_load_balancer.pick_server().clone();

                            match server_addr.get(&s.addr) {
                                Some(saddr) => {
                                    let saddr = saddr.clone();
                                    Thread::spawn(move ||
                                        handle_request(move_socket,
                                                      request_message.as_slice(),
                                                      source_addr,
                                                      saddr,
                                                      &s,
                                                      client_map));
                                },
                                None => {}
                            }
                        }
                    }
                },
                Err(err) => {
                    error!("Failed in UDP recv_from: {}", err);
                    break
                }
            }
        }
    }
}

fn handle_request(mut socket: UdpSocket,
                  request_message: &[u8],
                  from_addr: SocketAddr,
                  server_addr: SocketAddr,
                  config: &ServerConfig,
                  client_map: Arc<Mutex<LruCache<socks5::Address, SocketAddr>>>) {
    // According to RFC 1928
    //
    // Implementation of fragmentation is optional; an implementation that
    // does not support fragmentation MUST drop any datagram whose FRAG
    // field is other than X'00'.
    if request_message[2] != 0x00u8 {
        // Drop it
        warn!("Does not support fragmentation");
        return;
    }


    let mut bufr = BufReader::new(request_message);
    let request = socks5::UdpAssociateHeader::read_from(&mut bufr).unwrap();

    let addr = request.address.clone();

    info!("UDP ASSOCIATE {}", addr);
    debug!("UDP associate {} <-> {}", addr, from_addr);

    client_map.lock().unwrap().insert(addr, from_addr);

    let key = config.method.bytes_to_key(config.password.as_bytes());
    let mut iv = config.method.gen_init_vec();
    let mut encryptor = cipher::with_type(config.method,
                                          key.as_slice(),
                                          iv.as_slice(),
                                          CryptoMode::Encrypt);

    let mut wbuf = Vec::new();
    request.write_to(&mut wbuf).unwrap();
    io::util::copy(&mut bufr, &mut wbuf).unwrap();

    iv.push_all(encryptor.update(wbuf.as_slice()).unwrap().as_slice());
    iv.push_all(encryptor.finalize().unwrap().as_slice());

    socket.send_to(iv.as_slice(), server_addr)
        .ok().expect("Error occurs while sending to remote");
}

fn handle_response(mut socket: UdpSocket,
                   response_message: &[u8],
                   from_addr: SocketAddr,
                   config: &ServerConfig,
                   client_map: Arc<Mutex<LruCache<socks5::Address, SocketAddr>>>) {
    let key = config.method.bytes_to_key(config.password.as_bytes());

    let mut decryptor = cipher::with_type(config.method,
                                          key.as_slice(),
                                          &response_message[0..config.method.block_size()],
                                          CryptoMode::Decrypt);
    let mut decrypted_data = decryptor.update(&response_message[config.method.block_size()..]).unwrap();
    decrypted_data.push_all(decryptor.finalize().unwrap().as_slice());

    let mut bufr = BufReader::new(decrypted_data.as_slice());

    let addr = socks5::Address::read_from(&mut bufr).unwrap();

    let client_addr = {
        let mut cmap = client_map.lock().unwrap();
        match cmap.get(&addr) {
            Some(a) => a.clone(),
            None => return
        }
    };

    debug!("UDP response {} -> {}", from_addr, client_addr);

    let mut bufw = MemWriter::new();
    socks5::UdpAssociateHeader::new(0, addr)
        .write_to(&mut bufw).unwrap();
    io::util::copy(&mut bufr, &mut bufw).unwrap();

    socket.send_to(bufw.into_inner().as_slice(), client_addr)
        .ok().expect("Error occurs while sending to local");
}
