//! A `hyper` connection wrapper.

use futures::{FutureExt, TryFutureExt};
use hyper::{
    client::{
        connect::{Connected, Connection},
        HttpConnector,
    },
    service::Service,
    Uri,
};
use hyper_rustls::{HttpsConnector, MaybeHttpsStream};
#[cfg(unix)]
use hyperlocal::UnixConnector;
use log::{error, warn};
use rustls::{
    internal::pemfile,
    sign::{CertifiedKey, RSASigningKey},
    Certificate, ClientConfig, PrivateKey, ResolvesClientCert, SignatureScheme,
};
use std::{
    env, fs,
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::errors::{Error, ErrorKind, ResultExt};

/// A more flexible `Result` type than `error-chain` generates.
type Result<T, E = Error> = std::result::Result<T, E>;

/// A wrapper for generic errors.
type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// The underlying stream type for `UnixConnector`, which isn't exported.
#[cfg(unix)]
type UnixStream = <UnixConnector as Service<Uri>>::Response;

/// A connector to either an HTTPS endpoint or a local Unix socket.
#[derive(Clone)]
pub(crate) enum Connector {
    /// Connect via HTTPS (or HTTP).
    Https(HttpsConnector<HttpConnector>),

    /// Connect via a local Unix stream.
    #[cfg(unix)]
    Local(UnixConnector),
}

impl Connector {
    /// Configure an HTTPS/HTTP connector.
    pub(crate) fn https() -> Result<Connector> {
        // This code is adapted from the default configuration setup at
        // https://github.com/ctz/hyper-rustls/blob/69133c8d81442f5efa1d3bba5626049bf1573c22/src/connector.rs#L27-L59

        // Set up HTTP.
        let mut http = HttpConnector::new();
        http.enforce_http(false);

        // Set up SSL parameters.
        let mut config = ClientConfig::new();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        config.ct_logs = Some(&ct_logs::LOGS);

        // Look up any certs managed by the operating system.
        config.root_store = match rustls_native_certs::load_native_certs() {
            Ok(store) => store,
            Err((Some(store), err)) => {
                warn!("could not load all certificates: {}", err);
                store
            }
            Err((None, err)) => {
                warn!("cannot access native certificate store: {}", err);
                config.root_store
            }
        };

        // Add any webpki certs, too, in case the OS is useless.
        config
            .root_store
            .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);

        // Install our Docker CA if we have one.
        if should_enable_tls() {
            let ca_path = docker_ca_pem_path()?;
            let mut rdr = open_buffered(&ca_path)?;
            config
                .root_store
                .add_pem_file(&mut rdr)
                .map_err(|_| format!("error reading {}", ca_path.display()))?;
        }

        // Install a client certificate resolver to find our client cert (if we need one).
        config.client_auth_cert_resolver = Arc::new(DockerClientCertResolver);

        Ok(Connector::Https(HttpsConnector::from((http, config))))
    }

    /// Configure a Unix socket connector.
    #[cfg(unix)]
    pub(crate) fn unix() -> Result<Connector> {
        Ok(Connector::Local(UnixConnector))
    }
}

pub(crate) enum Stream {
    /// An HTTPS or HTTP stream.
    Https(MaybeHttpsStream<<HttpConnector as Service<Uri>>::Response>),

    /// A local Unix stream.
    #[cfg(unix)]
    Local(UnixStream),
}

impl Connection for Stream {
    fn connected(&self) -> Connected {
        match self {
            Stream::Https(https) => https.connected(),
            #[cfg(unix)]
            Stream::Local(local) => local.connected(),
        }
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Stream::Https(https) => Pin::new(https).poll_read(cx, buf),
            #[cfg(unix)]
            Stream::Local(local) => Pin::new(local).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.get_mut() {
            Stream::Https(https) => Pin::new(https).poll_write(cx, buf),
            #[cfg(unix)]
            Stream::Local(local) => Pin::new(local).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            Stream::Https(https) => Pin::new(https).poll_flush(cx),
            #[cfg(unix)]
            Stream::Local(local) => Pin::new(local).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            Stream::Https(https) => Pin::new(https).poll_shutdown(cx),
            #[cfg(unix)]
            Stream::Local(local) => Pin::new(local).poll_shutdown(cx),
        }
    }
}

impl Service<Uri> for Connector {
    type Response = Stream;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self {
            Connector::Https(https) => https.poll_ready(cx),
            #[cfg(unix)]
            Connector::Local(local) => local.poll_ready(cx).map_err(BoxError::from),
        }
    }

    fn call(&mut self, req: Uri) -> Self::Future {
        match self {
            Connector::Https(https) => https.call(req).map_ok(Stream::Https).boxed(),
            #[cfg(unix)]
            Connector::Local(local) => local
                .call(req)
                .map_ok(Stream::Local)
                .map_err(BoxError::from)
                .boxed(),
        }
    }
}

/// A client certificate resolver that looks up Docker client certs the same way
/// the official CLI tools do.
struct DockerClientCertResolver;

impl ResolvesClientCert for DockerClientCertResolver {
    fn resolve(
        &self,
        _acceptable_issuers: &[&[u8]],
        _sigschemes: &[SignatureScheme],
    ) -> Option<CertifiedKey> {
        if self.has_certs() {
            match docker_client_key() {
                Ok(key) => Some(key),
                Err(err) => {
                    error!("error reading Docker client keys: {}", err);
                    None
                }
            }
        } else {
            None
        }
    }

    fn has_certs(&self) -> bool {
        should_enable_tls()
    }
}

fn should_enable_tls() -> bool {
    env::var("DOCKER_TLS_VERIFY").is_ok()
}

/// The default directory in which to look for our Docker certificate files.
fn default_cert_path() -> Result<PathBuf> {
    let from_env = env::var("DOCKER_CERT_PATH").or_else(|_| env::var("DOCKER_CONFIG"));
    if let Ok(ref path) = from_env {
        Ok(Path::new(path).to_owned())
    } else {
        let home = dirs::home_dir().ok_or_else(|| ErrorKind::NoCertPath)?;
        Ok(home.join(".docker"))
    }
}

/// Path to `ca.pem`.
fn docker_ca_pem_path() -> Result<PathBuf> {
    let dir = default_cert_path()?;
    Ok(dir.join("ca.pem"))
}

/// Our Docker client credentials, if we have them.
fn docker_client_key() -> Result<CertifiedKey> {
    let dir = default_cert_path()?;

    // Look up our certificates.
    let mut all_certs = certs(&dir.join("cert.pem"))?;
    all_certs.extend(certs(&dir.join("ca.pem"))?.into_iter());

    // Look up our keys.
    let key_path = dir.join("key.pem");
    let mut all_keys = keys(&key_path)?;
    let key = if all_keys.len() == 1 {
        all_keys.remove(0)
    } else {
        return Err(format!(
            "expected 1 private key in {}, found {}",
            key_path.display(),
            all_keys.len()
        )
        .into());
    };
    let signing_key = RSASigningKey::new(&key)
        .map_err(|_| format!("could not parse signing key from {}", key_path.display()))?;

    Ok(CertifiedKey::new(
        all_certs,
        Arc::new(Box::new(signing_key)),
    ))
}

/// Fetch any certificates stored at `path`.
fn certs(path: &Path) -> Result<Vec<Certificate>> {
    let mut rdr = open_buffered(path)?;
    Ok(pemfile::certs(&mut rdr).map_err(|_| format!("cannot read {}", path.display()))?)
}

/// Fetch any keys stored at `path`.
fn keys(path: &Path) -> Result<Vec<PrivateKey>> {
    // Look for pcks8 keys.
    let mut rdr = open_buffered(path)?;
    let mut keys = pemfile::pkcs8_private_keys(&mut rdr)
        .map_err(|_| format!("cannot read {}", path.display()))?;

    // Re-open and look for RSA keys.
    rdr = open_buffered(path)?;
    keys.extend(
        pemfile::rsa_private_keys(&mut rdr)
            .map_err(|_| format!("cannot read {}", path.display()))?,
    );
    Ok(keys)
}

/// Open a path and return a buffered reader.
fn open_buffered(path: &Path) -> Result<io::BufReader<fs::File>> {
    let f = fs::File::open(path).chain_err(|| format!("cannot open {}", path.display()))?;
    Ok(io::BufReader::new(f))
}
