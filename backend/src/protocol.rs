use serde_json::{Value, json};

use super::{Activity, bindings::alcomd::extension::host_local_ipc as ipc};

const APPLICATION_ID: &str = "1536569303653154996";
const TARGET: &str = "discord";
const MAX_PAYLOAD: usize = 65_528;
const DEADLINE_TICKS: u8 = 4;

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Status {
    Waiting,
    Connecting,
    Connected,
    Publishing,
    ClientError,
}

#[derive(Default)]
pub(super) struct Presence {
    connection: Option<Connection>,
    backoff: u8,
    wait: u8,
    failed: bool,
}

struct Connection {
    channel: ipc::Connection,
    buffer: Vec<u8>,
    ready: bool,
    last: Option<Activity>,
    pending: Option<Pending>,
    nonce: u64,
    waiting_ticks: u8,
}

struct Pending {
    nonce: String,
    activity: Option<Activity>,
}

impl Presence {
    pub(super) fn status(&self) -> Status {
        match &self.connection {
            None if self.failed => Status::ClientError,
            None => Status::Waiting,
            Some(connection) if !connection.ready => Status::Connecting,
            Some(connection) if connection.last.is_some() => Status::Publishing,
            Some(_) => Status::Connected,
        }
    }

    pub(super) fn tick(&mut self, activity: Option<&Activity>) {
        if let Some(mut connection) = self.connection.take() {
            if connection.tick(activity).is_ok() {
                if connection.ready {
                    self.backoff = 0;
                }
                self.connection = Some(connection);
                return;
            }
            let _ = ipc::close(&connection.channel.channel);
            self.failed = true;
            self.delay();
            return;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        match Connection::connect() {
            Ok(connection) => {
                self.connection = Some(connection);
                self.failed = false;
            }
            Err(error) => {
                self.failed =
                    !matches!(error, ipc::IpcError::Unavailable | ipc::IpcError::TimedOut);
                self.delay();
            }
        }
    }

    fn delay(&mut self) {
        self.wait = 1 << self.backoff;
        self.backoff = (self.backoff + 1).min(4);
    }

    pub(super) fn clear_and_close(&mut self) -> bool {
        let Some(mut connection) = self.connection.take() else {
            return true;
        };
        let clear =
            if connection.ready && (connection.last.is_some() || connection.pending.is_some()) {
                connection
                    .set_activity(None)
                    .and_then(|()| {
                        for _ in 0..2 {
                            connection.read()?;
                            if connection.pending.is_none() {
                                return Ok(());
                            }
                        }
                        Err(ipc::IpcError::TimedOut)
                    })
                    .is_ok()
            } else {
                true
            };
        let _ = ipc::close(&connection.channel.channel);
        clear
    }
}

impl Connection {
    fn connect() -> Result<Self, ipc::IpcError> {
        let channel = ipc::connect(TARGET)?;
        let connection = Self {
            channel,
            buffer: Vec::new(),
            ready: false,
            last: None,
            pending: None,
            nonce: 0,
            waiting_ticks: 0,
        };
        if let Err(error) = connection.send_json(0, &json!({"v": 1, "client_id": APPLICATION_ID})) {
            let _ = ipc::close(&connection.channel.channel);
            return Err(error);
        }
        Ok(connection)
    }

    fn tick(&mut self, activity: Option<&Activity>) -> Result<(), ipc::IpcError> {
        if self.ready && self.pending.is_none() && self.last.as_ref() != activity {
            return self.set_activity(activity);
        }
        self.read()?;
        if !self.ready || self.pending.is_some() {
            self.waiting_ticks += 1;
            if self.waiting_ticks >= DEADLINE_TICKS {
                return Err(ipc::IpcError::TimedOut);
            }
        }
        Ok(())
    }

    fn set_activity(&mut self, activity: Option<&Activity>) -> Result<(), ipc::IpcError> {
        self.nonce = self
            .nonce
            .checked_add(1)
            .ok_or(ipc::IpcError::ResourceLimit)?;
        let nonce = format!("presence-{}", self.nonce);
        self.send_json(
            1,
            &json!({
                "cmd": "SET_ACTIVITY", "nonce": nonce,
                "args": { "pid": self.channel.client_process_id, "activity": activity },
            }),
        )?;
        self.pending = Some(Pending {
            nonce,
            activity: activity.cloned(),
        });
        self.waiting_ticks = 0;
        Ok(())
    }

    fn send_json(&self, opcode: u32, payload: &Value) -> Result<(), ipc::IpcError> {
        let payload = serde_json::to_vec(payload).map_err(|_| ipc::IpcError::InvalidArgument)?;
        ipc::write(&self.channel.channel, &frame(opcode, &payload)?)
    }

    fn read(&mut self) -> Result<(), ipc::IpcError> {
        match ipc::read(&self.channel.channel, 4096) {
            Ok(bytes) => {
                if self.buffer.len() + bytes.len() > MAX_PAYLOAD + 8 + 4096 {
                    return Err(ipc::IpcError::ResourceLimit);
                }
                self.buffer.extend(bytes);
            }
            Err(ipc::IpcError::TimedOut) => return Ok(()),
            Err(error) => return Err(error),
        }
        while let Some(DecodedFrame {
            opcode,
            payload,
            consumed,
        }) = next_frame(&self.buffer)?
        {
            match opcode {
                1 => {
                    let response = serde_json::from_slice(payload)
                        .map_err(|_| ipc::IpcError::InvalidArgument)?;
                    self.accept_response(&response)?;
                }
                2 => return Err(ipc::IpcError::PeerClosed),
                3 => ipc::write(&self.channel.channel, &frame(4, payload)?)?,
                4 => {}
                _ => return Err(ipc::IpcError::InvalidArgument),
            }
            self.buffer.drain(..consumed);
        }
        Ok(())
    }

    fn accept_response(&mut self, response: &Value) -> Result<(), ipc::IpcError> {
        if response["evt"].as_str() == Some("READY") {
            self.ready = true;
            self.waiting_ticks = 0;
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| response["nonce"].as_str() == Some(&pending.nonce))
        {
            if response["evt"].as_str() == Some("ERROR")
                || response["cmd"].as_str() != Some("SET_ACTIVITY")
            {
                return Err(ipc::IpcError::Unavailable);
            }
            self.last = self.pending.take().and_then(|pending| pending.activity);
            self.waiting_ticks = 0;
        }
        Ok(())
    }
}

fn frame(opcode: u32, payload: &[u8]) -> Result<Vec<u8>, ipc::IpcError> {
    if payload.len() > MAX_PAYLOAD {
        return Err(ipc::IpcError::ResourceLimit);
    }
    let mut message = Vec::with_capacity(payload.len() + 8);
    message.extend(opcode.to_le_bytes());
    message.extend((payload.len() as u32).to_le_bytes());
    message.extend(payload);
    Ok(message)
}

struct DecodedFrame<'a> {
    opcode: u32,
    payload: &'a [u8],
    consumed: usize,
}

fn next_frame(bytes: &[u8]) -> Result<Option<DecodedFrame<'_>>, ipc::IpcError> {
    let Some(header) = bytes.first_chunk::<8>() else {
        return Ok(None);
    };
    let [a, b, c, d, e, f, g, h] = *header;
    let opcode = u32::from_le_bytes([a, b, c, d]);
    let length = u32::from_le_bytes([e, f, g, h]) as usize;
    if length > MAX_PAYLOAD {
        return Err(ipc::IpcError::ResourceLimit);
    }
    let end = 8 + length;
    Ok(bytes.get(8..end).map(|payload| DecodedFrame {
        opcode,
        payload,
        consumed: end,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_decoder_preserves_partial_and_coalesced_messages() {
        let mut bytes = frame(1, b"one").expect("frame");
        assert!(next_frame(&bytes[..10]).expect("partial").is_none());
        bytes.extend(frame(3, b"two").expect("frame"));
        let DecodedFrame {
            opcode,
            payload,
            consumed,
        } = next_frame(&bytes).expect("valid").expect("complete");
        assert_eq!((opcode, payload), (1, b"one".as_slice()));
        assert_eq!(
            next_frame(&bytes[consumed..])
                .expect("valid")
                .expect("complete")
                .payload,
            b"two"
        );
        let invalid = [1, 0, 0, 0, 255, 255, 255, 255];
        assert!(matches!(
            next_frame(&invalid),
            Err(ipc::IpcError::ResourceLimit)
        ));
    }

    #[test]
    fn presence_and_clear_require_an_acknowledgement_for_the_exact_request() {
        let mut connection = Connection {
            channel: ipc::Connection {
                channel: "fixture".into(),
                client_process_id: 42,
            },
            buffer: Vec::new(),
            ready: false,
            last: None,
            pending: None,
            nonce: 1,
            waiting_ticks: 0,
        };
        connection
            .accept_response(&json!({"cmd":"DISPATCH","evt":"READY"}))
            .expect("ready");
        let activity = Activity {
            kind: 0,
            details: "Editing in Unity".into(),
            state: None,
        };
        connection.pending = Some(Pending {
            nonce: "presence-1".into(),
            activity: Some(activity.clone()),
        });
        connection
            .accept_response(&json!({"cmd":"SET_ACTIVITY","nonce":"incorrect"}))
            .expect("unrelated response");
        assert!(connection.last.is_none() && connection.pending.is_some());
        connection
            .accept_response(&json!({"cmd":"SET_ACTIVITY","nonce":"presence-1"}))
            .expect("publish acknowledgement");
        assert_eq!(connection.last, Some(activity));
        connection.pending = Some(Pending {
            nonce: "presence-2".into(),
            activity: None,
        });
        assert!(
            connection
                .accept_response(&json!({"cmd":"SET_ACTIVITY","evt":"ERROR","nonce":"presence-2"}))
                .is_err()
        );
        assert!(connection.last.is_some());
        connection
            .accept_response(&json!({"cmd":"SET_ACTIVITY","nonce":"presence-2"}))
            .expect("clear acknowledgement");
        assert!(connection.last.is_none() && connection.pending.is_none());
    }
}
