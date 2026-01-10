use quinn::{Endpoint, ServerConfig};
use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // créer l'adresse serveur
    let addr: SocketAddr = "0.0.0.0:8047".parse()?;

    // créer un certificat auto-signé pour faire marché QUIC
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let key = cert.key_pair.serialize_der();
    let cert_der = cert.cert.der().to_vec();

    // créer le serveur QUIC avec le certificat d'avant
    let mut server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(cert_der)],
            rustls::pki_types::PrivateKeyDer::try_from(key)?,
        )?;
    server_crypto.alpn_protocols = vec![b"relay".to_vec()];

    let mut server_config = ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)?,
    ));

    let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
    // activer les datagrammes
    transport_config.datagram_receive_buffer_size(Some(1024 * 64));
    transport_config.datagram_send_buffer_size(1024 * 64);

    let endpoint = Endpoint::server(server_config, addr)?;
    println!("Serveur QUIC relay (Datagrams) en écoute sur {}", addr);

    // canal de broadcast pour envoyer les messages entre clients
    const MAX_PACKET_SIZE: usize = 1200;
    let (tx, _rx) = broadcast::channel::<Vec<u8>>(1000);

    // boucle d'acceptation des clients
    loop {
        if let Some(incoming) = endpoint.accept().await {
            let tx = tx.clone();
            let mut rx = tx.subscribe();

            tokio::spawn(async move {
                match incoming.await {
                    Ok(connection) => {
                        let remote = connection.remote_address();
                        println!("Nouveau client connecté: {}", remote);

                        // réception (Lecture des Datagrams)
                        let conn_recv = connection.clone();
                        let tx_clone = tx.clone();
                        let recv_task = tokio::spawn(async move {
                            loop {
                                match conn_recv.read_datagram().await {
                                    Ok(data) => {
                                        // Envoyer sur le channel broadcast
                                        let _ = tx_clone.send(data.to_vec());
                                    }
                                    Err(e) => {
                                        eprintln!("Client {} déconnecté (lecture): {}", remote, e);
                                        break;
                                    }
                                }
                            }
                        });

                        // tâche d'envoi (Broadcast vers Client)
                        let conn_send = connection.clone();
                        let send_task = tokio::spawn(async move {
                            while let Ok(data) = rx.recv().await {
                                if let Err(e) = conn_send.send_datagram(data.into()) {
                                    eprintln!("Erreur envoi datagram vers {}: {}", remote, e);
                                }
                            }
                        });

                        // attendre la fin de connexion
                        let _ = tokio::join!(recv_task, send_task);
                        println!("Client déconnecté: {}", remote);
                    }
                    Err(e) => {
                        eprintln!("Erreur connexion: {}", e);
                    }
                }
            });
        }
    }
}
