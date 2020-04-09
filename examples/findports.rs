use boondock::errors::*;
use boondock::{ContainerListOptions, Docker};
use std::io::{self, Write};

async fn find_all_exported_ports() -> Result<()> {
    let docker = Docker::connect_with_defaults()?;
    let containers = docker.containers(ContainerListOptions::default()).await?;
    for container in &containers {
        let info = docker.container_info(&container).await?;

        // Uncomment this to dump everything we know about a container.
        //println!("{:#?}", &info);

        if let Some(ports) = info.NetworkSettings.Ports {
            let ports: Vec<String> = ports.keys().cloned().collect();
            println!("{}: {}", &info.Name, ports.join(", "));
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(err) = find_all_exported_ports().await {
        write!(io::stderr(), "Error: ")?;
        for e in err.iter() {
            write!(io::stderr(), "{}\n", e)?;
        }
    }
    Ok(())
}
