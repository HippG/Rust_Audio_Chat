use jack::*;
use opus::{Encoder, Channels, Application};
use ringbuf::{HeapRb};
use ringbuf::traits::*;
use std::thread;
use std::time::Duration;
use std::net::UdpSocket;

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

fn main() -> Result<(), jack::Error> {
    let (client, _status) = Client::new("MicClient", ClientOptions::NO_START_SERVER)?;
    let in_port = client.register_port("input", AudioIn::default())?;

    // Ring Buffer
    let ring = HeapRb::<f32>::new(48000 * 2); // 2 seconds buffer
    let (producer, mut consumer) = ring.split();

    let process = AudioCapture { in_port, producer };
    let active_client = client.activate_async((), process)?;

    // Network
    let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind UDP socket");
    let target_addr = "127.0.0.1:7878";

    // Encoding Thread
    thread::spawn(move || {
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip).unwrap();
        let mut pcm_in = vec![0.0; FRAME_SIZE];
        let mut out_bytes = vec![0u8; 1000];

        println!("🎙️ MicClient running. Sending to {}", target_addr);

        loop {
            if consumer.occupied_len() >= FRAME_SIZE {
                consumer.pop_slice(&mut pcm_in);
                
                match encoder.encode_float(&pcm_in, &mut out_bytes) {
                    Ok(len) => {
                        let packet = &out_bytes[..len];
                        if let Err(e) = socket.send_to(packet, target_addr) {
                            eprintln!("Send error: {}", e);
                        }
                    }
                    Err(e) => eprintln!("Encode error: {}", e),
                }
            } else {
                thread::sleep(Duration::from_millis(1));
            }
        }
    });

    println!("Press Enter to exit...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
    
    active_client.deactivate()?;
    Ok(())
}
