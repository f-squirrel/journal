//! A scripted HTTP transport for the tests.
//!
//! Every test names the requests it expects, in order, with the reply to each;
//! a request that is not next in the script fails the test. What the adapter
//! sent is kept, so a test can assert on the route, the JSON arguments and the
//! headers as well as on what came back. That covers response parsing and
//! error mapping without a mock server, and without the network.

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::{Arc, Mutex, PoisonError};

use dropbox_sdk::client_trait::{HttpClient, HttpRequest, HttpRequestResultRaw, NoauthClient};
use dropbox_sdk::Error;
use serde_json::{json, Value};

#[derive(Clone, Default)]
pub(crate) struct FakeHttp {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    script: VecDeque<(String, Reply)>,
    sent: Vec<Sent>,
}

pub(crate) enum Reply {
    Http {
        status: u16,
        body: Vec<u8>,
        result_header: Option<String>,
    },
    NetworkDown,
}

impl Reply {
    /// 200 with a JSON body.
    pub fn ok(body: Value) -> Self {
        Self::status(200, body)
    }

    pub fn status(status: u16, body: Value) -> Self {
        Reply::Http {
            status,
            body: body.to_string().into_bytes(),
            result_header: None,
        }
    }

    /// A plain-text body, as Dropbox sends for a 400.
    pub fn text(status: u16, body: &str) -> Self {
        Reply::Http {
            status,
            body: body.as_bytes().to_vec(),
            result_header: None,
        }
    }

    /// 409: the request was understood and the operation refused. Dropbox
    /// wraps the typed error in `{"error": ..}`.
    pub fn api_error(error: Value) -> Self {
        Self::status(409, json!({ "error_summary": "test", "error": error }))
    }

    /// A download: the metadata in a header, the bytes in the body.
    pub fn download(metadata: Value, bytes: &[u8]) -> Self {
        Reply::Http {
            status: 200,
            body: bytes.to_vec(),
            result_header: Some(metadata.to_string()),
        }
    }

    pub fn network_down() -> Self {
        Reply::NetworkDown
    }
}

/// One request as the adapter made it.
#[derive(Clone, Debug)]
pub(crate) struct Sent {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Sent {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The call's arguments: in a header for uploads and downloads, in the
    /// body for everything else.
    pub fn args(&self) -> Value {
        match self.header("Dropbox-API-Arg") {
            Some(arg) => serde_json::from_str(arg).expect("the argument header is JSON"),
            None => serde_json::from_slice(&self.body).expect("the body is JSON"),
        }
    }

    /// A form-encoded body, as the OAuth token endpoint takes.
    pub fn form(&self) -> Vec<(String, String)> {
        url::form_urlencoded::parse(&self.body)
            .into_owned()
            .collect()
    }
}

impl FakeHttp {
    /// Expect the next request to go to `route` (e.g. `2/files/upload`), and
    /// answer it with `reply`.
    pub fn expect(self, route: &str, reply: Reply) -> Self {
        self.lock().script.push_back((route.to_owned(), reply));
        self
    }

    pub fn sent(&self) -> Vec<Sent> {
        self.lock().sent.clone()
    }

    /// The single request that was sent.
    pub fn only_request(&self) -> Sent {
        let sent = self.sent();
        assert_eq!(sent.len(), 1, "expected exactly one request: {sent:?}");
        sent.into_iter().next().unwrap()
    }

    /// Every scripted request was made, and no more.
    pub fn assert_finished(&self) {
        let inner = self.lock();
        let pending: Vec<_> = inner.script.iter().map(|(route, _)| route).collect();
        assert!(pending.is_empty(), "requests never made: {pending:?}");
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub(crate) struct FakeRequest {
    url: String,
    headers: Vec<(String, String)>,
}

impl HttpRequest for FakeRequest {
    fn set_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

impl HttpClient for FakeHttp {
    type Request = FakeRequest;

    fn execute(&self, request: FakeRequest, body: &[u8]) -> Result<HttpRequestResultRaw, Error> {
        // Released before any assert, so a failed one does not poison the
        // script for the messages that follow.
        let next = {
            let mut inner = self.lock();
            inner.sent.push(Sent {
                url: request.url.clone(),
                headers: request.headers.clone(),
                body: body.to_vec(),
            });
            inner.script.pop_front()
        };
        let Some((route, reply)) = next else {
            panic!("unscripted request to {}", request.url);
        };
        assert!(
            request.url.ends_with(&format!("/{route}")),
            "expected a request to {route}, got {}",
            request.url
        );
        match reply {
            Reply::NetworkDown => Err(Error::HttpClient(Box::new(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "connection reset",
            )))),
            Reply::Http {
                status,
                body,
                result_header,
            } => Ok(HttpRequestResultRaw {
                status,
                result_header,
                content_length: Some(body.len() as u64),
                body: Box::new(Cursor::new(body)),
            }),
        }
    }

    fn new_request(&self, url: &str) -> FakeRequest {
        FakeRequest {
            url: url.to_owned(),
            headers: Vec::new(),
        }
    }
}

impl NoauthClient for FakeHttp {}

/// Dropbox's JSON for the things the adapter reads.
pub(crate) mod wire {
    use super::*;

    pub fn file(name: &str, rev: &str, hash: Option<&str>) -> Value {
        let mut file = json!({
            ".tag": "file",
            "name": name,
            "id": format!("id:{name}"),
            "client_modified": "2026-01-01T00:00:00Z",
            "server_modified": "2026-01-01T00:00:00Z",
            "rev": rev,
            "size": 1,
            "path_lower": format!("/{}", name.to_lowercase()),
            "path_display": format!("/{name}"),
            "is_downloadable": true,
        });
        if let Some(hash) = hash {
            file["content_hash"] = json!(hash);
        }
        file
    }

    pub fn deleted(name: &str) -> Value {
        json!({
            ".tag": "deleted",
            "name": name,
            "path_lower": format!("/{}", name.to_lowercase()),
            "path_display": format!("/{name}"),
        })
    }

    pub fn folder(name: &str) -> Value {
        json!({
            ".tag": "folder",
            "name": name,
            "id": format!("id:{name}"),
            "path_lower": format!("/{}", name.to_lowercase()),
        })
    }

    pub fn listing(entries: Vec<Value>, cursor: &str, has_more: bool) -> Value {
        json!({ "entries": entries, "cursor": cursor, "has_more": has_more })
    }

    /// `users/get_current_account`'s answer, with every field the SDK requires.
    pub fn account(account_id: &str) -> Value {
        json!({
            "account_id": account_id,
            "name": {
                "given_name": "Ada",
                "surname": "Lovelace",
                "familiar_name": "Ada",
                "display_name": "Ada Lovelace",
                "abbreviated_name": "AL",
            },
            "email": "ada@example.com",
            "email_verified": true,
            "disabled": false,
            "locale": "en",
            "referral_link": "https://db.tt/x",
            "is_paired": false,
            "account_type": tag("basic"),
            "root_info": {
                ".tag": "user",
                "root_namespace_id": "1",
                "home_namespace_id": "1",
            },
        })
    }

    /// `{".tag": tag}` — a Stone union with no payload.
    pub fn tag(tag: &str) -> Value {
        json!({ ".tag": tag })
    }
}
