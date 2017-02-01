extern crate argparse;

extern crate serde;

#[macro_use]
extern crate serde_derive;

extern crate serde_json;
extern crate serde_xml;

use std::io::BufRead;
use std::io::Write;
use serde::de::Deserialize;

#[derive(Clone, Debug)]
enum ManagerError {
    NullError,
    ConvertError,
    ReadError,
}

impl From<serde_json::Error> for ManagerError {
    fn from(_: serde_json::Error) -> ManagerError {
        ManagerError::ConvertError
    }
}

impl From<serde_xml::Error> for ManagerError {
    fn from(_: serde_xml::Error) -> ManagerError {
        ManagerError::ConvertError
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum Channel {
    #[serde(rename = "process_control_request")]
    ProcessControlRequest,
    #[serde(rename = "process_control_reply")]
    ProcessControlReply,
    #[serde(rename = "graphics_request")]
    GraphicsRequest,
    #[serde(rename = "graphics_reply")]
    GraphicsReply,
    #[serde(rename = "heartbeat")]
    Heartbeat,
    #[serde(rename = "trickle_up")]
    TrickleUp,
    #[serde(rename = "trickle_down")]
    TrickleDown,
    #[serde(rename = "app_status")]
    AppStatus,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
enum Action {
    #[serde(rename = "view")]
    View,
    #[serde(rename = "receive")]
    Receive,
    #[serde(rename = "send")]
    Send,
    #[serde(rename = "delete")]
    Delete,
}

type Message = serde_json::Value;
type MessageData = std::collections::HashMap<Channel, Message>;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ToolRequest {
    action: Action,
    channel: Channel,
    payload: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ToolResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    ok: Option<bool>,

    #[serde(rename = "error", skip_serializing_if = "Option::is_none")]
    error_msg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<String>,
}

trait ToolStream {
    fn do_request(&mut self, req: &ToolRequest) -> Result<ToolResponse, ManagerError>;
}

struct SimpleToolStream {
    mutex: std::sync::Mutex<i32>,
    input: std::process::ChildStdin,
    output: std::io::BufReader<std::process::ChildStdout>,
}

impl SimpleToolStream {
    fn new(i: std::process::ChildStdin,
           o: std::io::BufReader<std::process::ChildStdout>)
           -> SimpleToolStream {
        SimpleToolStream {
            mutex: std::sync::Mutex::new(0),
            input: i,
            output: o,
        }
    }
}

impl ToolStream for SimpleToolStream {
    fn do_request(&mut self, req: &ToolRequest) -> Result<ToolResponse, ManagerError> {
        let lg = self.mutex.lock();
        self.input.write_all(serde_json::to_string(&req).unwrap().as_bytes());
        self.input.write_all("\n".to_string().as_bytes());
        let mut buf = "".to_string();
        match self.output.read_line(&mut buf) {
            Ok(_) => {
                return Ok(try!(serde_json::from_str(&buf)));
            }
            _ => {
                return Err(ManagerError::NullError);
            }
        }
    }
}

struct ToolManager<S: ToolStream> {
    stream: Box<S>,
    fetch_cache: std::collections::HashMap<Channel, String>,
    out_cb: Box<FnMut(&MessageData)>,
}

impl<S: ToolStream> ToolManager<S> {
    fn new(stream: S, out_cb: Box<FnMut(&MessageData)>) -> ToolManager<S> {
        ToolManager {
            stream: Box::new(stream),
            fetch_cache: std::collections::HashMap::new(),
            out_cb: out_cb,
        }
    }

    fn fetch_updates(&mut self,
                     c: Channel,
                     receive: bool)
                     -> Result<Option<Message>, ManagerError> {
        let rsp = try!(self.stream.do_request(&ToolRequest {
            action: if receive {
                Action::Receive
            } else {
                Action::View
            },
            channel: c.clone(),
            payload: None,
        }));

        let payload = match rsp.data {
            Some(v) => v,
            None => {
                return Ok(None);
            }
        };

        self.fetch_cache.entry(c).or_insert("".to_string());

        if self.fetch_cache.get(&c).unwrap() == &payload {
            return Ok(None);
        } else {
            self.fetch_cache.insert(c, payload.clone());
        }

        if payload.is_empty() {
            return Ok(None);
        }

        Ok(try!(serde_xml::from_str(&payload)))
    }

    fn fetch_cycle(&mut self) {
        let mut rsps = MessageData::new();
        for c in vec![Channel::AppStatus].into_iter() {
            let rsp = self.fetch_updates(c, false);
            if rsp.is_ok() {
                let rsp = rsp.unwrap();
                if rsp.is_some() {
                    rsps.insert(c, rsp.unwrap());
                }
            }
        }
        (self.out_cb)(&rsps);
    }
}

fn main() {
    let mut mmap_path = "".to_string();
    let mut tool_path = "/usr/bin/boinc-shmem-tool".to_string();
    {
        let mut ap = argparse::ArgumentParser::new();
        ap.refer(&mut mmap_path).add_option(&["--mmap-path"], argparse::Store, "mmap path");
        ap.refer(&mut tool_path)
            .add_option(&["--tool-path"], argparse::Store, "boinc-shmem-tool path");
        ap.parse_args_or_exit();
    }

    let mut child = std::process::Command::new(&tool_path)
        .arg(&mmap_path)
        .stdout(std::process::Stdio::piped())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to start shmem tool");

    let mut manager = ToolManager::new(SimpleToolStream::new(child.stdin.take().unwrap(),
                                                             std::io::BufReader::new(child.stdout
                                                                 .take()
                                                                 .unwrap())), Box::new(|v| if !v.is_empty() { println!("{}", serde_json::to_string(&v).unwrap()) }));

    loop {
        manager.fetch_cycle();

        std::thread::sleep(std::time::Duration::from_millis(330));
    }
}
