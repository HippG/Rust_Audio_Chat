// Client1/main.rs (Émetteur)
use std::error::Error;
use std::sync::Arc;
use quinn::Endpoint;

#[derive(Debug)]
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let server_addr = "127.0.0.1:8047";
    
    // Configurer le client QUIC
    let mut client_crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
        .with_no_client_auth();
    client_crypto.alpn_protocols = vec![b"relay".to_vec()];
    
    let client_config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)?
    ));
    
    let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
    endpoint.set_default_client_config(client_config);
    
    println!("📡 Connexion au serveur {}...", server_addr);
    let connection = endpoint
        .connect(server_addr.parse()?, "localhost")?
        .await?;
    
    println!("✓ Connecté au serveur !");
    
    // Attendre un peu pour s'assurer que le serveur est prêt
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    
    // Ouvrir un stream et envoyer un message
    println!("📤 Envoi du message...");
    let (mut send, _recv) = connection.open_bi().await?;
    
    let message = b"Bonjour depuis le Client 1 !";
    send.write_all(message).await?;
    send.finish()?;
    
    println!("✓ Message envoyé: {:?}", String::from_utf8_lossy(message));
    
    // Garder la connexion ouverte un moment
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    
    connection.close(0u32.into(), b"bye");
    endpoint.wait_idle().await;
    
    println!("✓ Déconnexion propre");
    
    Ok(())
}