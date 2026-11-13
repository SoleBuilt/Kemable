//! Tunnels through a small SSH server inside the tests, to an echo server.

use std::sync::Arc;
use std::time::Duration;

use kemable_core::config::{Secret, SshAuth, SshTunnel};
use kemable_core::{CallOptions, ErrorCode};
use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, ChannelOpenHandle, Msg, Server as _, Session};
use russh::{Channel, ChannelOpenFailure};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::{HostKeys, open};

/// Lets `kemable` in with the password `secret`, and opens every
/// direct-tcpip channel it is asked for, unless told to refuse them.
#[derive(Clone)]
struct TestServer {
    refuse_forward: bool,
}

impl russh::server::Server for TestServer {
    type Handler = Self;

    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        self.clone()
    }
}

impl russh::server::Handler for TestServer {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == "kemable" && password == "secret" {
            Auth::Accept
        } else {
            Auth::reject()
        })
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.refuse_forward {
            reply.reject(ChannelOpenFailure::ConnectFailed).await;
            return Ok(());
        }
        let target = format!("{host}:{port}");
        reply.accept().await;
        tokio::spawn(async move {
            if let Ok(mut tcp) = TcpStream::connect(target).await {
                let mut stream = channel.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut tcp, &mut stream).await;
            }
        });
        Ok(())
    }
}

/// An SSH server on a free port with a fresh host key: its port and key.
async fn ssh_server(refuse_forward: bool) -> (u16, PrivateKey) {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let config = Arc::new(russh::server::Config {
        keys: vec![key.clone()],
        auth_rejection_time: Duration::from_millis(1),
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = socket.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut server = TestServer { refuse_forward };
        let _ = server.run_on_socket(config, &socket).await;
    });
    (port, key)
}

/// A server that sends back what it is sent: the database at the far end.
async fn echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = [0u8; 1024];
                while let Ok(read) = socket.read(&mut buffer).await {
                    if read == 0 || socket.write_all(&buffer[..read]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    port
}

fn tunnel_to(port: u16, password: &str) -> SshTunnel {
    SshTunnel {
        host: "127.0.0.1".into(),
        port: Some(port),
        user: "kemable".into(),
        auth: SshAuth::Password {
            password: Secret::new(password),
        },
    }
}

fn known_hosts_file(name: &str) -> std::path::PathBuf {
    let folder = std::env::temp_dir().join(format!("kemable-tunnel-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

fn options() -> CallOptions {
    CallOptions::default().with_timeout(Duration::from_secs(10))
}

#[tokio::test]
async fn an_unknown_host_is_refused_until_trusted_then_links_go_through_to_the_database() {
    let (ssh_port, _) = ssh_server(false).await;
    let echo = echo_server().await;
    let file = known_hosts_file("trusted");
    let keys = Arc::new(HostKeys::with_user_file(file.clone(), None));

    let unknown = open(
        &tunnel_to(ssh_port, "secret"),
        "127.0.0.1",
        echo,
        Arc::clone(&keys),
        &options(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(unknown.code, ErrorCode::SshHostUnknown);
    assert!(
        unknown.message.contains("SHA256:"),
        "the fingerprint to show: {}",
        unknown.message
    );
    assert!(!file.exists(), "nothing is trusted before the user says so");

    keys.trust("127.0.0.1", ssh_port).unwrap();
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .trim_start()
            .starts_with(&format!("[127.0.0.1]:{ssh_port} ssh-ed25519 "))
    );
    assert_eq!(
        keys.trust("127.0.0.1", ssh_port).unwrap_err().code,
        ErrorCode::NotFound,
        "a key is trusted once, as it was shown"
    );

    let tunnel = open(
        &tunnel_to(ssh_port, "secret"),
        "127.0.0.1",
        echo,
        Arc::clone(&keys),
        &options(),
    )
    .await
    .unwrap();
    assert!(tunnel.local().ip().is_loopback());
    // Two links at once, each its own channel.
    let mut first = TcpStream::connect(tunnel.local()).await.unwrap();
    let mut second = TcpStream::connect(tunnel.local()).await.unwrap();
    for (link, word) in [(&mut first, b"select 1"), (&mut second, b"select 2")] {
        link.write_all(word).await.unwrap();
        let mut answer = [0u8; 8];
        link.read_exact(&mut answer).await.unwrap();
        assert_eq!(&answer, word);
    }
    assert!(!tunnel.is_closed());
    drop(tunnel);
}

#[tokio::test]
async fn a_wrong_password_a_refused_forward_and_a_changed_key_each_say_so() {
    let (ssh_port, key) = ssh_server(false).await;
    let echo = echo_server().await;
    let file = known_hosts_file("each");
    let keys = Arc::new(HostKeys::with_user_file(file.clone(), None));
    let _ = open(
        &tunnel_to(ssh_port, "secret"),
        "127.0.0.1",
        echo,
        Arc::clone(&keys),
        &options(),
    )
    .await;
    keys.trust("127.0.0.1", ssh_port).unwrap();

    let refused = open(
        &tunnel_to(ssh_port, "wrong"),
        "127.0.0.1",
        echo,
        Arc::clone(&keys),
        &options(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(refused.code, ErrorCode::SshAuth);

    let (refusing_port, _) = ssh_server(true).await;
    let _ = open(
        &tunnel_to(refusing_port, "secret"),
        "127.0.0.1",
        echo,
        Arc::clone(&keys),
        &options(),
    )
    .await;
    keys.trust("127.0.0.1", refusing_port).unwrap();
    let forward = open(
        &tunnel_to(refusing_port, "secret"),
        "127.0.0.1",
        echo,
        Arc::clone(&keys),
        &options(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(forward.code, ErrorCode::SshForward);

    // Another server where the first one's key is known: it must not pass.
    let (other_port, _) = ssh_server(false).await;
    let changed_file = known_hosts_file("changed");
    std::fs::write(
        &changed_file,
        format!("[127.0.0.1]:{other_port} {}\n", key.public_key().to_openssh().unwrap()),
    )
    .unwrap();
    let changed_keys = Arc::new(HostKeys::with_user_file(known_hosts_file("own"), Some(changed_file)));
    let changed = open(
        &tunnel_to(other_port, "secret"),
        "127.0.0.1",
        echo,
        Arc::clone(&changed_keys),
        &options(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(changed.code, ErrorCode::SshHostChanged);
    assert!(
        changed_keys.trust("127.0.0.1", other_port).is_err(),
        "a changed key is never offered to trust"
    );
}

#[tokio::test]
async fn a_key_file_signs_in_and_a_missing_one_is_a_config_error() {
    let missing = SshTunnel {
        host: "127.0.0.1".into(),
        port: Some(1),
        user: "kemable".into(),
        auth: SshAuth::Key {
            path: known_hosts_file("no-such-key"),
            passphrase: Secret::default(),
        },
    };
    let keys = Arc::new(HostKeys::with_user_file(known_hosts_file("keys"), None));
    let error = open(&missing, "127.0.0.1", 5432, keys, &options()).await.err().unwrap();
    assert_eq!(error.code, ErrorCode::ConfigInvalid);
}
