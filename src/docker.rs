use hyper::{client::Client, Body, Request, Response, Uri};
use std::{convert::TryFrom, env};
use tokio::stream::StreamExt;

use crate::connector::Connector;
use crate::container::{Container, ContainerInfo};
use crate::errors::*;
use crate::filesystem::FilesystemChange;
use crate::image::Image;
use crate::options::*;
use crate::process::{Process, Top};
use crate::system::SystemInfo;
use crate::version::Version;

use serde::de::DeserializeOwned;
use serde_json;

/// The default `DOCKER_HOST` address that we will try to connect to.
#[cfg(unix)]
pub const DEFAULT_DOCKER_HOST: &'static str = "unix:///var/run/docker.sock";

/// The default `DOCKER_HOST` address that we will try to connect to.
///
/// This should technically be `"npipe:////./pipe/docker_engine"` on
/// Windows, but we don't support Windows pipes yet.  However, the TCP port
/// is still available.
#[cfg(windows)]
pub const DEFAULT_DOCKER_HOST: &'static str = "tcp://localhost:2375";

/// Used to build URLs.
enum UrlBuilder {
    Https(String),
    #[cfg(unix)]
    Local(String),
}

impl UrlBuilder {
    fn build_url(&self, path: &str) -> Result<Uri> {
        match self {
            Self::Https(base) => Ok(Uri::try_from(format!("{}{}", base, path))
                .map_err(|err| format!("cannot parse URL {}{}: {}", base, path, err))?),
            #[cfg(unix)]
            Self::Local(base) => Ok(Uri::from(hyperlocal::Uri::new(base, path))),
        }
    }
}

/// Our Docker client.
pub struct Docker {
    client: Client<Connector, Body>,
    url_builder: UrlBuilder,
}

impl Docker {
    /// Connect to the Docker daemon using the standard Docker
    /// configuration options.  This includes `DOCKER_HOST`,
    /// `DOCKER_TLS_VERIFY`, `DOCKER_CERT_PATH` and `DOCKER_CONFIG`, and we
    /// try to interpret these as much like the standard `docker` client as
    /// possible.
    pub fn connect_with_defaults() -> Result<Docker> {
        // Read in our configuration from the Docker environment.
        let host = env::var("DOCKER_HOST").unwrap_or(DEFAULT_DOCKER_HOST.to_string());

        // Dispatch to the correct connection function.
        let mkerr = || ErrorKind::CouldNotConnect(host.clone());
        if host.starts_with("unix://") {
            Docker::connect_with_unix(&host).chain_err(&mkerr)
        } else if host.starts_with("tcp://") {
            Docker::connect_with_ssl(&host).chain_err(&mkerr)
        } else {
            Err(ErrorKind::UnsupportedScheme(host.clone()).into())
        }
    }

    #[cfg(unix)]
    pub fn connect_with_unix(addr: &str) -> Result<Docker> {
        // Get our bare path.
        let client_addr = if addr.starts_with("unix://") {
            addr.replacen("unix://", "", 1)
        } else {
            addr.to_owned()
        };
        let client = Client::builder().build(Connector::unix()?);
        Ok(Docker {
            client,
            url_builder: UrlBuilder::Local(client_addr),
        })
    }

    #[cfg(not(unix))]
    pub fn connect_with_unix(addr: &str) -> Result<Docker> {
        Err(ErrorKind::UnsupportedScheme(addr.to_owned()).into())
    }

    pub fn connect_with_ssl(addr: &str) -> Result<Docker> {
        // This ensures that using docker-machine-esque addresses work with Hyper.
        let client_addr = if addr.starts_with("tcp://") {
            addr.replacen("tcp://", "https://", 1)
        } else {
            addr.to_owned()
        };

        let client = Client::builder().build(Connector::https()?);
        Ok(Docker {
            client,
            url_builder: UrlBuilder::Https(client_addr),
        })
    }

    fn get_url(&self, path: &str) -> Result<Uri> {
        self.url_builder.build_url(path)
    }

    fn build_empty_get_request(&self, request_url: &Uri) -> Result<Request<Body>> {
        Ok(Request::get(request_url)
            .body(Body::empty())
            .chain_err(|| "error building request")?)
    }

    /*
    fn build_post_request(&self, request_url: &Uri) -> Builder {
        Request::post(request_url)
    }
    */

    async fn start_request(&self, request: Request<Body>) -> Result<Response<Body>> {
        let response = self.client.request(request).await?;
        if response.status().is_success() {
            Ok(response)
        } else {
            Err(format!("HTTP request failed: {}", response.status()).into())
        }
    }

    async fn execute_request(&self, request: Request<Body>) -> Result<Vec<u8>> {
        let response = self.start_request(request).await?;
        let mut data = vec![];
        let mut stream = response.into_body();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            data.extend(&chunk[..]);
        }
        Ok(data)
    }

    /*
    fn arrayify(&self, s: &[u8]) -> String {
        let wrapped = format!("[{}]", s);
        wrapped.clone().replace(b"}\r\n{", b"}{").replace(b"}{", b"},{")
    }
    */

    /// `GET` a URL and decode it.
    async fn decode_url<'a, T>(&'a self, type_name: &'static str, url: &'a str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let request_url = self.get_url(url)?;
        let request = self.build_empty_get_request(&request_url)?;
        let body = self.execute_request(request).await?;
        let info = serde_json::from_slice::<T>(&body).chain_err(|| {
            ErrorKind::ParseError(type_name, String::from_utf8_lossy(&body[..]).into_owned())
        })?;
        Ok(info)
    }

    pub async fn containers(&self, opts: ContainerListOptions) -> Result<Vec<Container>> {
        let url = format!("/containers/json?{}", opts.to_url_params());
        self.decode_url("Container", &url).await
    }

    pub async fn processes(&self, container: &Container) -> Result<Vec<Process>> {
        let url = format!("/containers/{}/top", container.Id);
        let top: Top = self.decode_url("Top", &url).await?;

        let mut processes: Vec<Process> = Vec::new();
        let mut process_iter = top.Processes.iter();
        loop {
            let process = match process_iter.next() {
                Some(process) => process,
                None => {
                    break;
                }
            };

            let mut p = Process {
                user: String::new(),
                pid: String::new(),
                cpu: None,
                memory: None,
                vsz: None,
                rss: None,
                tty: None,
                stat: None,
                start: None,
                time: None,
                command: String::new(),
            };

            let mut value_iter = process.iter();
            let mut i: usize = 0;
            loop {
                let value = match value_iter.next() {
                    Some(value) => value,
                    None => {
                        break;
                    }
                };
                let key = &top.Titles[i];
                match key.as_ref() {
                    "UID" => p.user = value.clone(),
                    "USER" => p.user = value.clone(),
                    "PID" => p.pid = value.clone(),
                    "%CPU" => p.cpu = Some(value.clone()),
                    "%MEM" => p.memory = Some(value.clone()),
                    "VSZ" => p.vsz = Some(value.clone()),
                    "RSS" => p.rss = Some(value.clone()),
                    "TTY" => p.tty = Some(value.clone()),
                    "STAT" => p.stat = Some(value.clone()),
                    "START" => p.start = Some(value.clone()),
                    "STIME" => p.start = Some(value.clone()),
                    "TIME" => p.time = Some(value.clone()),
                    "CMD" => p.command = value.clone(),
                    "COMMAND" => p.command = value.clone(),
                    _ => {}
                }

                i = i + 1;
            }
            processes.push(p);
        }

        Ok(processes)
    }

    /*
    pub async fn stats(&self, container: &Container) -> Result<StatsReader> {
        if container.Status.contains("Up") == false {
            return Err("The container is already stopped.".into());
        }

        let request_url = self.get_url(&format!("/containers/{}/stats", container.Id));
        let request = self
            .build_empty_get_request(&request_url)?;
        let response = self.start_request(request).await?;
        Ok(StatsReader::new(response))
    }

    pub async fn create_image(&self, image: String, tag: String) -> Result<Vec<ImageStatus>> {
        let request_url = self.get_url(&format!("/images/create?fromImage={}&tag={}", image, tag));
        let request = self
            .build_post_request(&request_url)
            .body(Body::empty())
            .chain_err(|| "error building request")?;
        let body = self.execute_request(request).await?;
        let fixed = self.arrayify(&body);
        let statuses = serde_json::from_str::<Vec<ImageStatus>>(&fixed)
            .chain_err(|| ErrorKind::ParseError("ImageStatus", fixed))?;
        Ok(statuses)
    }
    */

    pub async fn images(&self, all: bool) -> Result<Vec<Image>> {
        let a = match all {
            true => "1",
            false => "0",
        };
        let url = format!("/images/json?a={}", a);
        self.decode_url("Image", &url).await
    }

    pub async fn system_info(&self) -> Result<SystemInfo> {
        self.decode_url("SystemInfo", &format!("/info")).await
    }

    pub async fn container_info(&self, container: &Container) -> Result<ContainerInfo> {
        let url = format!("/containers/{}/json", container.Id);
        self.decode_url("ContainerInfo", &url)
            .await
            .chain_err(|| ErrorKind::ContainerInfo(container.Id.clone()))
    }

    pub async fn filesystem_changes(&self, container: &Container) -> Result<Vec<FilesystemChange>> {
        let url = format!("/containers/{}/changes", container.Id);
        self.decode_url("FilesystemChange", &url).await
    }

    pub async fn export_container(&self, container: &Container) -> Result<Response<Body>> {
        let url = format!("/containers/{}/export", container.Id);
        let request_url = self.get_url(&url)?;
        let request = self.build_empty_get_request(&request_url)?;
        let response = self.start_request(request).await?;
        Ok(response)
    }

    pub async fn ping(&self) -> Result<Vec<u8>> {
        let request_url = self.get_url("/_ping")?;
        let request = self.build_empty_get_request(&request_url)?;
        self.execute_request(request).await
    }

    pub async fn version(&self) -> Result<Version> {
        self.decode_url("Version", "/version").await
    }
}
