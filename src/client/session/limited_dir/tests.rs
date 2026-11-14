use crate::client::SftpSession;
use crate::protocol::{File, FileAttributes, Handle, Name, Packet, StatusCode, Version};
use bytes::Bytes;
use tokio::io::{AsyncWriteExt, DuplexStream};

async fn read(server: &mut DuplexStream) -> Packet {
    let mut bytes = crate::utils::read_packet(server, 262144).await.unwrap();
    Packet::try_from(&mut bytes).unwrap()
}
async fn write(server: &mut DuplexStream, packet: Packet) {
    server
        .write_all(&Bytes::try_from(packet).unwrap())
        .await
        .unwrap();
}
async fn session() -> (SftpSession, DuplexStream) {
    let (client, mut server) = tokio::io::duplex(65536);
    let peer = async {
        assert!(matches!(read(&mut server).await, Packet::Init(_)));
        write(&mut server, Version::new().into()).await;
    };
    let (client, ()) = tokio::join!(SftpSession::new(client), peer);
    (client.unwrap(), server)
}
async fn open(server: &mut DuplexStream) {
    let Packet::OpenDir(request) = read(server).await else {
        panic!("expected OPENDIR")
    };
    write(
        server,
        Handle {
            id: request.id,
            handle: "dir".into(),
        }
        .into(),
    )
    .await;
}
async fn close(server: &mut DuplexStream) {
    let Packet::Close(request) = read(server).await else {
        panic!("expected CLOSE, not another READDIR")
    };
    write(server, Packet::status(request.id, StatusCode::Ok, "", "")).await;
}

#[tokio::test]
async fn limits_stop_wire_reads_without_fetching_the_rest_of_a_large_directory() {
    for (count, bytes, expected) in [(3, 1000, 3), (1000, 8, 2), (1000, 3, 0)] {
        let (session, mut server) = session().await;
        let peer = async {
            open(&mut server).await;
            let Packet::ReadDir(request) = read(&mut server).await else {
                panic!("expected READDIR")
            };
            write(
                &mut server,
                Name {
                    id: request.id,
                    files: (0..100)
                        .map(|i| File::new(format!("{i:04}"), FileAttributes::empty()))
                        .collect(),
                }
                .into(),
            )
            .await;
            close(&mut server).await;
        };
        let (listing, ()) = tokio::join!(
            session.read_dir_bytes_limited(b"/".to_vec(), count, bytes),
            peer
        );
        let listing = listing.unwrap();
        assert!(!listing.complete);
        assert_eq!(listing.entries.count(), expected);
    }
}

#[tokio::test]
async fn eof_is_distinguished_from_a_limited_listing() {
    let (session, mut server) = session().await;
    let peer = async {
        open(&mut server).await;
        let Packet::ReadDir(request) = read(&mut server).await else {
            panic!("expected READDIR")
        };
        write(
            &mut server,
            Packet::status(request.id, StatusCode::Eof, "", ""),
        )
        .await;
        close(&mut server).await;
    };
    let (listing, ()) = tokio::join!(session.read_dir_bytes_limited(b"/".to_vec(), 10, 100), peer);
    assert!(listing.unwrap().complete);
}

#[tokio::test]
async fn cancellation_while_waiting_for_entries_closes_the_directory() {
    let (session, mut server) = session().await;
    let mut listing = Box::pin(session.read_dir_bytes_limited(b"/".to_vec(), 10, 100));
    tokio::select! {
        _ = &mut listing => panic!("listing completed too soon"),
        _ = async { open(&mut server).await; assert!(matches!(read(&mut server).await, Packet::ReadDir(_))); } => {}
    }
    drop(listing);
    tokio::time::timeout(std::time::Duration::from_secs(1), close(&mut server))
        .await
        .unwrap();
}

#[tokio::test]
async fn cancellation_before_the_open_reply_closes_the_late_handle() {
    let (session, mut server) = session().await;
    let mut listing = Box::pin(session.read_dir_bytes_limited(b"/".to_vec(), 10, 100));
    let request = tokio::select! {
        _ = &mut listing => panic!("listing completed too soon"),
        request = read(&mut server) => request,
    };
    let Packet::OpenDir(request) = request else {
        panic!("expected OPENDIR")
    };
    drop(listing);
    write(
        &mut server,
        Handle {
            id: request.id,
            handle: "late".into(),
        }
        .into(),
    )
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(1), close(&mut server))
        .await
        .unwrap();
}

#[tokio::test]
async fn directory_errors_close_the_handle() {
    let (session, mut server) = session().await;
    let peer = async {
        open(&mut server).await;
        let Packet::ReadDir(request) = read(&mut server).await else {
            panic!("expected READDIR")
        };
        write(
            &mut server,
            Packet::status(request.id, StatusCode::PermissionDenied, "", ""),
        )
        .await;
        close(&mut server).await;
    };
    let (listing, ()) = tokio::join!(session.read_dir_bytes_limited(b"/".to_vec(), 10, 100), peer);
    assert!(listing.is_err());
}
