// Server/main.rs
use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;
use quinn::{Endpoint, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let addr: SocketAddr = "127.0.0.1:8047".parse()?;
    
    // Générer un certificat auto-signé pour le développement
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let key = cert.key_pair.serialize_der();
    let cert_der = cert.cert.der().to_vec();
    
    // Configurer le serveur QUIC
    let mut server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(cert_der)],
            rustls::pki_types::PrivateKeyDer::try_from(key)?
        )?;
    server_crypto.alpn_protocols = vec![b"relay".to_vec()];
    
    let mut server_config = ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)?
    ));
    
    let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
    transport_config.max_concurrent_uni_streams(0_u8.into());
    
    let endpoint = Endpoint::server(server_config, addr)?;
    println!("✓ Serveur QUIC relay en écoute sur {}", addr);
    
    // Canal de broadcast pour relayer les messages entre clients
    let (tx, _rx) = broadcast::channel::<Vec<u8>>(100);
    
    // Boucle d'acceptation des clients
    loop {
        if let Some(incoming) = endpoint.accept().await {
            let tx = tx.clone();
            let mut rx = tx.subscribe();
            
            tokio::spawn(async move {
                match incoming.await {
                    Ok(connection) => {
                        let remote = connection.remote_address();
                        println!("✓ Nouveau client connecté: {}", remote);
                        
                        // Gérer les streams du client
                        loop {
                            match connection.accept_bi().await {
                                Ok((mut send, mut recv)) => {
                                    let tx = tx.clone();
                                    let mut rx = rx.resubscribe();
                                    
                                    // Tâche de réception depuis le client
                                    let recv_task = tokio::spawn(async move {
                                        loop {
                                            match recv.read_to_end(1024 * 64).await {
                                                Ok(data) => {
                                                    if !data.is_empty() {
                                                        println!("← Reçu {} octets de {}", data.len(), remote);
                                                        // Broadcaster le message à tous les autres clients
                                                        let _ = tx.send(data);
                                                    }
                                                    break;
                                                }
                                                Err(e) => {
                                                    eprintln!("Erreur lecture stream: {}", e);
                                                    break;
                                                }
                                            }
                                        }
                                    });
                                    
                                    // Tâche d'envoi vers le client
                                    let send_task = tokio::spawn(async move {
                                        while let Ok(data) = rx.recv().await {
                                            println!("→ Envoi de {} octets à {}", data.len(), remote);
                                            if let Err(e) = send.write_all(&data).await {
                                                eprintln!("Erreur envoi: {}", e);
                                                break;
                                            }
                                            send.finish().ok();
                                        }
                                    });
                                    
                                    // Attendre que les deux tâches se terminent
                                    let _ = tokio::join!(recv_task, send_task);
                                }
                                Err(e) => {
                                    eprintln!("Erreur accept_bi: {}", e);
                                    break;
                                }
                            }
                        }
                        
                        println!("✗ Client déconnecté: {}", remote);
                    }
                    Err(e) => {
                        eprintln!("Erreur connexion: {}", e);
                    }
                }
            });
        }
    }
}