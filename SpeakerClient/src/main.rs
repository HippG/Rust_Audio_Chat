use jack::*;
use opus::{Decoder, Channels};
use ringbuf::{HeapRb};
use ringbuf::traits::*;
use std::sync::Arc;
use std::time::{Duration};
use quinn::{ClientConfig, Endpoint, TransportConfig};
use std::error::Error;
use std::collections::VecDeque;

const SAMPLE_RATE: u32 = 48000;
const FRAME_SIZE_MS: u32 = 20; // 20ms
const FRAME_SIZE: usize = (SAMPLE_RATE as usize * FRAME_SIZE_MS as usize) / 1000; // 960 samples

struct AudioPlayback<C> {
    out_port: Port<AudioOut>,
    consumer: C,
}

impl<C> ProcessHandler for AudioPlayback<C>
where
    C: Consumer<Item = f32> + Send
{
    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        let out_slice = self.out_port.as_mut_slice(ps);
        for i in 0..out_slice.len() {
            out_slice[i] = self.consumer.try_pop().unwrap_or(0.0);
        }
        Control::Continue
    }
}

#[derive(Debug)]
struct SkipServerVerification;
impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self, _end_entity: &rustls::pki_types::CertificateDer<'_>, _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>, _ocsp_response: &[u8], _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self, _message: &[u8], _cert: &rustls::pki_types::CertificateDer<'_>, _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self, _message: &[u8], _cert: &rustls::pki_types::CertificateDer<'_>, _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![rustls::SignatureScheme::RSA_PSS_SHA256, rustls::SignatureScheme::RSA_PSS_SHA384, rustls::SignatureScheme::RSA_PSS_SHA512, rustls::SignatureScheme::ED25519]
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

    println!("🚀 SpeakerClient connecting to {}", server_addr);

    // QUIC Configuration
    // Correct chain: builder -> dangerous -> verifier -> no auth (build)
    let mut crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
        .with_no_client_auth();
    
    crypto.alpn_protocols = vec![b"relay".to_vec()];

    let mut client_config = ClientConfig::new(Arc::new(quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?));
    
    let mut transport = TransportConfig::default();
    transport.datagram_receive_buffer_size(Some(1024 * 64)); 
    transport.datagram_send_buffer_size(1024 * 64);
    client_config.transport_config(Arc::new(transport));

    let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())?;
    endpoint.set_default_client_config(client_config);

    let connection = endpoint.connect(server_addr.parse()?, "localhost")?.await?;
    println!("✅ Connected via QUIC");

    // JACK
    let (client, _status) = Client::new("SpeakerClient", ClientOptions::NO_START_SERVER)?;
    let out_port = client.register_port("output", AudioOut::default())?;

    let ring = HeapRb::<f32>::new(48000 * 2);
    let (mut producer, consumer) = ring.split();

    let process = AudioPlayback { out_port, consumer };
    let active_client = client.activate_async((), process)?;

    // Decoder
    let mut decoder = Decoder::new(SAMPLE_RATE, Channels::Mono).unwrap();
    let mut pcm_out = vec![0.0; FRAME_SIZE];

    // Jitter Buffer
    let mut packet_queue: VecDeque<Vec<u8>> = VecDeque::new();
    let mut buffering = true;
    let target_buffer_time = Duration::from_secs(5);
    let target_packets = 250; 

    println!("⏳ Buffering {}s of audio...", target_buffer_time.as_secs());

    loop {
        match connection.read_datagram().await {
            Ok(data) => {
                let packet = data.to_vec();
                packet_queue.push_back(packet);

                if buffering {
                    if packet_queue.len() >= target_packets {
                        buffering = false;
                        println!("▶️ Buffering complete. Playing...");
                    }
                } else {
                    // Playback Mode
                    // On consomme la queue en fonction de la place dans le ringbuf
                    // Mais ici on est piloté par l'ARRIVÉE des paquets (event loop read_datagram).
                    // Si on reçoit plus vite qu'on joue, la queue grandit.
                    // Si on reçoit moins vite, la queue se vide.
                    
                    // Pour chaque paquet reçu, on essaie d'en décoder un de la queue et de le push.
                    // (Ou plusieurs si on a accumulé).
                    
                    while !packet_queue.is_empty() && producer.vacant_len() >= FRAME_SIZE {
                        let pkt = packet_queue.pop_front().unwrap();
                        match decoder.decode_float(&pkt, &mut pcm_out, false) {
                            Ok(len) => {
                                producer.push_slice(&pcm_out[..len]);
                            }
                            Err(e) => eprintln!("Decode error: {}", e),
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Connection lost: {}", e);
                break;
            }
        }
    }
    
    // active_client.deactivate()?;
    Ok(())
}
