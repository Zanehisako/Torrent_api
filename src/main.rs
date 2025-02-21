use std::sync::Arc;
use thirtyfour::{prelude::*, DesiredCapabilities};
use tokio::sync::Semaphore;
use actix_web::{web, App, HttpServer, HttpResponse};
use serde::Deserialize;

#[derive(Deserialize)]
struct PosterQuery {
    movie: String,
}

async fn welcome() ->HttpResponse {
         return HttpResponse::Ok().body(format!("Successfully connected"));
}
 
async fn get_poster(query: web::Query<PosterQuery>, semaphore: web::Data<Arc<Semaphore>>) -> HttpResponse {
    let movie_name = query.movie.clone();
    
    // Spawn task to fetch poster
    let result = tokio::spawn(async move {
        // Clone the Arc before calling acquire_owned
        let _permit = semaphore.get_ref().clone().acquire_owned().await.unwrap();

        // Create a new WebDriver instance inside each task
        let caps = DesiredCapabilities::chrome();
        match WebDriver::new("http://localhost:55137", caps).await {
            Ok(driver) => {
                println!("Movie: {}", movie_name);
                let url = format!("https://www.movieposters.com/collections/shop?q={}", movie_name);
                
                // Navigate to the URL
                driver.goto(url).await?;
                
                // Find the first img element and get its src attribute
                let imgs = driver.find_all(By::Tag("img")).await?;
                let img = imgs[1].clone();
                let img_src = img.attr("src").await?;
                println!("img src: {}",img.clone().class_name().await.unwrap().unwrap());
                
                // Quit WebDriver to clean up the session
                let _ = driver.quit().await;
                
                Ok(img_src.unwrap_or_default())
            },
            Err(e) => Err(e)
        }
    }).await;

    match result {
        Ok(Ok(img_src)) => HttpResponse::Ok().body(img_src),
        _ => HttpResponse::InternalServerError().body("Failed to fetch poster image")
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Create semaphore to limit concurrent WebDriver instances
    let semaphore = web::Data::new(Arc::new(Semaphore::new(20)));

    println!("Server running at http://127.0.0.1:8080");
    HttpServer::new(move || {
        App::new()
            .app_data(semaphore.clone())
            .route("/poster", web::get().to(get_poster))
            .route("/", web::get().to(welcome))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await?;

    Ok(())
}
