use simple_broadcaster::Subscriber;
use std::{
    any::Any,
    io::{self, Write},
    process::{Command as StdCommand, Stdio},
    sync::mpsc::{channel, Receiver, Sender},
    thread,
};
use thiserror::Error as ThisError;

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
        let pid = self
            .std_command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let handle_stdin_read = pid.stdin.map(|mut stdin| {
            thread::spawn(move || -> Result<(), Error> {
                while let Ok(stdin_text) = rx_in.recv() {
                    let stdin_text: String = stdin_text;
                    stdin.write(stdin_text.as_bytes())?;
                }
                Ok(())
            })
        });
        let handle_stdout_write = pid
            .stdout
            .map(|mut stdout| thread::spawn(move || -> Result {}));

        if let Some(stderr) = pid.stderr {}

        if let Some(handle_val) = handle_stdin_read {
            handle_val.join()?;
        }

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
    FromResidual(#[from] io::Error),
    #[error("error while joing the thread: {0:?}")]
    Any(Box<dyn Any + Send>),
}
