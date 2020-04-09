use boondock::{errors::Result, Docker};

#[tokio::main]
async fn main() -> Result<()> {
    let docker = Docker::connect_with_defaults()?;
    println!("{:#?}", docker.system_info().await?);
    Ok(())
}
