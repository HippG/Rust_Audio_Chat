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
use std::sync::Mutex;
mod interface;
mod tls_skip;
use tls_skip::SkipServerVerification;
use interface::{AppState, ClientInfo};
use std::io::{self, Write};

const SAMPLE_RATE: u32 = 48000;
const FRAME_SIZE_MS: u32 = 20;
const FRAME_SIZE: usize = (SAMPLE_RATE as usize * FRAME_SIZE_MS as usize) / 1000;

// audio processor
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



#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // on récupère l'ip serveur passé en argument, sinon IP par défaut du serveur aws
    let args: Vec<String> = std::env::args().collect();
    let server_addr = if args.len() > 1 {
        args[1].clone()
    } else {
        "13.37.250.113:8047".to_string()
    };
    
    // same pour le port
    let web_port = if args.len() > 2 {
        args[2].parse().unwrap_or(8047)
    } else {
        8047
    };

    // demander le pseudonymne
    let mut buffer = String::new();
    while buffer.trim().is_empty() {
        print!("Enter your pseudonym: ");
        io::stdout().flush()?;
        io::stdin().read_line(&mut buffer)?;
    }
    let my_name = buffer.trim().to_string();

    // client ID random associé aux paquets audio
    let mut rng = rand::thread_rng();
    let my_id: u64 = rng.gen();
    println!("Client ID: {:}", my_id);

    println!("Connecting to {}", server_addr);

    // QUIC config, skip TLS verification
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
    println!("Connected to server");

    // Initialize App State
    let app_state = AppState {
        clients: Arc::new(Mutex::new(Vec::new())),
        is_muted: Arc::new(Mutex::new(false)),
    };

    // Start Web Server
    let web_state = app_state.clone();
    tokio::spawn(async move {
        interface::start_web_server(web_state, web_port).await;
    });

    // Send Indentify Packet
    // Packet Type 0x02: [Type(1)] + [ID(8)] + [Name(UTF8)]
    let name_bytes = my_name.as_bytes();
    let mut id_packet = Vec::with_capacity(1 + 8 + name_bytes.len());
    id_packet.push(0x02);
    id_packet.extend_from_slice(&my_id.to_le_bytes());
    id_packet.extend_from_slice(name_bytes);
    connection.send_datagram(id_packet.into())?;

    // JACK config
    let (client, _status) = Client::new("AudioClient", ClientOptions::NO_START_SERVER)?; // on redémarre pas le serveur pw qui tourne au démarrage linux

    let in_port_name = "input";
    let out_port_name = "output";

    let in_port = client.register_port(in_port_name, AudioIn::default())?;
    let out_port = client.register_port(out_port_name, AudioOut::default())?;

    let capture_rb = HeapRb::<f32>::new(48000 * 2);
    let (mut cap_prod, mut cap_cons) = capture_rb.split();

    let playback_rb = HeapRb::<f32>::new(48000 * 2);
    let (mut play_prod, play_cons) = playback_rb.split();

    let process = AudioProcessor { 
        in_port, 
        out_port, 
        producer: cap_prod, 
        consumer: play_cons 
    };
    let active_client = client.activate_async((), process)?;

    let in_port_name = "AudioClient:input";
    let out_port_name = "AudioClient:output";

    // Capture
    let system_captures = active_client
        .as_client()
        .ports(Some("system:capture_.*"), None, PortFlags::IS_OUTPUT);

    for (i, port_name) in system_captures.iter().enumerate() {
        if i >= 1 { break; }
        println!("Connecting {} -> {}", port_name, in_port_name);
        active_client
            .as_client()
            .connect_ports_by_name(port_name, in_port_name)?;
    }

    // Playback
    let system_playbacks = active_client
        .as_client()
        .ports(Some("system:playback_.*"), None, PortFlags::IS_INPUT);

    for port_name in system_playbacks.iter() {
        println!("Connecting {} -> {}", out_port_name, port_name);
        active_client
            .as_client()
            .connect_ports_by_name(out_port_name, port_name)?;
    }

    // opus encoder/decoder
    let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip).unwrap();
    let mut decoder = Decoder::new(SAMPLE_RATE, Channels::Mono).unwrap();
    
    let mut pcm_in = vec![0.0; FRAME_SIZE];
    let mut pcm_out = vec![0.0; FRAME_SIZE];
    let mut encode_buf = vec![0u8; 1500]; // 1500 bytes, taille du paquet réseau

    // split de la connection pour send/recv
    let conn_send = connection.clone();
    let conn_recv = connection.clone();

    // SEND
    let app_state_send = app_state.clone();
    let send_task = tokio::spawn(async move {
        let app_state = app_state_send; // capture name
        println!("Audio capture started");
        loop {
            // Vérif si on a assez de samples
            if cap_cons.occupied_len() >= FRAME_SIZE {
                cap_cons.pop_slice(&mut pcm_in);
                match encoder.encode_float(&pcm_in, &mut encode_buf) {
                    Ok(len) => {
                        // Ajout du client ID avant le paquet audio
                        // Ajout du client ID avant le paquet audio
                        // Packet Type 0x01: [Type(1)] + [ID(8)] + [Opus Data]
                        
                        // Check Mute
                        let is_muted = *app_state.is_muted.lock().unwrap();
                        
                        if !is_muted {
                            let mut final_packet = Vec::with_capacity(1 + 8 + len);
                            final_packet.push(0x01); // Audio Type
                            final_packet.extend_from_slice(&my_id.to_le_bytes()); // ID
                            final_packet.extend_from_slice(&encode_buf[..len]);   // Audio
                            // Envoi du paquet
                            let _ = conn_send.send_datagram(final_packet.into());
                        }
                    }
                    Err(e) => eprintln!("Encode error: {}", e),
                }
            } else {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
    });

    // RECEIVE
    let recv_task = tokio::spawn(async move {
        println!("Playback started");
        loop {
            match conn_recv.read_datagram().await {
                Ok(data) => {
                    if data.len() > 0 {
                        let packet_type = data[0];
                        match packet_type {
                            0x01 => { // Audio
                                if data.len() > 9 {
                                    let sender_id = LittleEndian::read_u64(&data[1..9]);
                                    if sender_id != my_id {
                                        let audio_data = &data[9..];
                                        match decoder.decode_float(audio_data, &mut pcm_out, false) {
                                            Ok(len) => {
                                                play_prod.push_slice(&pcm_out[..len]);
                                            }
                                            Err(e) => eprintln!("Decode error: {}", e),
                                        }
                                    }
                                }
                            }
                            0x03 => { // Client List
                                // Payload is JSON
                                if let Ok(list) = serde_json::from_slice::<Vec<ClientInfo>>(&data[1..]) {
                                    *app_state.clients.lock().unwrap() = list;
                                }
                            }
                            _ => {}
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
    Ok(())
}
