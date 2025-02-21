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
 
async fn get_poster(query: web::Query<PosterQuery>, semaphore: web::Data<Arc<Semaphore>>, driver: web::Data<Arc<WebDriver>>) -> HttpResponse {
    let movie_name = query.movie.clone();
    
    let result = tokio::spawn(async move {
        let _permit = semaphore.get_ref().clone().acquire_owned().await.unwrap();
        
        println!("Movie: {}", movie_name);
        let url = format!("https://www.movieposters.com/collections/shop?q={}", movie_name);
        
        // Instead of opening a new tab, navigate in the current window
        driver.goto(&url).await?;
        
        // Find the first img element and get its src attribute
        let imgs = driver.find_all(By::Tag("img")).await?;
        if imgs.len() < 2 {
            return Ok(String::new());
        }
        let img = imgs[1].clone();
        let img_src = img.attr("src").await?;
        
        Ok::<String, Box<dyn std::error::Error + Send + Sync>>(img_src.unwrap_or_default())
    }).await;

    match result {
        Ok(Ok(img_src)) if !img_src.is_empty() => HttpResponse::Ok().body(img_src),
        _ => HttpResponse::InternalServerError().body("Failed to fetch poster image")
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Create semaphore with fewer concurrent requests
    let semaphore = web::Data::new(Arc::new(Semaphore::new(5)));

    // Create multiple WebDriver instances for better concurrency
    let caps = DesiredCapabilities::chrome();
    let driver = WebDriver::new("http://localhost:52388", caps).await.unwrap();
    let driver = web::Data::new(Arc::new(driver));

    println!("Server running at http://127.0.0.1:8080");
    HttpServer::new(move || {
        App::new()
            .app_data(semaphore.clone())
            .app_data(driver.clone())
            .route("/poster", web::get().to(get_poster))
            .route("/", web::get().to(welcome))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await?;

    Ok(())
}
