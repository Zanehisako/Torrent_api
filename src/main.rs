use std::sync::Arc;
use thirtyfour::{prelude::*, DesiredCapabilities};
use tokio::sync::Semaphore;
use futures::future::join_all;

#[tokio::main]
async fn main() -> WebDriverResult<()> {
    let movies = vec![
        "The Shawshank Redemption",
        "The Godfather",
        "Pulp Fiction",
        "The Dark Knight",
        "Fight Club",
        "Inception",
        "Goodfellas",
        "The Matrix",
        "Forrest Gump",
        "Star Wars",
        "The Lord of the Rings",
        "Jurassic Park",
        "Titanic",
        "Avatar",
        "The Avengers",
        "Gladiator",
        "The Silence of the Lambs",
        "Saving Private Ryan",
        "Schindler's List",
        "The Green Mile",
    ];

    let semaphore = Arc::new(Semaphore::new(20)); // Limit concurrent WebDriver instances
    let mut handles = Vec::new();

    for movie in movies {
        let movie_name = movie.to_string();
        let semaphore = semaphore.clone();
        
        let handle = tokio::spawn(async move {
            // Acquire a permit from the semaphore
            let _permit = semaphore.acquire_owned().await.unwrap();

            // Create a new WebDriver instance inside each task
            let caps = DesiredCapabilities::chrome();
            let driver = WebDriver::new("http://localhost:65094", caps).await?;

            println!("Movie: {}", movie_name);
            let url = format!("https://www.movieposters.com/collections/shop?q={}", movie_name);
            driver.goto(url).await?;

            // Quit WebDriver to clean up the session
            driver.quit().await?;

            Ok::<(), WebDriverError>(())
        });

        handles.push(handle);
    }

    // Wait for all tasks to complete
    let results = join_all(handles).await;
    for result in results {
        result.expect("Task panicked").expect("Error navigating to website");
    }

    Ok(())
}
