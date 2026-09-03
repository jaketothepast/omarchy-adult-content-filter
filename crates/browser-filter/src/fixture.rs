use std::{
    io::Cursor,
    net::{Ipv4Addr, TcpListener},
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use tokio::{net::TcpListener as TokioTcpListener, runtime::Builder, sync::watch};
use url::Url;

const FIXTURE_MARKER: &str = "omarchy-kids-fixture";
const FLAGGED_VALUE: &str = "flagged";

#[derive(Clone)]
struct FixtureState {
    images: Arc<Vec<Vec<u8>>>,
    flagged_index: usize,
    shutdown: watch::Receiver<bool>,
}

pub struct FixtureServer {
    url: Url,
    shutdown: Option<watch::Sender<bool>>,
    thread: Option<JoinHandle<()>>,
}

impl FixtureServer {
    pub fn start(image_count: usize, flagged_index: usize) -> Result<Self> {
        anyhow::ensure!(image_count > 0, "image_count must be positive");
        anyhow::ensure!(
            flagged_index < image_count,
            "flagged_index must identify an image"
        );
        anyhow::ensure!(image_count <= 100, "image_count must be within 1..=100");

        let images = (0..image_count).map(make_png).collect::<Result<Vec<_>>>()?;
        let (shutdown, shutdown_received) = watch::channel(false);
        let state = FixtureState {
            images: Arc::new(images),
            flagged_index,
            shutdown: shutdown_received.clone(),
        };
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .context("failed to bind fixture server to loopback")?;
        listener
            .set_nonblocking(true)
            .context("failed to configure fixture listener")?;
        let address = listener
            .local_addr()
            .context("failed to read fixture listener address")?;
        let url =
            Url::parse(&format!("http://{address}/")).context("failed to construct fixture URL")?;
        let (ready, started) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("omarchy-kids-fixture".to_owned())
            .spawn(move || run_server(listener, state, shutdown_received, ready))
            .context("failed to start fixture server thread")?;

        started
            .recv()
            .context("fixture server thread stopped before startup")??;

        Ok(Self {
            url,
            shutdown: Some(shutdown),
            thread: Some(thread),
        })
    }

    pub fn url(&self) -> Url {
        self.url.clone()
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            shutdown.send_replace(true);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_server(
    listener: TcpListener,
    state: FixtureState,
    mut shutdown: watch::Receiver<bool>,
    ready: mpsc::SyncSender<Result<()>>,
) {
    let runtime = match Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = ready.send(Err(error.into()));
            return;
        }
    };

    runtime.block_on(async move {
        let listener = match TokioTcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                let _ = ready.send(Err(error.into()));
                return;
            }
        };
        let app = router(state);
        if ready.send(Ok(())).is_err() {
            return;
        }
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown.wait_for(|stopping| *stopping).await;
            })
            .await;
    });
}

fn router(state: FixtureState) -> Router {
    Router::new()
        .route("/", get(homepage))
        .route("/image/{filename}", get(image))
        .route("/redirect.png", get(redirect))
        .route("/corrupt.png", get(corrupt))
        .route("/slow/{filename}", get(slow))
        .route("/health", get(health))
        .with_state(state)
}

async fn homepage(State(state): State<FixtureState>) -> Html<String> {
    let images = state
        .images
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let marker = if index == state.flagged_index {
                format!("?{FIXTURE_MARKER}={FLAGGED_VALUE}")
            } else {
                String::new()
            };
            format!("<img src=\"/image/{index}.png{marker}\">")
        })
        .collect::<String>();
    Html(format!("<!doctype html><html><body>{images}</body></html>"))
}

async fn image(Path(filename): Path<String>, State(state): State<FixtureState>) -> Response {
    match filename
        .strip_suffix(".png")
        .and_then(|index| index.parse().ok())
    {
        Some(index) => fixture_image_response(&state, index),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn redirect(State(state): State<FixtureState>) -> Redirect {
    Redirect::temporary(&fixture_image_path(
        state.flagged_index,
        state.flagged_index,
    ))
}

async fn corrupt() -> Response {
    png_response(b"corrupt png".to_vec(), false)
}

async fn slow(Path(filename): Path<String>, State(state): State<FixtureState>) -> Response {
    match filename
        .strip_suffix(".png")
        .and_then(|millis| millis.parse().ok())
    {
        Some(millis) => {
            let mut shutdown = state.shutdown.clone();
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(millis)) => {
                    fixture_image_response(&state, 0)
                }
                _ = shutdown.wait_for(|stopping| *stopping) => {
                    StatusCode::SERVICE_UNAVAILABLE.into_response()
                }
            }
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn health() -> StatusCode {
    StatusCode::OK
}

fn fixture_image_response(state: &FixtureState, index: usize) -> Response {
    match state.images.get(index) {
        Some(image) => png_response(image.clone(), index == state.flagged_index),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn png_response(body: Vec<u8>, flagged: bool) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
    if flagged {
        headers.insert(
            HeaderName::from_static("x-omarchy-kids-fixture"),
            HeaderValue::from_static(FLAGGED_VALUE),
        );
    }
    (headers, body).into_response()
}

fn fixture_image_path(index: usize, flagged_index: usize) -> String {
    let marker = if index == flagged_index {
        format!("?{FIXTURE_MARKER}={FLAGGED_VALUE}")
    } else {
        String::new()
    };
    format!("/image/{index}.png{marker}")
}

fn make_png(index: usize) -> Result<Vec<u8>> {
    let color = Rgb([index as u8, (index >> 8) as u8, (index >> 16) as u8]);
    let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(1, 1, color));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png)?;
    Ok(bytes.into_inner())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpStream,
        thread,
        time::Duration,
    };

    use image::GenericImageView;

    use super::FixtureServer;

    struct HttpResponse {
        status: u16,
        headers: String,
        body: Vec<u8>,
    }

    impl HttpResponse {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers.lines().find_map(|line| {
                let (header_name, value) = line.split_once(": ")?;
                (header_name.eq_ignore_ascii_case(name)).then_some(value)
            })
        }
    }

    fn request(server: &FixtureServer, path: &str) -> HttpResponse {
        let address = format!(
            "{}:{}",
            server.url().host_str().unwrap(),
            server.url().port().unwrap()
        );
        let mut last_error = None;
        for _ in 0..20 {
            match TcpStream::connect(&address) {
                Ok(mut stream) => {
                    stream
                        .write_all(
                            format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                                .as_bytes(),
                        )
                        .unwrap();
                    let mut response = Vec::new();
                    stream.read_to_end(&mut response).unwrap();
                    let body_start = response
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                        .unwrap()
                        + 4;
                    let headers = String::from_utf8(response[..body_start].to_vec()).unwrap();
                    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
                    return HttpResponse {
                        status,
                        headers,
                        body: response[body_start..].to_vec(),
                    };
                }
                Err(error) => {
                    last_error = Some(error);
                    thread::sleep(Duration::from_millis(5));
                }
            }
        }
        panic!(
            "fixture server did not accept a loopback request: {}",
            last_error.unwrap()
        );
    }

    // Production mutation caught: binding to an externally reachable interface, a fixed port, or
    // not serving health would either report the wrong origin or prevent two fixture runs from coexisting.
    #[test]
    fn starts_on_an_ephemeral_ipv4_loopback_origin() {
        let first = FixtureServer::start(2, 1).unwrap();
        let second = FixtureServer::start(2, 1).unwrap();

        assert_eq!(first.url().scheme(), "http");
        assert_eq!(first.url().host_str(), Some("127.0.0.1"));
        assert_ne!(first.url().port(), Some(0));
        assert_ne!(first.url().port(), second.url().port());
        assert_eq!(request(&first, "/health").status, 200);
        assert_eq!(request(&second, "/health").status, 200);
    }

    // Production mutation caught: accepting more than the experiment's 100-image maximum would
    // eagerly generate and retain an unbounded number of encoded fixture PNGs before returning.
    #[test]
    fn rejects_one_hundred_and_one_images_before_generating_fixtures() {
        let error = FixtureServer::start(101, 0).err().unwrap();

        assert_eq!(error.to_string(), "image_count must be within 1..=100");
    }

    // Production mutation caught: omitting image elements or failing to put the fixture marker in
    // the flagged image URL would prevent the controlled page from exercising every fixture branch.
    #[test]
    fn homepage_lists_the_requested_images_and_marks_the_flagged_url() {
        let server = FixtureServer::start(3, 1).unwrap();
        let response = request(&server, "/");
        let page = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(page.matches("<img ").count(), 3);
        assert!(page.contains("/image/0.png\""));
        assert!(page.contains("/image/1.png?omarchy-kids-fixture=flagged\""));
        assert!(page.contains("/image/2.png\""));
    }

    // Production mutation caught: returning one shared PNG, changing a fixture's deterministic
    // index-to-color mapping, or dropping the flagged-response header hides response mix-ups.
    #[test]
    fn image_routes_return_distinct_deterministic_pngs_and_mark_the_flagged_response() {
        let server = FixtureServer::start(2, 1).unwrap();
        let first = request(&server, "/image/0.png");
        let flagged = request(&server, "/image/1.png?omarchy-kids-fixture=flagged");

        assert_eq!(first.status, 200);
        assert_eq!(flagged.status, 200);
        assert_eq!(
            image::load_from_memory(&first.body)
                .unwrap()
                .get_pixel(0, 0)
                .0,
            [0, 0, 0, 255]
        );
        assert_eq!(
            image::load_from_memory(&flagged.body)
                .unwrap()
                .get_pixel(0, 0)
                .0,
            [1, 0, 0, 255]
        );
        assert_ne!(first.body, flagged.body);
        assert!(flagged.headers.contains("x-omarchy-kids-fixture: flagged"));
    }

    // Production mutation caught: redirecting to an unmarked or wrong image, returning decodable
    // corrupt content, or skipping the requested delay leaves later interceptor branches untested.
    #[test]
    fn diagnostic_fixture_routes_preserve_their_response_contracts() {
        let server = FixtureServer::start(2, 0).unwrap();

        let redirect = request(&server, "/redirect.png");
        assert_eq!(redirect.status, 307);
        assert_eq!(
            redirect.header("location"),
            Some("/image/0.png?omarchy-kids-fixture=flagged")
        );

        let corrupt = request(&server, "/corrupt.png");
        assert_eq!(corrupt.status, 200);
        assert!(image::load_from_memory(&corrupt.body).is_err());

        let delayed_at = std::time::Instant::now();
        let slow = request(&server, "/slow/50.png");
        assert_eq!(slow.status, 200);
        assert!(delayed_at.elapsed() >= Duration::from_millis(35));
    }

    // Production mutation caught: leaving a slow handler asleep after shutdown makes the server's
    // synchronous Drop wait for request-controlled time after a browser deadline cancels a run.
    #[test]
    fn shutdown_cancels_an_active_slow_handler_before_joining_the_server_thread() {
        let server = FixtureServer::start(1, 0).unwrap();
        let address = format!(
            "{}:{}",
            server.url().host_str().unwrap(),
            server.url().port().unwrap()
        );
        let (request_written, request_started) = std::sync::mpsc::sync_channel(1);
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(&address).unwrap();
            stream
                .write_all(
                    format!(
                        "GET /slow/500.png HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
            request_written.send(()).unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).unwrap();
        });
        request_started.recv().unwrap();
        thread::sleep(Duration::from_millis(50));

        let shutdown_started = std::time::Instant::now();
        drop(server);
        let shutdown_elapsed = shutdown_started.elapsed();
        client.join().unwrap();

        assert!(
            shutdown_elapsed < Duration::from_millis(250),
            "fixture shutdown waited {shutdown_elapsed:?} for the slow handler"
        );
    }
}
