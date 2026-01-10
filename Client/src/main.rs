use jack::*;
use opus::{Decoder, Encoder, Channels, Application};
use ringbuf::{HeapRb};
use ringbuf::traits::*;
use std::thread;
use std::time::Duration;

const SAMPLE_RATE: u32 = 48000;
const FRAME_SIZE_MS: u32 = 20; // 20ms
const FRAME_SIZE: usize = (SAMPLE_RATE as usize * FRAME_SIZE_MS as usize) / 1000; // 960 samples

// Generic AudioGraph to handle ringbuf types
struct AudioGraph<P, C> {
    in_l: Port<AudioIn>,
    in_r: Port<AudioIn>,
    out: Port<AudioOut>,
    capture_prod: P,
    playback_cons: C,
}

impl<P, C> ProcessHandler for AudioGraph<P, C>
where
    P: Producer<Item = f32> + Send,
    C: Consumer<Item = f32> + Send,
{
    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        let in_l = self.in_l.as_slice(ps);
        let in_r = self.in_r.as_slice(ps);
        let out = self.out.as_mut_slice(ps);

        for i in 0..out.len() {
            let mono = (in_l[i] + in_r[i]) * 0.5;
            let _ = self.capture_prod.try_push(mono);
            out[i] = self.playback_cons.try_pop().unwrap_or(0.0);
        }
        Control::Continue
    }
}

fn main() -> Result<(), jack::Error> {
    let (client, _status) = Client::new("mic_pass", ClientOptions::NO_START_SERVER)?;

    let in_l = client.register_port("in_l", AudioIn::default())?;
    let in_r = client.register_port("in_r", AudioIn::default())?;
    let out = client.register_port("out", AudioOut::default())?;

    // Ring buffers
    let capture_rb = HeapRb::<f32>::new(48000 * 10);
    let (capture_prod, mut capture_cons) = capture_rb.split();

    let playback_rb = HeapRb::<f32>::new(48000 * 10);
    let (mut playback_prod, playback_cons) = playback_rb.split();

    let process = AudioGraph { 
        in_l, 
        in_r, 
        out, 
        capture_prod, 
        playback_cons 
    };

    let active_client = client.activate_async((), process)?;

    // Processing Thread
    thread::spawn(move || {
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip).unwrap();
        let mut decoder = Decoder::new(SAMPLE_RATE, Channels::Mono).unwrap();

        let mut pcm_in = vec![0.0; FRAME_SIZE];
        let mut out_bytes = vec![0u8; 1000];
        let mut pcm_out = vec![0.0; FRAME_SIZE];

        let mut stored_packets: Vec<Vec<u8>> = Vec::new();

        println!("⏳ Waiting 10 seconds for you to connect cables in Helvum...");
        thread::sleep(Duration::from_secs(10));
        println!("🚀 Starting now!");

        println!("🎙️ Recording 10s of audio...");
        let start = std::time::Instant::now();
        
        while start.elapsed().as_secs() < 10 {
            if capture_cons.occupied_len() >= FRAME_SIZE {
                capture_cons.pop_slice(&mut pcm_in);
                
                match encoder.encode_float(&pcm_in, &mut out_bytes) {
                    Ok(len) => {
                        let packet = out_bytes[..len].to_vec();
                        stored_packets.push(packet);
                    }
                    Err(e) => eprintln!("Encode error: {}", e),
                }
            } else {
                thread::sleep(Duration::from_millis(1));
            }
        }
        println!("⏹️ Recording stopped. Captured {} packets.", stored_packets.len());

        println!("▶️ Playing back...");
        for packet in stored_packets {
            match decoder.decode_float(&packet, &mut pcm_out, false) {
                Ok(len) => {
                   // Encode frame size might differ from decode result if packet loss, but here it should be same
                   while playback_prod.vacant_len() < len {
                       thread::sleep(Duration::from_millis(1));
                   }
                   playback_prod.push_slice(&pcm_out[..len]);
                }
                Err(e) => eprintln!("Decode error: {}", e),
            }
        }
        println!("✅ Playback data sent to buffer.");
    });

    println!("🔊 System running. Press Enter to exit.");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();

    active_client.deactivate()?;
    Ok(())
}
