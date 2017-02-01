extern crate std;

use std::io::prelude::*;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::io::BufReader;

pub struct Process {
    process: std::process::Child,
    tx: mpsc::Sender<Option<String>>,
    rx: mpsc::Receiver<Option<String>>,
}

impl Process {
    pub fn new(procname: &str, args: &str) -> Process {
        let mut process = Command::new(procname)
            .arg(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .spawn()
            .unwrap();

        let (tx, rx) = mpsc::channel();
        Process {
            process: process,
            tx: tx,
            rx: rx,
        }
    }

    pub fn run(&mut self) {
        let tx = self.tx.clone();
        let stdout = self.process
            .stdout
            .take()
            .unwrap();

        println!("spanwing thread");
        thread::spawn(move || {
            let reader = BufReader::new(stdout);

            for line in reader.lines() {
                tx.send(Some(line.unwrap()));
            }
        });
    }

    pub fn push(&mut self, buf: &[u8]) {
        let mut stdin = self.process
            .stdin
            .as_mut()
            .unwrap();

        stdin.write_all(buf);
    }

    pub fn packets(&mut self) -> ProcessIntoIterator {
        ProcessIntoIterator { subprocess: self }
    }
}

pub struct ProcessIntoIterator<'a> {
    subprocess: &'a mut Process,
}

impl<'a> Iterator for ProcessIntoIterator<'a> {
    type Item = String;
    fn next(&mut self) -> Option<String> {
        let data = self.subprocess.rx.try_recv();
        if data.is_ok() { data.unwrap() } else { None }
    }
}
