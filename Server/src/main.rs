    use quinn::{Endpoint, ServerConfig};
    use std::error::Error;
    use std::net::SocketAddr;
    use std::time::Duration;
    use std::sync::{Arc, Mutex};
    use tokio::sync::broadcast;
    use std::collections::HashMap;
    use serde::{Serialize, Deserialize};
    use byteorder::{ByteOrder, LittleEndian};

    #[derive(Clone, Serialize, Deserialize, Debug)]
    pub struct ClientInfo {
        pub id: u64,
        pub name: String,
    }

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

        // configure le channel quic 
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_idle_timeout(Some(quinn::VarInt::from_u32(5000).into())); // 5s timeout pour deconnecter le client
        transport_config.keep_alive_interval(Some(Duration::from_secs(2)));
        transport_config.datagram_receive_buffer_size(Some(1024 * 64));
        transport_config.datagram_send_buffer_size(1024 * 64);
        
        server_config.transport_config(Arc::new(transport_config));

        let endpoint = Endpoint::server(server_config, addr)?;
        println!("Serveur QUIC relay (Datagrams) en écoute sur {}", addr);

    // canal de broadcast pour envoyer les messages entre clients

    let (tx, _rx) = broadcast::channel::<Vec<u8>>(1000);

        // dictionnaire (hashmap) des clients connectés pour l'interface web
        let clients_map: Arc<Mutex<HashMap<SocketAddr, ClientInfo>>> = Arc::new(Mutex::new(HashMap::new()));

        // boucle d'acceptation des clients
        loop {
            if let Some(incoming) = endpoint.accept().await {
                let tx = tx.clone();
                let mut rx = tx.subscribe();
                let clients_map = clients_map.clone();

                tokio::spawn(async move {
                    match incoming.await {
                        Ok(connection) => {
                            let remote = connection.remote_address();
                            println!("Nouveau client connecté: {}", remote);

                            let conn_recv = connection.clone();
                            let tx_clone = tx.clone();
                            let clients_map_recv = clients_map.clone();
                            let remote_addr = remote;

                            // réception 
                            let recv_fut = async move {
                                loop {
                                    match conn_recv.read_datagram().await {
                                        Ok(data) => {
                                            if data.len() > 0 {
                                                match data[0] {
                                                    0x01 => { // paquet audio
                                                        let _ = tx_clone.send(data.to_vec());
                                                    }
                                                    0x02 => { // paquet d'identification
                                                        if data.len() > 9 {
                                                            let id = LittleEndian::read_u64(&data[1..9]);
                                                            let name = String::from_utf8_lossy(&data[9..]).to_string();
                                                            
                                                            {
                                                                let mut map = clients_map_recv.lock().unwrap();
                                                                map.insert(remote_addr, ClientInfo { id, name: name.clone() });
                                                            }
                                                            println!("Client identifié: {} ({})", name, id);

                                                            // envoie liste des clients
                                                            let clients: Vec<ClientInfo> = clients_map_recv.lock().unwrap().values().cloned().collect();
                                                            if let Ok(json) = serde_json::to_vec(&clients) {
                                                                let mut packet = vec![0x03];
                                                                packet.extend(json);
                                                                let _ = tx_clone.send(packet);
                                                            }
                                                        }
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("Client {} déconnecté (lecture): {}", remote_addr, e);
                                            break;
                                        }
                                    }
                                }
                            };

                            // envoie
                            let conn_send = connection.clone();
                            let send_fut = async move {
                                while let Ok(data) = rx.recv().await {
                                    conn_send.send_datagram(data.into()) 
                                }
                            };

                            // attends qu'un des deux s'arrête
                            tokio::select! {
                                _ = recv_fut => {},
                                _ = send_fut => {},
                            }

                            // clean le client qui a déco
                            println!("Clean client: {}", remote);
                            {
                                let mut map = clients_map.lock().unwrap();
                                map.remove(&remote);
                            }
                            
                            // met à jour la liste des clients et envoie en broadcast
                            let clients: Vec<ClientInfo> = clients_map.lock().unwrap().values().cloned().collect();
                            if let Ok(json) = serde_json::to_vec(&clients) {
                                let mut packet = vec![0x03];
                                packet.extend(json);
                                let _ = tx.send(packet);
                            }
                        }
                        Err(e) => {
                            eprintln!("Erreur connexion: {}", e);
                        }
                    }
                });
            }
        }
    }
