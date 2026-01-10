use jack::*;
use opus::{Decoder, Channels};
use ringbuf::{HeapRb};
use ringbuf::traits::*;
use std::thread;
use std::time::{Duration, Instant};
use std::net::UdpSocket;
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
        // Pop from ringbuf to output
        for i in 0..out_slice.len() {
            out_slice[i] = self.consumer.try_pop().unwrap_or(0.0);
        }
        Control::Continue
    }
}

fn main() -> Result<(), jack::Error> {
    let (client, _status) = Client::new("SpeakerClient", ClientOptions::NO_START_SERVER)?;
    let out_port = client.register_port("output", AudioOut::default())?;

    // Playback Ring Buffer
    // We need a large enough buffer for smoothed playback after decoding
    let ring = HeapRb::<f32>::new(48000 * 2); // 2 seconds ring buffer for immediate playback
    let (mut producer, consumer) = ring.split();

    let process = AudioPlayback { out_port, consumer };
    let active_client = client.activate_async((), process)?;

    // Network & Buffering
    let socket = UdpSocket::bind("0.0.0.0:7878").expect("Failed to bind UDP socket");
    println!("🔊 SpeakerClient listening on 0.0.0.0:7878");

    let mut decoder = Decoder::new(SAMPLE_RATE, Channels::Mono).unwrap();
    let mut pcm_out = vec![0.0; FRAME_SIZE];
    
    // Jitter Buffer (Packet Queue)
    let mut packet_queue: VecDeque<Vec<u8>> = VecDeque::new();
    let mut buffering = true;
    let target_buffer_time = Duration::from_secs(5);
    // Approx packets for 5s: 5000ms / 20ms = 250 packets
    let target_packets = 250; 
    let mut total_packets_received = 0;

    println!("⏳ Buffering {}s of audio...", target_buffer_time.as_secs());

    loop {
        let mut buf = [0u8; 1000]; // Max UDP packet size
        match socket.recv_from(&mut buf) {
            Ok((size, _src)) => {
                let packet = buf[..size].to_vec();
                packet_queue.push_back(packet);
                total_packets_received += 1;

                if buffering {
                    if packet_queue.len() >= target_packets {
                        buffering = false;
                        println!("▶️ Buffering complete. Playing...");
                    }
                }
            }
            Err(e) => eprintln!("Recv error: {}", e),
        }

        if !buffering {
            // Processing loop: Decode and push to ringbuf if there is space
            // This is mixed with the recv loop which blocks... 
            // In a pro app, recv would be on a separate thread pushing to a queue.
            // But here `recv_from` blocks completely.
            // So if we don't receive packets fast enough, we might starve the playback ringbuf 
            // BUT we only deplete packet_queue if we have packets.
            // We should really separate recv and decode threads so decode doesn't wait for recv if we have buffered packets.
            // HOWEVER, we are just prototyping.
            // Let's create a thread for receiving and putting into a shared queue?
            // Or just a thread for decoding?
            // Let's refactor to have a dedicated Recv thread.
            break; 
        }
    }
    
    // REFACTORING: Spawning receive thread to avoid blocking playback feeding
    let socket_clone = socket.try_clone().expect("Failed to clone socket");
    let (tx, rx) = std::sync::mpsc::channel();
    
    thread::spawn(move || {
        loop {
            let mut buf = [0u8; 1000];
            match socket_clone.recv_from(&mut buf) {
                Ok((size, _)) => {
                    let packet = buf[..size].to_vec();
                    let _ = tx.send(packet);
                }
                Err(e) => eprintln!("Recv error: {}", e),
            }
        }
    });

    // Decoding Loop
    loop {
        // Refill queue from channel
        while let Ok(pkt) = rx.try_recv() {
            packet_queue.push_back(pkt);
        }
        
        if packet_queue.len() > 0 && producer.vacant_len() >= FRAME_SIZE {
            let packet = packet_queue.pop_front().unwrap();
             match decoder.decode_float(&packet, &mut pcm_out, false) {
                Ok(len) => {
                   producer.push_slice(&pcm_out[..len]);
                }
                Err(e) => eprintln!("Decode error: {}", e),
            }
        } else {
             thread::sleep(Duration::from_millis(1));
        }
    }
}
