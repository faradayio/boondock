use boondock::{errors::Result, ContainerListOptions, Docker};

#[tokio::main]
async fn main() -> Result<()> {
    let docker = Docker::connect_with_defaults()?;
    let opts = ContainerListOptions::default();
    if let Some(container) = docker.containers(opts).await?.get(0) {
        for process in docker.processes(container).await? {
            println!("{:#?}", process);
        }
    }
    Ok(())
}
