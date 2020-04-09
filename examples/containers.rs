use boondock::{errors::Result, ContainerListOptions, Docker};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let docker = Docker::connect_with_defaults()?;
    let opts = ContainerListOptions::default().all();
    let containers = docker.containers(opts).await?;

    for container in &containers {
        println!(
            "{} -> Created: {}, Image: {}, Status: {}",
            container.Id, container.Created, container.Image, container.Status
        );
    }
    Ok(())
}
