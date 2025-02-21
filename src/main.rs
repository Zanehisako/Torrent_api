use std::{sync::Arc, time::Duration, collections::HashMap, time::SystemTime};
use std::sync::RwLock;
use thirtyfour::{prelude::*, DesiredCapabilities};
use tokio::{process::Command, sync::Semaphore, time::sleep};
use actix_web::{web, App, HttpServer, HttpResponse};
use serde::Deserialize;

#[derive(Deserialize)]
struct PosterQuery {
    movie: String,
}

// Add a struct to track cache entry metadata
struct CacheEntry {
    url: String,
    timestamp: SystemTime,
    access_count: u32,
}

struct AppState {
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    semaphore: Arc<Semaphore>,
    driver: Arc<WebDriver>,
}

async fn welcome() ->HttpResponse {
         return HttpResponse::Ok().body(format!("Successfully connected"));
}
 
async fn get_poster(
    query: web::Query<PosterQuery>, 
    data: web::Data<AppState>
) -> HttpResponse {
    let movie_name = query.movie.clone();
    
    // Check cache first and update access count
    {
        let mut cache = data.cache.write().unwrap();
        if let Some(entry) = cache.get_mut(&movie_name) {
            entry.access_count += 1;
            return HttpResponse::Ok().body(entry.url.clone());
        }
    }
    
    let result = tokio::spawn(async move {
        let _permit = data.semaphore.clone().acquire_owned().await.unwrap();
        
        println!("Movie: {}", movie_name);
        let url = format!("https://www.movieposters.com/collections/shop?q={}", movie_name);
        
        // Instead of opening a new tab, navigate in the current window
        data.driver.goto(&url).await?;
        
        // Find the first img element and get its src attribute
        let imgs = data.driver.find_all(By::Tag("img")).await?;
        if imgs.len() < 2 {
            return Ok(String::new());
        }
        let img = imgs[1].clone();
        let img_src = img.attr("src").await?;
        
        // Store result in cache before returning
        if let Some(src) = &img_src {
            data.cache.write().unwrap().insert(movie_name, CacheEntry {
                url: src.clone(),
                timestamp: SystemTime::now(),
                access_count: 1,
            });
        }
        
        Ok::<String, Box<dyn std::error::Error + Send + Sync>>(img_src.unwrap_or_default())
    }).await;

    match result {
        Ok(Ok(img_src)) if !img_src.is_empty() => HttpResponse::Ok().body(img_src),
        _ => HttpResponse::InternalServerError().body("Failed to fetch poster image")
    }
}

// Add a new function to clean the cache
async fn clean_cache(cache: &RwLock<HashMap<String, CacheEntry>>) {
    let day = Duration::from_secs(24 * 60 * 60);
    let now = SystemTime::now();
    
    let mut cache = cache.write().unwrap();
    let mut to_remove: Vec<String> = Vec::new();
    
    // Find entries older than a day
    for (key, entry) in cache.iter() {
        if now.duration_since(entry.timestamp).unwrap() > day {
            to_remove.push(key.clone());
        }
    }
    
    // If we need to remove entries, keep the most accessed ones
    if !to_remove.is_empty() {
        // Sort by access count (ascending)
        to_remove.sort_by_key(|k| cache.get(k).unwrap().access_count);
        // Remove the least accessed entries (keeping 20% of old entries)
        let remove_count = (to_remove.len() * 80) / 100;
        for key in to_remove.iter().take(remove_count) {
            cache.remove(key);
        }
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Create semaphore with fewer concurrent requests
      // Start ChromeDriver process
    let mut child = Command::new("./chromedriver_PATCHED.exe")
        .arg("--port=9515")  // Set the port
        .spawn()?;

    // Wait a bit to ensure ChromeDriver is running
    sleep(Duration::from_secs(2)).await;

    // Create multiple WebDriver instances for better concurrency
    let caps = DesiredCapabilities::chrome();
    let driver = WebDriver::new("http://localhost:9515", caps).await.unwrap();

    let app_state = web::Data::new(AppState {
        cache: Arc::new(RwLock::new(HashMap::new())),
        semaphore: Arc::new(Semaphore::new(10000)),
        driver: Arc::new(driver),
    });

    // Spawn cache cleaning task
    let cache = Arc::clone(&app_state.cache);
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(3600)).await; // Check every hour
            clean_cache(cache.as_ref()).await;
        }
    });

    println!("Server running at http://127.0.0.1:8080");
    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .route("/poster", web::get().to(get_poster))
            .route("/", web::get().to(welcome))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await?;
    child.kill().await?;
    Ok(())
}
