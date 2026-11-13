//! Catching the OAuth redirect on a loopback socket.
//!
//! A provider's adapter crate (`journal-dropbox`) builds the authorize URL and
//! redeems the code, and opens no browser and listens on no socket. Listening is this file: a one-shot
//! HTTP listener on 127.0.0.1 that the browser is sent back to, which reads the
//! `code` and `state` off the request and answers with a page saying to return
//! to the app. Desktop only in effect: a phone's browser cannot reach the
//! app's loopback the way the flow needs (iOS suspends the app while the
//! browser is in front, so nothing is listening), and its redirect is a deep
//! link instead: [`DeepLink`].

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use url::Url;

/// How long a request line may take to arrive.
const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// More than any redirect needs, and less than a hostile client could send.
const MAX_REQUEST_LINE: u64 = 8 * 1024;

/// Listens on `port` (0 for any free one; see `Provider::redirect_port`) and
/// returns the redirect URI that
/// names the socket. `provider` is the name the user knows it by, for the
/// messages here and on the page the browser lands on.
pub fn bind(port: u16, provider: &str) -> Result<(TcpListener, String), String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).map_err(|e| {
        format!(
            "could not listen on 127.0.0.1:{port} for {provider}'s redirect: {e}. \
             Is another connection in progress?"
        )
    })?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("could not read the redirect port: {e}"))?
        .port();
    Ok((listener, format!("http://127.0.0.1:{port}/")))
}

/// What one request to the listener amounted to.
#[derive(Debug, PartialEq, Eq)]
enum Redirect {
    /// The redirect we are waiting for.
    Code(String),
    /// The user refused, or the provider reported a failure: the OAuth `error`
    /// code it sent.
    Denied(String),
    /// Not ours: another path, a wrong `state`, garbage. Keep waiting.
    Ignore,
}

/// Waits for the redirect and returns its authorization code.
///
/// A request whose `state` differs from `expected_state` is answered and
/// ignored rather than failing the wait: `state` is what stops another site
/// from completing *its* authorization inside this app, and dropping such a
/// request is the whole defence — it must not also be a way to abort the real
/// one. Gives up after `timeout`. `provider` is as for [`bind`].
pub fn await_code(
    listener: &TcpListener,
    expected_state: &str,
    timeout: Duration,
    provider: &str,
) -> Result<String, String> {
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("could not wait for {provider}'s redirect: {e}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((stream, _)) => match serve(stream, expected_state, provider) {
                Redirect::Code(code) => return Ok(code),
                Redirect::Denied(error) => return Err(denied(provider, &error)),
                Redirect::Ignore => {}
            },
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(format!(
                        "timed out waiting for {provider}; try connecting again"
                    ));
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("could not accept {provider}'s redirect: {e}")),
        }
    }
}

fn serve(mut stream: TcpStream, expected_state: &str, provider: &str) -> Redirect {
    // An accepted socket may inherit the listener's non-blocking mode.
    if stream.set_nonblocking(false).is_err()
        || stream.set_read_timeout(Some(READ_TIMEOUT)).is_err()
    {
        return Redirect::Ignore;
    }
    let mut line = String::new();
    let read = BufReader::new((&stream).take(MAX_REQUEST_LINE)).read_line(&mut line);
    if read.is_err() {
        return Redirect::Ignore;
    }

    let redirect = parse(&line, expected_state);
    let (status, body) = match &redirect {
        Redirect::Code(_) => (
            "200 OK",
            format!("Journal is connected to {provider}. You can close this tab."),
        ),
        Redirect::Denied(_) => (
            "200 OK",
            format!(
                "{provider} was not connected. You can close this tab and try again in Journal."
            ),
        ),
        Redirect::Ignore => ("404 Not Found", "Not found.".to_owned()),
    };
    let page = format!("<!doctype html><meta charset=utf-8><title>Journal</title><p>{body}</p>");
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    redirect
}

/// Reads an HTTP request line, `GET /?code=…&state=… HTTP/1.1`.
fn parse(request_line: &str, expected_state: &str) -> Redirect {
    let mut parts = request_line.split_whitespace();
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else {
        return Redirect::Ignore;
    };
    let Ok(url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Redirect::Ignore;
    };
    if url.path() != "/" {
        // A browser also asks for /favicon.ico.
        return Redirect::Ignore;
    }
    classify(&url, expected_state)
}

/// What a redirect's query says, whichever way it arrived.
fn classify(url: &Url, expected_state: &str) -> Redirect {
    let param = |key: &str| {
        url.query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
    };
    if param("state").as_deref() != Some(expected_state) {
        return Redirect::Ignore;
    }
    if let Some(error) = param("error") {
        return Redirect::Denied(error);
    }
    match param("code") {
        Some(code) if !code.is_empty() => Redirect::Code(code),
        _ => Redirect::Ignore,
    }
}

/// The error for a refused login.
fn denied(provider: &str, error: &str) -> String {
    match error {
        "access_denied" => format!("{provider} access was not granted"),
        other => format!("{provider} refused the connection: {other}"),
    }
}

/// Catching the redirect as a deep link, for a phone: the provider sends the
/// browser to `<scheme>:/oauth?code=…&state=…`, the OS reopens the app, and the
/// platform layer hands the URL to [`deliver`](Self::deliver). The scheme is
/// fixed at build time (it is declared in the app's manifest), so it is the
/// bundle identifier, and the user registers `<scheme>:/oauth` with the
/// provider.
pub struct DeepLink {
    scheme: String,
    waiting: Mutex<Option<Sender<Url>>>,
}

impl DeepLink {
    // Only a phone's build constructs one and receives links.
    #[cfg_attr(not(mobile), allow(dead_code))]
    pub fn new(scheme: &str) -> Self {
        Self {
            scheme: scheme.to_owned(),
            waiting: Mutex::new(None),
        }
    }

    /// The redirect URI to register with the provider and send in the request.
    pub fn redirect_uri(&self) -> String {
        format!("{}:/oauth", self.scheme)
    }

    /// Starts listening, replacing any earlier wait (whose `await_code` then
    /// ends as "interrupted"). Arm *before* the browser opens.
    pub fn arm(&self) -> Receiver<Url> {
        let (tx, rx) = mpsc::channel();
        *self.waiting.lock().unwrap_or_else(PoisonError::into_inner) = Some(tx);
        rx
    }

    /// Stops listening: a link arriving now is dropped.
    pub fn disarm(&self) {
        *self.waiting.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Hands over a URL the OS opened the app with. Anything that is not the
    /// redirect is ignored by the waiter, which checks `state`.
    #[cfg_attr(not(mobile), allow(dead_code))]
    pub fn deliver(&self, url: Url) {
        let waiting = self.waiting.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = waiting.as_ref() {
            // The waiter may have given up; there is nobody to tell.
            let _ = tx.send(url);
        }
    }

    /// Waits for the redirect and returns its authorization code; the
    /// counterpart of [`await_code`]. Links that are not ours (another scheme
    /// or path, a wrong `state`) are ignored rather than ending the wait.
    pub fn await_code(
        &self,
        links: &Receiver<Url>,
        expected_state: &str,
        timeout: Duration,
        provider: &str,
    ) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let url = match links.recv_timeout(left) {
                Ok(url) => url,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!(
                        "timed out waiting for {provider}; try connecting again"
                    ))
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(format!("the {provider} connection was interrupted"))
                }
            };
            if url.scheme() != self.scheme || url.path() != "/oauth" {
                continue;
            }
            match classify(&url, expected_state) {
                Redirect::Code(code) => return Ok(code),
                Redirect::Denied(error) => return Err(denied(provider, &error)),
                Redirect::Ignore => {}
            }
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    /// What a browser does: one request, and read the reply to the end.
    fn browse(port: u16, target: &str) -> String {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        write!(stream, "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        reply
    }

    fn port(listener: &TcpListener) -> u16 {
        listener.local_addr().unwrap().port()
    }

    #[test]
    fn the_redirect_uri_names_the_socket_it_is_listening_on() {
        let (listener, uri) = bind(0, "Dropbox").unwrap();
        assert_eq!(uri, format!("http://127.0.0.1:{}/", port(&listener)));
    }

    #[test]
    fn a_busy_port_says_so() {
        let (listener, _) = bind(0, "Dropbox").unwrap();
        let error = bind(port(&listener), "Dropbox").unwrap_err();
        assert!(error.contains("could not listen"), "{error}");
    }

    #[test]
    fn the_code_comes_back_when_the_state_matches() {
        let (listener, _) = bind(0, "Dropbox").unwrap();
        let p = port(&listener);
        let browser = thread::spawn(move || browse(p, "/?code=abc%20123&state=s3cret"));

        let code = await_code(&listener, "s3cret", Duration::from_secs(5), "Dropbox").unwrap();

        assert_eq!(code, "abc 123");
        let reply = browser.join().unwrap();
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert!(reply.contains("connected to Dropbox"), "{reply}");
    }

    #[test]
    fn requests_that_are_not_the_redirect_do_not_end_the_wait() {
        let (listener, _) = bind(0, "Dropbox").unwrap();
        let p = port(&listener);
        let browser = thread::spawn(move || {
            // Another site's authorization, and the favicon, and only then the
            // real thing.
            let forged = browse(p, "/?code=evil&state=wrong");
            let missing_state = browse(p, "/?code=evil");
            let favicon = browse(p, "/favicon.ico");
            browse(p, "/?code=good&state=s3cret");
            [forged, missing_state, favicon]
        });

        let code = await_code(&listener, "s3cret", Duration::from_secs(5), "Dropbox").unwrap();

        assert_eq!(code, "good");
        for reply in browser.join().unwrap() {
            assert!(reply.starts_with("HTTP/1.1 404"), "{reply}");
        }
    }

    #[test]
    fn a_refusal_ends_the_wait_with_a_reason() {
        let (listener, _) = bind(0, "Dropbox").unwrap();
        let p = port(&listener);
        let browser = thread::spawn(move || browse(p, "/?error=access_denied&state=s3cret"));

        let error = await_code(&listener, "s3cret", Duration::from_secs(5), "Dropbox").unwrap_err();

        assert!(error.contains("not granted"), "{error}");
        browser.join().unwrap();
    }

    #[test]
    fn a_refusal_with_the_wrong_state_is_not_believed() {
        assert_eq!(
            parse("GET /?error=access_denied&state=other HTTP/1.1", "s3cret"),
            Redirect::Ignore
        );
    }

    #[test]
    fn no_redirect_in_time_is_an_error() {
        let (listener, _) = bind(0, "Dropbox").unwrap();
        let error =
            await_code(&listener, "s3cret", Duration::from_millis(100), "Dropbox").unwrap_err();
        assert!(error.contains("timed out"), "{error}");
    }

    #[test]
    fn only_a_well_formed_get_is_considered() {
        for line in [
            "",
            "POST /?code=a&state=s HTTP/1.1",
            "GET",
            "GET /other?code=a&state=s HTTP/1.1",
            "GET /?code=&state=s HTTP/1.1",
            "GET /?state=s HTTP/1.1",
        ] {
            assert_eq!(parse(line, "s"), Redirect::Ignore, "{line:?}");
        }
    }

    fn link(url: &str) -> Url {
        Url::parse(url).unwrap()
    }

    #[test]
    fn the_deep_link_redirect_uri_is_the_scheme_and_a_path() {
        assert_eq!(
            DeepLink::new("com.example.app").redirect_uri(),
            "com.example.app:/oauth"
        );
    }

    #[test]
    fn a_deep_link_brings_the_code_and_skips_what_is_not_ours() {
        let links = DeepLink::new("app.test");
        let rx = links.arm();
        links.deliver(link("other:/oauth?code=evil&state=s"));
        links.deliver(link("app.test:/elsewhere?code=evil&state=s"));
        links.deliver(link("app.test:/oauth?code=evil&state=wrong"));
        links.deliver(link("app.test:/oauth?code=a%20b&state=s"));

        let code = links
            .await_code(&rx, "s", Duration::from_secs(5), "Dropbox")
            .unwrap();
        assert_eq!(code, "a b");
    }

    #[test]
    fn a_deep_link_refusal_ends_the_wait_with_a_reason() {
        let links = DeepLink::new("app.test");
        let rx = links.arm();
        links.deliver(link("app.test:/oauth?error=access_denied&state=s"));
        let error = links
            .await_code(&rx, "s", Duration::from_secs(5), "Dropbox")
            .unwrap_err();
        assert!(error.contains("not granted"), "{error}");
    }

    #[test]
    fn no_deep_link_in_time_is_an_error() {
        let links = DeepLink::new("app.test");
        let rx = links.arm();
        let error = links
            .await_code(&rx, "s", Duration::from_millis(50), "Dropbox")
            .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
    }

    #[test]
    fn arming_again_interrupts_the_earlier_wait_and_disarming_drops_links() {
        let links = DeepLink::new("app.test");
        let first = links.arm();
        let second = links.arm();
        let error = links
            .await_code(&first, "s", Duration::from_secs(5), "Dropbox")
            .unwrap_err();
        assert!(error.contains("interrupted"), "{error}");

        links.disarm();
        links.deliver(link("app.test:/oauth?code=late&state=s"));
        assert!(second.try_recv().is_err());
    }
}
