


mod conf;

use core::str;
use std::{ffi::OsStr, process::{Command, ExitStatus}, str::{from_utf8, FromStr}, sync::{atomic::{AtomicBool, Ordering}, mpsc::channel, Arc}, time::Duration};

use anyhow::{anyhow, Result};
use midir::MidiInput;
use signal_hook::{consts::TERM_SIGNALS, flag};

#[derive(Clone)]
enum Action {
    Exec(String)
}

fn main() -> Result<()> {
    let config = conf::get_config()?;


    let mut midi_in = MidiInput::new("katarl")?;
    midi_in.ignore(midir::Ignore::SysexAndTime);

    let (tx, rx) = channel();
    let mut connections = Vec::new();

    for (idx, input) in config.inputs.into_iter().enumerate() {
        let mut midi_in = MidiInput::new("katarl")?;
        midi_in.ignore(midir::Ignore::SysexAndTime);

        let ports_and_names = midi_in.ports()
            .into_iter()
            .map(|port| {
                let port_name = midi_in.port_name(&port)?;
                Ok((port_name, port))
            })
            .collect::<Result<Vec<(String, midir::MidiInputPort)>>>()?;

        let matching_port = match ports_and_names.into_iter().find(|(name, _)| name.contains(&input.port)) {
            Some((_, port)) => port,
            None => continue,
        };

        let note = u8::from_str(&input.note)?;
        let tx = tx.clone();
        let connection = midi_in.connect(&matching_port, &format!("katarl-in-{}", idx), move |_, message, _| {
            match message {
                [144, n, ..] if *n == note => { let _ = tx.send(Action::Exec(input.exec.clone())); }
                _ => return,
            }
        }, ())
        .map_err(|e| anyhow!("connection error: {}", e))?;

        connections.push(connection);
    }

    let term_now = Arc::new(AtomicBool::new(false));
    for sig in TERM_SIGNALS {
        // When terminated by a second term signal, exit with exit code 1.
        // This will do nothing the first time (because term_now is false).
        flag::register_conditional_shutdown(*sig, 1, Arc::clone(&term_now))?;
        // But this will "arm" the above for the second time, by setting it to true.
        // The order of registering these is important, if you put this one first, it will
        // first arm and then terminate ‒ all in the first round.
        flag::register(*sig, Arc::clone(&term_now))?;
    };

    while !term_now.load(Ordering::Relaxed) {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Err(_) => { continue; },
            Ok(Action::Exec(cmd)) => {
                let mut split = cmd.split_whitespace();
                let binary = match split.next() {
                    Some(p) => p,
                    None => continue,
                };
                let cmd = Command::new(binary)
                    .args(split.map(OsStr::new))
                    .output();

                match cmd {
                    Ok(o) if ExitStatus::success(&o.status) => {
                        if !o.stderr.is_empty() {
                            
                        }
                        println!("command output: {}", write_output(&o.stdout));
                    },
                    Ok(o) => {
                        println!("command failed: {}", write_output(&o.stderr));
                    }
                    Err(e) => {
                        println!("failed to execute command: {}", e)
                    }
                }
            },
        }
    }

    Ok(())
}

fn write_output(output: &[u8]) -> &str {
    match str::from_utf8(output) {
        Ok(o) => o.strip_suffix("\n").unwrap_or(o),
        Err(_) => "err: unreadable output",
    }
}


