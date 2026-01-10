use jack::*;
use opus::{Encoder, Channels, Application};
use ringbuf::{HeapRb};
use ringbuf::traits::*;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use quinn::{ClientConfig, Endpoint, TransportConfig};
use std::error::Error;

const SAMPLE_RATE: u32 = 48000;
const FRAME_SIZE_MS: u32 = 20; // 20ms
const FRAME_SIZE: usize = (SAMPLE_RATE as usize * FRAME_SIZE_MS as usize) / 1000; // 960 samples

struct AudioCapture<P> {
    in_port: Port<AudioIn>,
    producer: P,
}

impl<P> ProcessHandler for AudioCapture<P>
where
    P: Producer<Item = f32> + Send
{
    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        let in_slice = self.in_port.as_slice(ps);
        let _ = self.producer.push_slice(in_slice);
        Control::Continue
    }
}

// Configuration Rustls simple (skip verify)
// Configuration Rustls simple (skip verify)
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
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
        ]
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let server_addr = if args.len() > 1 {
        args[1].clone()
    } else {
        "127.0.0.1:8047".to_string()
    };

    println!("🚀 MicClient connecting to {}", server_addr);

    // QUIC Configuration
    let mut crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
        .with_no_client_auth();
    crypto.alpn_protocols = vec![b"relay".to_vec()];

    let mut client_config = ClientConfig::new(Arc::new(quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?));
    
    // Configurer transport pour Datagrams
    let mut transport = TransportConfig::default();
    transport.datagram_receive_buffer_size(Some(1024 * 64)); // Pas strictement nécessaire pour Sender, mais bon
    transport.datagram_send_buffer_size(1024 * 64);
    client_config.transport_config(Arc::new(transport));

    let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())?;
    endpoint.set_default_client_config(client_config);

    // Connect
    let connection = endpoint.connect(server_addr.parse()?, "localhost")?.await?;
    println!("✅ Connected via QUIC");

    // JACK init
    let (client, _status) = Client::new("MicClient", ClientOptions::NO_START_SERVER)?;
    let in_port = client.register_port("input", AudioIn::default())?;

    let ring = HeapRb::<f32>::new(48000 * 2);
    let (producer, mut consumer) = ring.split();

    let process = AudioCapture { in_port, producer };
    let active_client = client.activate_async((), process)?; // Handler is dropped when ActiveClient is dropped? No, ownership moved.

    // Encoding Loop
    let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip).unwrap();
    let mut pcm_in = vec![0.0; FRAME_SIZE];
    let mut out_bytes = vec![0u8; 1000];

    // Utilisation de spawn_blocking pour ne pas bloquer la runtime tokio avec le sleep/encode loop?
    // Ou juste un loop async avec sleep async.
    // Mais on lit du ringbuf qui est sync... 
    // RingBuf consumer : pop_slice est non bloquant (retourne ce qui est dispo ou rien, enfin on check len).
    // On peut faire ça dans la task principale.
    
    println!("🎙️ Streaming audio...");

    loop {
        if consumer.occupied_len() >= FRAME_SIZE {
            consumer.pop_slice(&mut pcm_in);
            match encoder.encode_float(&pcm_in, &mut out_bytes) {
                Ok(len) => {
                    let packet = out_bytes[..len].to_vec();
                    // Envoyer via QUIC Datagram
                    let _ = connection.send_datagram(packet.into()); 
                    // Ignore errors (fire and forget for audio)
                }
                Err(e) => eprintln!("Encode error: {}", e),
            }
        } else {
            // Petit sleep pour ne pas burn CPU
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }

    // active_client.deactivate()?; // Unreachable
    // Ok(()) 
}
