//! Multiplayer over TCP, in lockstep: every machine runs the whole game
//! and only orders travel. The host gathers everyone's orders and fixes
//! the turn each is carried out at, a few turns ahead; a machine plays a
//! turn once it knows that turn's orders, so all play the same game. Each
//! client reports a checksum of every turn to the host, which warns when
//! the games drift apart (they need the same build of OpenConquest).
//!
//! Messages are JSON lines.

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::scene::Order;

/// The default port.
pub const PORT: u16 = 17_999;
/// Seconds of game a turn plays, in steps of `STEP`.
pub const TURN: f32 = 0.1;
pub const STEP: f32 = 0.02;
/// Turns between an order and the turn it is carried out at.
const DELAY: u64 = 3;
/// Turns the host may play ahead of the slowest client.
const LEAD: u64 = 10;
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What every machine needs to start the same game.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Setup {
    pub mission: String,
    /// The human players by seat: (nation, slot).
    pub seats: Vec<(u32, u32)>,
    pub difficulty: usize,
    pub fog: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
enum Msg {
    /// Everyone is here: the game begins.
    Start,
    /// A line of chat from a player (relayed to all by the host).
    Chat {
        seat: usize,
        text: String,
    },
    Hello {
        version: String,
    },
    Welcome {
        seat: usize,
        setup: Setup,
    },
    Orders {
        orders: Vec<Order>,
    },
    Turn {
        turn: u64,
        orders: Vec<(usize, Order)>,
    },
    Ack {
        turn: u64,
        check: u64,
    },
    Refused {
        reason: String,
    },
}

/// A line-oriented JSON connection.
struct Link {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    line: String,
    out: Vec<u8>,
    closed: bool,
}

impl Link {
    fn new(stream: TcpStream) -> Result<Self> {
        stream.set_nodelay(true)?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Link {
            stream,
            reader,
            line: String::new(),
            out: Vec::new(),
            closed: false,
        })
    }

    fn send(&mut self, msg: &Msg) {
        if let Ok(mut s) = serde_json::to_vec(msg) {
            s.push(b'\n');
            self.out.extend(s);
        }
        self.flush();
    }

    fn flush(&mut self) {
        while !self.out.is_empty() && !self.closed {
            match self.stream.write(&self.out) {
                Ok(0) => self.closed = true,
                Ok(n) => {
                    self.out.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => self.closed = true,
            }
        }
    }

    /// The next whole message, if one has arrived.
    fn recv(&mut self) -> Option<Msg> {
        self.flush();
        loop {
            match self.reader.read_line(&mut self.line) {
                Ok(0) => {
                    self.closed = true;
                    return None;
                }
                Ok(_) if self.line.ends_with('\n') => {
                    let msg = serde_json::from_str(self.line.trim()).ok();
                    self.line.clear();
                    if msg.is_some() {
                        return msg;
                    }
                }
                Ok(_) => return None,
                Err(e) if e.kind() == ErrorKind::WouldBlock => return None,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => {
                    self.closed = true;
                    return None;
                }
            }
        }
    }
}
struct Client {
    link: Link,
    seat: usize,
    acked: u64,
}

enum Role {
    Host {
        clients: Vec<Client>,
        /// Orders waiting for a turn, by seat.
        inbox: Vec<(usize, Order)>,
        /// The host's own checksums, to compare clients' with.
        checks: BTreeMap<u64, u64>,
    },
    Join {
        link: Link,
        acks: VecDeque<(u64, u64)>,
    },
}

/// A game being hosted, waiting for its players.
pub struct Lobby {
    listener: TcpListener,
    /// Connections that have not said who they are yet.
    pending: Vec<Link>,
    clients: Vec<Client>,
    pub setup: Setup,
    pub port: u16,
}

impl Lobby {
    pub fn open(port: u16, setup: Setup) -> Result<Lobby> {
        let listener = TcpListener::bind(("0.0.0.0", port))
            .with_context(|| format!("cannot listen on port {port}"))?;
        listener.set_nonblocking(true)?;
        Ok(Lobby {
            listener,
            pending: Vec::new(),
            clients: Vec::new(),
            setup,
            port,
        })
    }

    /// Players still to join.
    pub fn missing(&self) -> usize {
        self.setup.seats.len() - 1 - self.clients.len()
    }

    /// Takes in whoever has come; the game, once everyone is here.
    pub fn poll(&mut self) -> Result<Option<Net>> {
        while let Ok((stream, _)) = self.listener.accept() {
            stream.set_nonblocking(true)?;
            self.pending.push(Link::new(stream)?);
        }
        for mut link in std::mem::take(&mut self.pending) {
            match link.recv() {
                Some(Msg::Hello { version }) if version == VERSION && self.missing() > 0 => {
                    let seat = self.clients.len() + 1;
                    link.send(&Msg::Welcome {
                        seat,
                        setup: self.setup.clone(),
                    });
                    self.clients.push(Client {
                        link,
                        seat,
                        acked: 0,
                    });
                }
                Some(Msg::Hello { version }) if version != VERSION => link.send(&Msg::Refused {
                    reason: format!("the host runs OpenConquest {VERSION}, you {version}"),
                }),
                Some(_) => link.send(&Msg::Refused {
                    reason: "the game is full".into(),
                }),
                None if !link.closed => self.pending.push(link),
                None => {}
            }
        }
        // Someone who left before the start frees their seat.
        self.clients.retain_mut(|c| {
            c.link.flush();
            !c.link.closed
        });
        for (k, c) in self.clients.iter_mut().enumerate() {
            c.seat = k + 1;
        }
        if self.missing() > 0 {
            return Ok(None);
        }
        // Seats may have shifted: tell everyone theirs again before the start.
        for c in &mut self.clients {
            c.link.send(&Msg::Welcome {
                seat: c.seat,
                setup: self.setup.clone(),
            });
            c.link.send(&Msg::Start);
        }
        Ok(Some(Net::new(
            Role::Host {
                clients: std::mem::take(&mut self.clients),
                inbox: Vec::new(),
                checks: BTreeMap::new(),
            },
            0,
            self.setup.clone(),
        )))
    }
}

/// Joining a hosted game, until it starts.
pub struct Joining {
    link: Link,
    welcome: Option<(usize, Setup)>,
    pub address: String,
}

impl Joining {
    pub fn connect(addr: &str) -> Result<Joining> {
        let address = if addr.contains(':') {
            addr.to_string()
        } else {
            format!("{addr}:{PORT}")
        };
        let target = address
            .to_socket_addrs()
            .with_context(|| format!("cannot find {address}"))?
            .next()
            .with_context(|| format!("cannot find {address}"))?;
        let stream = TcpStream::connect_timeout(&target, Duration::from_secs(5))
            .with_context(|| format!("cannot reach {address}"))?;
        stream.set_nonblocking(true)?;
        let mut link = Link::new(stream)?;
        link.send(&Msg::Hello {
            version: VERSION.into(),
        });
        Ok(Joining {
            link,
            welcome: None,
            address,
        })
    }

    /// The player's seat, once the host has given one.
    pub fn seat(&self) -> Option<usize> {
        self.welcome.as_ref().map(|w| w.0)
    }

    /// The game, once the host starts it.
    pub fn poll(&mut self) -> Result<Option<Net>> {
        while let Some(msg) = self.link.recv() {
            match msg {
                Msg::Welcome { seat, setup } => self.welcome = Some((seat, setup)),
                Msg::Refused { reason } => bail!("the host refused: {reason}"),
                Msg::Start => {
                    let (seat, setup) = self.welcome.take().context("no seat from the host")?;
                    // The game takes the connection (and what it has read
                    // ahead); the lobby keeps a spare.
                    let spare = Link::new(self.link.stream.try_clone()?)?;
                    let link = std::mem::replace(&mut self.link, spare);
                    return Ok(Some(Net::new(
                        Role::Join {
                            link,
                            acks: VecDeque::new(),
                        },
                        seat,
                        setup,
                    )));
                }
                _ => {}
            }
        }
        if self.link.closed {
            bail!("the host has gone");
        }
        Ok(None)
    }
}

/// This machine's address on the local network, for others to join (the
/// route to the internet's, without sending anything).
pub fn local_address() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    Some(socket.local_addr().ok()?.ip())
}

/// The seats of a game of `players` on a map with `map` players: the
/// map's own player first, then its Greeks and Persians.
pub fn seats_for(
    map: &[openconquest_formats::PlayerInfo],
    players: usize,
) -> Option<Vec<(u32, u32)>> {
    let seats: Vec<(u32, u32)> = map
        .iter()
        .filter(|p| !p.computer)
        .chain(map.iter().filter(|p| p.computer && p.nation != 4))
        .map(|p| (p.nation, p.slot))
        .take(players)
        .collect();
    (players >= 2 && seats.len() == players).then_some(seats)
}

/// This machine's side of a multiplayer game.
pub struct Net {
    role: Role,
    pub seat: usize,
    pub setup: Setup,
    /// The next turn to play.
    pub turn: u64,
    turns: BTreeMap<u64, Vec<(usize, Order)>>,
    /// Problems to show the player (a player gone, the games drifting).
    pub notices: Vec<String>,
    /// Chat from the other players: (seat, text).
    pub chat: Vec<(usize, String)>,
}

impl Net {
    /// Hosts a game for `setup.seats.len()` players on `port`, waiting for
    /// the others to join (the host takes seat 0).
    pub fn host(port: u16, setup: Setup) -> Result<Net> {
        let mut lobby = Lobby::open(port, setup)?;
        let mut told = usize::MAX;
        loop {
            if let Some(net) = lobby.poll()? {
                return Ok(net);
            }
            if lobby.missing() != told {
                told = lobby.missing();
                eprintln!("waiting for {told} more player(s) on port {port}...");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Joins the game hosted at `addr` (host or host:port), waiting for it
    /// to start.
    pub fn join(addr: &str) -> Result<Net> {
        let mut joining = Joining::connect(addr)?;
        eprintln!("joined {addr}; waiting for the game to start...");
        loop {
            if let Some(net) = joining.poll()? {
                return Ok(net);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn new(role: Role, seat: usize, setup: Setup) -> Net {
        // The first turns have no orders: nobody could give any in time.
        let turns = (0..DELAY).map(|t| (t, Vec::new())).collect();
        Net {
            role,
            seat,
            setup,
            turn: 0,
            turns,
            notices: Vec::new(),
            chat: Vec::new(),
        }
    }

    /// Gives an order of this machine's player.
    pub fn send(&mut self, order: Order) {
        let seat = self.seat;
        match &mut self.role {
            Role::Host { inbox, .. } => inbox.push((seat, order)),
            Role::Join { link, .. } => link.send(&Msg::Orders {
                orders: vec![order],
            }),
        }
    }

    /// Says `text` to the other players.
    pub fn chat(&mut self, text: &str) {
        let msg = Msg::Chat {
            seat: self.seat,
            text: text.chars().take(200).collect(),
        };
        match &mut self.role {
            Role::Host { clients, .. } => {
                for c in clients.iter_mut() {
                    c.link.send(&msg);
                }
            }
            Role::Join { link, .. } => link.send(&msg),
        }
    }

    /// Reads what has arrived.
    pub fn poll(&mut self) {
        match &mut self.role {
            Role::Host {
                clients,
                inbox,
                checks,
            } => {
                let mut said = Vec::new();
                for c in clients.iter_mut() {
                    while let Some(msg) = c.link.recv() {
                        match msg {
                            Msg::Chat { text, .. } => said.push((c.seat, text)),
                            Msg::Orders { orders } => {
                                inbox.extend(orders.into_iter().map(|o| (c.seat, o)))
                            }
                            Msg::Ack { turn, check } => {
                                c.acked = c.acked.max(turn + 1);
                                if checks.get(&turn).is_some_and(|&mine| mine != check) {
                                    self.notices.push(format!(
                                        "Player {}'s game has drifted from yours (turn {turn})",
                                        c.seat + 1
                                    ));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                // Chat goes to everyone else, and shows here.
                for (seat, text) in said {
                    let msg = Msg::Chat {
                        seat,
                        text: text.clone(),
                    };
                    for c in clients.iter_mut().filter(|c| c.seat != seat) {
                        c.link.send(&msg);
                    }
                    self.chat.push((seat, text));
                }
                let before = clients.len();
                clients.retain(|c| !c.link.closed);
                if clients.len() < before {
                    self.notices.push("A player has left the game".into());
                }
            }
            Role::Join { link, acks } => {
                while let Some(msg) = link.recv() {
                    match msg {
                        Msg::Turn { turn, orders } => {
                            self.turns.insert(turn, orders);
                        }
                        Msg::Chat { seat, text } => self.chat.push((seat, text)),
                        _ => {}
                    }
                }
                while let Some((turn, check)) = acks.pop_front() {
                    link.send(&Msg::Ack { turn, check });
                }
                if link.closed && !self.notices.iter().any(|n| n.contains("host")) {
                    self.notices.push("The host has left the game".into());
                }
            }
        }
    }

    /// The orders of the next turn, if it can be played now.
    pub fn next_turn(&mut self) -> Option<Vec<(usize, Order)>> {
        let turn = self.turn;
        if let Role::Host { clients, inbox, .. } = &mut self.role {
            // Not too far ahead of the slowest machine.
            if clients.iter().any(|c| c.acked + LEAD < turn) {
                return None;
            }
            // Orders given now are carried out a few turns on.
            let orders = std::mem::take(inbox);
            let msg = Msg::Turn {
                turn: turn + DELAY,
                orders: orders.clone(),
            };
            for c in clients.iter_mut() {
                c.link.send(&msg);
            }
            self.turns.insert(turn + DELAY, orders);
        }
        let orders = self.turns.remove(&turn)?;
        self.turn += 1;
        Some(orders)
    }

    /// Notes the checksum of the turn just played.
    pub fn played(&mut self, turn: u64, check: u64) {
        match &mut self.role {
            Role::Host { checks, .. } => {
                checks.insert(turn, check);
                // Old ones are of no more use.
                while checks.len() > 1000 {
                    checks.pop_first();
                }
            }
            Role::Join { acks, .. } => acks.push_back((turn, check)),
        }
    }
}
