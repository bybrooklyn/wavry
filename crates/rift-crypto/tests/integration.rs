//! Integration tests for encrypted connection over UDP.

use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::timeout;

use rift_crypto::connection::{handshake_type, SecureClient, SecureServer};

/// Test full Noise XX handshake over UDP sockets
#[tokio::test]
async fn test_encrypted_handshake_over_udp() {
    // Bind server socket
    let server_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let server_addr = server_socket.local_addr().unwrap();

    // Bind client socket
    let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = client_socket.local_addr().unwrap();

    // Spawn server task
    let server_handle = tokio::spawn(async move {
        let mut server = SecureServer::new().unwrap();
        let mut buf = vec![0u8; 4096];

        // Receive msg1
        let (len, peer) = server_socket.recv_from(&mut buf).await.unwrap();

        // Process and send msg2
        let msg2 = server.process_client_hello(&buf[..len]).unwrap();
        server_socket.send_to(&msg2, peer).await.unwrap();

        // Receive msg3
        let (len, _peer) = server_socket.recv_from(&mut buf).await.unwrap();
        server.process_client_finish(&buf[..len]).unwrap();

        assert!(server.is_established());

        // Receive encrypted data
        let (len, _) = server_socket.recv_from(&mut buf).await.unwrap();
        let packet_id = u64::from_le_bytes(buf[0..8].try_into().unwrap());
        let ciphertext = &buf[8..len];
        let plaintext = server.decrypt(packet_id, ciphertext).unwrap();

        assert_eq!(&plaintext, b"Hello from client!");

        // Send encrypted response
        let packet_id = 42u64;
        let ciphertext = server.encrypt(packet_id, b"Hello from server!").unwrap();
        let mut response = Vec::new();
        response.extend_from_slice(&packet_id.to_le_bytes());
        response.extend_from_slice(&ciphertext);
        server_socket.send_to(&response, client_addr).await.unwrap();

        "server_ok"
    });

    // Client side
    let mut client = SecureClient::new().unwrap();

    // Send msg1
    let msg1 = client.start_handshake().unwrap();
    client_socket.send_to(&msg1, server_addr).await.unwrap();

    // Receive msg2
    let mut buf = vec![0u8; 4096];
    let (len, _) = timeout(Duration::from_secs(5), client_socket.recv_from(&mut buf))
        .await
        .unwrap()
        .unwrap();

    // Process msg2 and send msg3
    let msg3 = client.process_server_response(&buf[..len]).unwrap();
    client_socket.send_to(&msg3, server_addr).await.unwrap();

    assert!(client.is_established());

    // Send encrypted data
    let packet_id = 100u64;
    let ciphertext = client.encrypt(packet_id, b"Hello from client!").unwrap();
    let mut packet = Vec::new();
    packet.extend_from_slice(&packet_id.to_le_bytes());
    packet.extend_from_slice(&ciphertext);
    client_socket.send_to(&packet, server_addr).await.unwrap();

    // Receive encrypted response
    let (len, _) = timeout(Duration::from_secs(5), client_socket.recv_from(&mut buf))
        .await
        .unwrap()
        .unwrap();
    let packet_id = u64::from_le_bytes(buf[0..8].try_into().unwrap());
    let ciphertext = &buf[8..len];
    let plaintext = client.decrypt(packet_id, ciphertext).unwrap();

    assert_eq!(&plaintext, b"Hello from server!");

    // Wait for server
    let result = server_handle.await.unwrap();
    assert_eq!(result, "server_ok");
}

/// Test multiple encrypted packets in sequence
#[tokio::test]
async fn test_encrypted_packet_sequence() {
    let mut client = SecureClient::new().unwrap();
    let mut server = SecureServer::new().unwrap();

    // Complete handshake in-memory
    let msg1 = client.start_handshake().unwrap();
    let msg2 = server.process_client_hello(&msg1).unwrap();
    let msg3 = client.process_server_response(&msg2).unwrap();
    server.process_client_finish(&msg3).unwrap();

    assert!(client.is_established());
    assert!(server.is_established());

    // Send multiple packets from client to server
    for i in 0..10 {
        let msg = format!("Message {}", i);
        let packet_id = i as u64;
        let ciphertext = client.encrypt(packet_id, msg.as_bytes()).unwrap();

        let decrypted = server.decrypt(packet_id, &ciphertext).unwrap();
        assert_eq!(decrypted, msg.as_bytes());
    }

    // Send multiple packets from server to client
    for i in 0..10 {
        let msg = format!("Response {}", i);
        let packet_id = i as u64;
        let ciphertext = server.encrypt(packet_id, msg.as_bytes()).unwrap();

        let decrypted = client.decrypt(packet_id, &ciphertext).unwrap();
        assert_eq!(decrypted, msg.as_bytes());
    }
}

/// Test out-of-order packet decryption (UDP reordering)
#[tokio::test]
async fn test_out_of_order_decryption() {
    let mut client = SecureClient::new().unwrap();
    let mut server = SecureServer::new().unwrap();

    // Complete handshake
    let msg1 = client.start_handshake().unwrap();
    let msg2 = server.process_client_hello(&msg1).unwrap();
    let msg3 = client.process_server_response(&msg2).unwrap();
    server.process_client_finish(&msg3).unwrap();

    // Encrypt three packets
    let ct0 = client.encrypt(0, b"packet 0").unwrap();
    let ct1 = client.encrypt(1, b"packet 1").unwrap();
    let ct2 = client.encrypt(2, b"packet 2").unwrap();

    // Decrypt out of order
    assert_eq!(server.decrypt(2, &ct2).unwrap(), b"packet 2");
    assert_eq!(server.decrypt(0, &ct0).unwrap(), b"packet 0");
    assert_eq!(server.decrypt(1, &ct1).unwrap(), b"packet 1");
}
