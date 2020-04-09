#![allow(unused_imports)]

use boondock::{errors::Result, Docker};

#[tokio::main]
async fn main() -> Result<()> {
    /*
    let docker = Docker::connect_with_defaults()?;

    let image = "debian".to_string();
    let tag = "latest".to_string();
    let statuses = docker.create_image(image, tag).await?;

    if let Some(last) = statuses.last() {
        println!("{}", last.status?);
    } else {
        println!("none");
    }
    */
    Ok(())
}
