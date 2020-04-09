use boondock::{errors::Result, Docker};

#[tokio::main]
async fn main() -> Result<()> {
    let docker = Docker::connect_with_defaults()?;
    let images = docker.images(false).await?;

    for image in &images {
        println!(
            "{} -> Size: {}, Virtual Size: {}, Created: {}",
            image.Id, image.Size, image.VirtualSize, image.Created
        );
    }
    Ok(())
}
