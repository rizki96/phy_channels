extern crate futures;
extern crate phoenix_channels;
extern crate websocket;

use pyo3::prelude::*;
use pyo3::{
    types::{PyAny, PyModule, PyString, PyDict, PyFloat, PyList, PyTuple, PyBool}
};
use pyo3::{PyErr, PyObject};
use pyo3::exceptions::TypeError;
//use pyo3::wrap_pyfunction;

use std::result::Result;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use futures::stream::Stream;
use websocket::futures::sync::mpsc;
use tokio_core::reactor::Core;
use serde_json::json;
use slog_term;

use phoenix_channels::client;
use phoenix_channels::sender;
use phoenix_channels::message;
use phoenix_channels::slog::{o, Drain};
use phoenix_channels::event;


#[pyclass(module = "phy_channels")]
struct PhoenixChannel {
    sender_ref: Arc<Mutex<sender::Sender>>,
    reader_ref: Arc<Mutex<Option<mpsc::Receiver<message::Message>>>>,
}

#[pymethods]
impl PhoenixChannel {
    #[new]
    fn new(pyurl: &PyString, pyparams: &PyDict, pylogger: &PyBool) -> Self {
        
        let mut logger = None;
        if pylogger.is_true() {
            let plain = slog_term::PlainSyncDecorator::new(std::io::stdout());
            logger = Some(phoenix_channels::slog::Logger::root(
                slog_term::FullFormat::new(plain)
                .build().fuse(), o!()
            ));
        };
        
        let url: String = pyurl.extract().unwrap();
        let mut params_holder: Vec<(String, String)> = Vec::new();
        pyparams.iter().for_each(|(key, val)| params_holder.push(
            (key.to_string(), val.to_string())));
        let mut params: Vec<(&str, &str)> = vec![];
        params_holder.iter().for_each(|(key, val)| params.push((&key, &val)));
        
        let (sender, receiver) = client::connect(&url, params, logger).unwrap();

        let sender_ref = Arc::new(Mutex::new(sender));
        let reader_ref = Arc::new(Mutex::new(Some(receiver.reader)));

        PhoenixChannel {
            sender_ref: sender_ref,
            reader_ref: reader_ref,
        }

    }

    fn join(&self, channel: &PyString) -> PyResult<u32> {
        let chan: String = channel.extract().unwrap();

        let sender_heartbeat = Arc::clone(&self.sender_ref);
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(2));
                // if the mutex is poisoned then the whole thread wont work
                let mut sender = sender_heartbeat.lock().unwrap();
                sender.heartbeat().unwrap();
            }
        });
    
        let sender_join = Arc::clone(&self.sender_ref);
        let mut sender = sender_join.lock().unwrap();
        match sender.join(&chan) {
            Ok(res) => Ok(res),
            Err(_) => Err(PyErr::new::<TypeError, _>("could not join topic"))
        }
    }

    fn send(&self, py: Python<'_>, channel: &PyString, event: &PyString, message: PyObject) -> PyResult<u32> {
        let chan: String = channel.extract().unwrap();
        let ev: String = event.extract().unwrap();

        let v = SerializePyObject {
            py,
            obj: message.extract(py)?,
            sort_keys: true,
        };
        let msg = json!(v);

        let sender_send = Arc::clone(&self.sender_ref);
        let mut sender = sender_send.lock().unwrap();

        match sender.send(&chan, event::EventKind::Custom(ev), &msg) {
            Ok(res) => Ok(res),
            Err(_) => Err(PyErr::new::<TypeError, _>("could not send message"))
        }
    }

    fn run_core(&mut self, py: Python<'_>, callback: PyObject) -> PyResult<u32> {

        let reader_clone = Arc::clone(&self.reader_ref);

        let mut reader = reader_clone.lock().unwrap();
        let runner = reader.as_mut().unwrap().for_each(|msg| {
            //println!("{:?}", msg);
            if let message::Message::Json(message::PhoenixMessage{
                                              join_ref,
                                              message_ref,
                                              topic,
                                              event,
                                              payload}) = msg {
                let _join_ref = match join_ref {
                    Some(val) => val,
                    None => 0
                };
                let m_ref = match message_ref {
                    Some(val) => val,
                    None => 0
                };
                let ev = match event {
                    event::EventKind::Close => String::from("phx_close"),
                    event::EventKind::Error => String::from("phx_error"),
                    event::EventKind::Join => String::from("phx_join"),
                    event::EventKind::Leave => String::from("phx_leave"),
                    event::EventKind::Reply => String::from("phx_reply"),
                    event::EventKind::Custom(val) => val,
                };
                callback.call(py, (m_ref, &topic, &ev, &payload.to_string()),
                              None).unwrap();
            }

            Ok(())
        });

        // TODO: how to get along with python async/await
        let mut core = Core::new().unwrap();
        core.run(runner).unwrap();

        Ok(1)
    }
}


use serde::ser::{self, Serialize, SerializeMap, SerializeSeq, Serializer};

pub struct SerializePyObject<'p, 'a> {
    py: Python<'p>,
    obj: &'a PyAny,
    sort_keys: bool,
}

impl<'p, 'a> Serialize for SerializePyObject<'p, 'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        macro_rules! cast {
            ($f:expr) => {
                if let Ok(val) = PyTryFrom::try_from(self.obj) {
                    return $f(val);
                }
            };
        }

        macro_rules! extract {
            ($t:ty) => {
                if let Ok(val) = <$t as FromPyObject>::extract(self.obj) {
                    return val.serialize(serializer);
                }
            };
        }

        fn debug_py_err<E: ser::Error>(err: PyErr) -> E {
            E::custom(format_args!("{:?}", err))
        }

        cast!(|x: &PyDict| {
            if self.sort_keys {
                // TODO: this could be implemented more efficiently by building
                // a `Vec<Cow<str>, &PyAny>` of the map entries, sorting
                // by key, and serializing as in the `else` branch. That avoids
                // buffering every map value into a serde_json::Value.
                let no_sort_keys = SerializePyObject {
                    py: self.py,
                    obj: self.obj,
                    sort_keys: false,
                };
                let jv = serde_json::to_value(no_sort_keys).map_err(ser::Error::custom)?;
                jv.serialize(serializer)
            } else {
                let mut map = serializer.serialize_map(Some(x.len()))?;
                for (key, value) in x {
                    if key.is_none() {
                        map.serialize_key("null")?;
                    } else if let Ok(key) = key.extract::<bool>() {
                        map.serialize_key(if key { "true" } else { "false" })?;
                    } else if let Ok(key) = key.str() {
                        let key = key.to_string().map_err(debug_py_err)?;
                        map.serialize_key(&key)?;
                    } else {
                        return Err(ser::Error::custom(format_args!(
                            "Dictionary key is not a string: {:?}",
                            key
                        )));
                    }
                    map.serialize_value(&SerializePyObject {
                        py: self.py,
                        obj: value,
                        sort_keys: self.sort_keys,
                    })?;
                }
                map.end()
            }
        });

        cast!(|x: &PyList| {
            let mut seq = serializer.serialize_seq(Some(x.len()))?;
            for element in x {
                seq.serialize_element(&SerializePyObject {
                    py: self.py,
                    obj: element,
                    sort_keys: self.sort_keys,
                })?
            }
            seq.end()
        });
        cast!(|x: &PyTuple| {
            let mut seq = serializer.serialize_seq(Some(x.len()))?;
            for element in x {
                seq.serialize_element(&SerializePyObject {
                    py: self.py,
                    obj: element,
                    sort_keys: self.sort_keys,
                })?
            }
            seq.end()
        });

        extract!(String);
        extract!(bool);

        cast!(|x: &PyFloat| x.value().serialize(serializer));
        extract!(u64);
        extract!(i64);

        if self.obj.is_none() {
            return serializer.serialize_unit();
        }

        match self.obj.repr() {
            Ok(repr) => Err(ser::Error::custom(format_args!(
                "Value is not JSON serializable: {}",
                repr,
            ))),
            Err(_) => Err(ser::Error::custom(format_args!(
                "Type is not JSON serializable: {}",
                self.obj.get_type().name().into_owned(),
            ))),
        }
    }
}

#[pymodule]
fn phy_channels(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<PhoenixChannel>()?;

    Ok(())
}
