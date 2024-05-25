use simple_broadcaster::Subscriber;
use std::{
    io::{self, Read, Write},
    process::{Command as StdCommand, Stdio},
    sync::mpsc::{self, channel, Receiver, RecvError, SendError, Sender},
    thread,
};
use thiserror::Error as ThisError;
use tracing::{self, trace};

pub struct Command {
    std_command: StdCommand,
}

impl From<StdCommand> for Command {
    fn from(std_command: StdCommand) -> Self {
        Self { std_command }
    }
}

pub struct StdinSender(Sender<String>);
pub struct StdoutReceiver(Receiver<String>);
pub struct StderrReceiver(Receiver<String>);

impl Command {
    pub fn run(
        &mut self,
        canceller: Subscriber<()>,
    ) -> Result<(StdinSender, StdoutReceiver, StderrReceiver), Error> {
        let (tx_in, rx_in) = channel::<String>();
        let (tx_out, rx_out) = channel::<String>();
        let (tx_err, rx_err) = channel::<String>();
        let mut pid = self
            .std_command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let cmd = self
            .std_command
            .get_program()
            .to_owned()
            .into_string()
            .unwrap();
        if let Some(mut stdin) = pid.stdin.take() {
            let cmd = cmd.clone();
            thread::spawn(move || {
                while let Ok(stdin_text) = rx_in.recv() {
                    let stdin_text: String = stdin_text;
                    stdin.write_all(stdin_text.as_bytes()).unwrap();
                }
                trace!("exiting the stdin thread of '{cmd:}'");
            });
        }
        if let Some(mut stdout) = pid.stdout.take() {
            let cmd = cmd.clone();
            thread::spawn(move || {
                let mut buf: [u8; 128] = [0; 128];
                while stdout.read(&mut buf).is_ok() {
                    let stdout_text = String::from_utf8_lossy(&buf);
                    tx_out.send(stdout_text.into_owned()).unwrap();
                }
                trace!("exiting the stdout thread of '{cmd:}'");
            });
        }

        if let Some(mut stderr) = pid.stderr.take() {
            let cmd = cmd.clone();
            thread::spawn(move || {
                let mut buf: [u8; 128] = [0; 128];
                while stderr.read(&mut buf).is_ok() {
                    let stderr_text = String::from_utf8_lossy(&buf);
                    tx_err.send(stderr_text.into_owned()).unwrap();
                }
                trace!("exiting the stderr thread of '{cmd:}'");
            });
        }

        thread::spawn(move || {
            let cmd = cmd.clone();
            if let Ok(_) = canceller.recv() {
                let _ = pid.kill();
            }
            trace!("exiting the canceller thread of '{cmd:}'");
        });

        Ok((
            StdinSender(tx_in),
            StdoutReceiver(rx_out),
            StderrReceiver(rx_err),
        ))
    }
}

#[derive(Debug, ThisError)]
pub enum Error {
    #[error(transparent)]
    IoError(#[from] io::Error),
    #[error(transparent)]
    SendError(#[from] mpsc::SendError<String>),
    #[error("thread could not join")]
    ThreadCouldNotJoin(String),
}

impl StdinSender {
    pub fn send(&self, input: String) -> Result<(), SendError<String>> {
        self.0.send(input)
    }
}

impl StdoutReceiver {
    pub fn recv(&self) -> Result<String, RecvError> {
        self.0.recv()
    }
}

impl StderrReceiver {
    pub fn recv(&self) -> Result<String, RecvError> {
        self.0.recv()
    }
}
