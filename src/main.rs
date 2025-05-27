


mod conf;

use core::str;
use std::{ffi::OsStr, process::{Command, ExitStatus}, str::FromStr, sync::{atomic::{AtomicBool, Ordering}, mpsc::channel, Arc}, time::Duration};

use anyhow::{anyhow, Context, Result};
use conf::{Exec, Input};
use log::{debug, info, warn};
use midir::MidiInput;
use parse_duration::parse;
use signal_hook::{consts::TERM_SIGNALS, flag};

#[derive(Clone)]
enum Action {
    Exec(Exec)
}

fn main() -> Result<()> {
    env_logger::init();
    info!("loading configuration");
    let config = conf::get_config()?;
    info!("found {} configured inputs", config.inputs.len());
    
    let (tx, rx) = channel();
    let mut connections = Vec::new();
    
    for (idx, input) in config.inputs.into_iter().enumerate() {
        info!("initializing MIDI client");
        let mut midi_in = MidiInput::new(format!("katarl-{}", idx).as_str())?;
        midi_in.ignore(midir::Ignore::SysexAndTime);

        info!("enumerating MIDI ports");
        let ports_and_names = midi_in.ports()
            .into_iter()
            .map(|port| {
                let port_name = midi_in.port_name(&port)?;
                debug!("found port {}", port_name);
                Ok((port_name, port))
            })
            .collect::<Result<Vec<(String, midir::MidiInputPort)>>>()?;

        info!("found {} MIDI ports", ports_and_names.len());

        let matching_port = match ports_and_names.into_iter().find(|(name, _)| name.contains(&input.port)) {
            Some((name, port)) => {
                info!("found matching midi port for pattern {}: name = {}, id = {}", input.port, name, port.id());
                port
            },
            None => {
                warn!("did not find matching MIDI input port for input pattern {}", input.port);
                continue
            },
        };

        let note = u8::from_str(&input.note)?;
        let press_duration = match input.hold_time.as_ref() {
            Some(h) => parse(&h).context(format!("failed to parse hold_time spec: {}", h))?,
            None => Duration::ZERO,
        };
        let tx = tx.clone();
        let mut input_buffer = None;
        let connection = midi_in.connect(&matching_port, &format!("katarl-in-{}", idx), move |ts, message, _| {
            match message {
                [128, n, ..] if *n == note && press_duration > Duration::ZERO => {
                    debug!("detected long press of note {}", n);
                    match input_buffer.take() {
                        Some((prev_ts, action)) => {
                            let observed_duration = ts - prev_ts;
                            let trigger_duration = press_duration.as_micros() as u64;
                            if observed_duration > trigger_duration {
                                debug!("long press of note {} had  observed duration {} > trigger duration {}, command active", n, observed_duration, trigger_duration);
                                let _ = tx.send(action);
                            }
                        },
                        _ => return,
                    };
                },
                [144, n, ..] if *n == note => {
                    let action = get_action(&input);
                    if press_duration == Duration::ZERO {
                        debug!("detected short press of note {}, command active", n);
                        let _ = tx.send(action);
                    } else {
                        input_buffer = Some((ts, action));
                    }
                }
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
                let cmd = match cmd {
                    Exec::String(s) => get_command(s.split_whitespace()),
                    Exec::List(l) => get_command(l.iter().map(|s| s.as_ref())),
                };

                let output = match cmd {
                    Some(mut cmd) => {
                        debug!("executing command: {:?}", cmd);
                        cmd.output()
                    },
                    None => continue,
                };

                match output {
                    Ok(o) if ExitStatus::success(&o.status) => {
                        if !o.stderr.is_empty() {
                            info!("command output: {}", write_output(&o.stdout));
                        }
                    },
                    Ok(o) => {
                        info!("command failed: {}", write_output(&o.stderr));
                    }
                    Err(e) => {
                        info!("failed to execute command: {}", e)
                    }
                }
            },
        }
    }

    Ok(())
}

fn get_action(input: &Input) -> Action {
    Action::Exec(input.exec.clone())
}

fn get_command<'a, I>(args: I) -> Option<Command> 
    where I: IntoIterator<Item = &'a str>
{
    let mut iter = args.into_iter();
    iter.next().map(|p| {
        let mut cmd = Command::new(p);
        cmd.args(iter.map(OsStr::new));
        cmd
    })
}

fn write_output(output: &[u8]) -> &str {
    match str::from_utf8(output) {
        Ok(o) => o.strip_suffix("\n").unwrap_or(o),
        Err(_) => "err: unreadable output",
    }
}


