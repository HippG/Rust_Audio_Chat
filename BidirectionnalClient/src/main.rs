use jack::*;
use opus::{Encoder, Decoder, Channels, Application};
use ringbuf::{HeapRb};
use ringbuf::traits::*;
use std::sync::Arc;
use std::time::Duration;
use std::collections::VecDeque;
use quinn::{ClientConfig, Endpoint, TransportConfig};
use std::error::Error;
use rand::Rng;
use byteorder::{ByteOrder, LittleEndian};

const SAMPLE_RATE: u32 = 48000;
const FRAME_SIZE_MS: u32 = 20;
const FRAME_SIZE: usize = (SAMPLE_RATE as usize * FRAME_SIZE_MS as usize) / 1000;

// Unified Audio Processor
struct AudioProcessor<P, C> {
    in_port: Port<AudioIn>,
    out_port: Port<AudioOut>,
    producer: P,
    consumer: C,
}

impl<P, C> ProcessHandler for AudioProcessor<P, C>
where
    P: Producer<Item = f32> + Send,
    C: Consumer<Item = f32> + Send
{
    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        // Capture
        let in_slice = self.in_port.as_slice(ps);
        let _ = self.producer.push_slice(in_slice);

        // Playback
        let out_slice = self.out_port.as_mut_slice(ps);
        for i in 0..out_slice.len() {
            out_slice[i] = self.consumer.try_pop().unwrap_or(0.0);
        }
        Control::Continue
    }
}

// Verification (Allow all including ECDSA)
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

    // Generate unique Client ID
    let mut rng = rand::thread_rng();
    let my_id: u64 = rng.gen();
    println!("🆔 Client ID: {:016X}", my_id);

    println!("🚀 Connecting to {}", server_addr);

    // QUIC Config
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
    let (client, _status) = Client::new("BidirectionnalClient", ClientOptions::NO_START_SERVER)?;
    let in_port = client.register_port("input", AudioIn::default())?;
    let out_port = client.register_port("output", AudioOut::default())?;

    let capture_rb = HeapRb::<f32>::new(48000 * 2);
    let (mut cap_prod, mut cap_cons) = capture_rb.split();

    let playback_rb = HeapRb::<f32>::new(48000 * 2);
    let (mut play_prod, play_cons) = playback_rb.split();

    let in_port_name = in_port.name()?.to_string();
    let out_port_name = out_port.name()?.to_string();

    let process = AudioProcessor { 
        in_port, 
        out_port, 
        producer: cap_prod, 
        consumer: play_cons 
    };
    let active_client = client.activate_async((), process)?;

    // Auto-connect ports
    // Connect System Capture -> Client Input
    let system_captures = active_client.as_client().ports(Some("system:capture_.*"), None, PortFlags::empty());
    for (i, port_name) in system_captures.iter().enumerate() {
        if i >= 1 { break; } 
        println!("🔗 Connecting {} -> {}", port_name, in_port_name);
        if let Err(e) = active_client.as_client().connect_ports_by_name(port_name, &in_port_name) {
            eprintln!("Failed to connect input: {}", e);
        }
    }

    // Connect Client Output -> System Playback
    let system_playbacks = active_client.as_client().ports(Some("system:playback_.*"), None, PortFlags::empty());
    for port_name in system_playbacks.iter() {
        println!("🔗 Connecting {} -> {}", out_port_name, port_name);
        if let Err(e) = active_client.as_client().connect_ports_by_name(&out_port_name, port_name) {
            eprintln!("Failed to connect output: {}", e);
        }
    }

    // Audio Loop
    let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip).unwrap();
    let mut decoder = Decoder::new(SAMPLE_RATE, Channels::Mono).unwrap();
    
    let mut pcm_in = vec![0.0; FRAME_SIZE];
    let mut pcm_out = vec![0.0; FRAME_SIZE];
    let mut encode_buf = vec![0u8; 1500];

    // Split connection for send/recv tasks
    let conn_send = connection.clone();
    let conn_recv = connection.clone();

    // 1. Send Task (Capture -> Encode -> ID -> Send)
    let send_task = tokio::spawn(async move {
        println!("🎙️ Capture started");
        loop {
            // Check if we have enough samples
            if cap_cons.occupied_len() >= FRAME_SIZE {
                cap_cons.pop_slice(&mut pcm_in);
                match encoder.encode_float(&pcm_in, &mut encode_buf) {
                    Ok(len) => {
                        // Prepend ID (8 bytes) + Packet
                        let mut final_packet = Vec::with_capacity(8 + len);
                        final_packet.extend_from_slice(&my_id.to_le_bytes()); // ID
                        final_packet.extend_from_slice(&encode_buf[..len]);   // Audio

                        let _ = conn_send.send_datagram(final_packet.into());
                    }
                    Err(e) => eprintln!("Encode error: {}", e),
                }
            } else {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
    });

    // 2. Recv Task (Recv -> Parse ID -> Filter -> Decode -> Play)
    let recv_task = tokio::spawn(async move {
        println!("🔊 Playback started");
        loop {
            match conn_recv.read_datagram().await {
                Ok(data) => {
                    if data.len() > 8 {
                        let sender_id = LittleEndian::read_u64(&data[0..8]);
                        
                        if sender_id != my_id {
                            // Valid packet from DIFFERENT user
                            let audio_data = &data[8..];
                            match decoder.decode_float(audio_data, &mut pcm_out, false) {
                                Ok(len) => {
                                    play_prod.push_slice(&pcm_out[..len]);
                                }
                                Err(e) => eprintln!("Decode error: {}", e),
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Rx Error: {}", e);
                    break;
                }
            }
        }
    });

    let _ = tokio::join!(send_task, recv_task);
    // active_client.deactivate()?;
    Ok(())
}
