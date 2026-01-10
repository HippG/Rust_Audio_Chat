use jack::*;

struct AudioPassThrough {
    in_l: Port<AudioIn>,
    in_r: Port<AudioIn>,
    out: Port<AudioOut>,
}

impl ProcessHandler for AudioPassThrough {
    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        let in_l = self.in_l.as_slice(ps);
        let in_r = self.in_r.as_slice(ps);
        let out = self.out.as_mut_slice(ps);

        for i in 0..out.len() {
            
            out[i] = (in_l[i] + in_r[i]) * 0.5;
        }

        Control::Continue
    }
}

fn main() -> Result<(), jack::Error> {
    let (client, _status) = Client::new("mic_pass", ClientOptions::NO_START_SERVER)?;

    let in_l = client.register_port("in_l", AudioIn::default())?;
    let in_r = client.register_port("in_r", AudioIn::default())?;
    let out = client.register_port("out", AudioOut::default())?;

    let process = AudioPassThrough { in_l, in_r, out };
    let _active = client.activate_async((), process)?;

    println!("🎤 Pass-through actif. Parlez dans votre micro…");
    std::thread::sleep(std::time::Duration::from_secs(100));

    println!("✅ Done!");
    Ok(())
}
