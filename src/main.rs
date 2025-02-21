use std::{sync::Arc, time::Duration, collections::HashMap};
use std::sync::RwLock;
use thirtyfour::{prelude::*, DesiredCapabilities};
use tokio::{process::Command, sync::Semaphore, time::sleep};
use actix_web::{web, App, HttpServer, HttpResponse};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct PosterQuery {
    movie: String,
}

// Add a struct to track cache entry metadata
#[derive(Deserialize, Serialize, Clone)]
struct CacheEntry {
    url: String,
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
        
        // Don't allow more than 20000 entries
        if cache.len() >= 20000 {
            // Remove least accessed entries before inserting new one
            let mut entries: Vec<_> = cache.keys().cloned().collect();
            entries.sort_by_key(|k| cache.get(k).unwrap().access_count);
            if let Some(key) = entries.first() {
                cache.remove(&key.clone());
            }
        }
    }
    
    let result = tokio::spawn(async move {
        let _permit = data.semaphore.clone().acquire_owned().await.unwrap();
        
        println!("Movie: {}", movie_name);
        let url = format!("https://www.movieposters.com/collections/shop?q={}", movie_name);
        
        data.driver.goto(&url).await?;
        
        // Use a more specific selector for faster lookup
        let img = match data.driver.find(By::ClassName("ss_img_load")).await {
            Ok(element) => element,
            Err(_) => return Ok(String::new()),
        };
        
        let img_src = img.attr("src").await?;
        
        // Store result in cache before returning
        if let Some(src) = &img_src {
            data.cache.write().unwrap().insert(movie_name, CacheEntry {
                url: src.clone(),
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

// Simplified clean_cache function
async fn clean_cache(cache: &RwLock<HashMap<String, CacheEntry>>) {
    let mut cache = cache.write().unwrap();
    
    // Check if we've exceeded the threshold
    if cache.len() >= 10000 {
        let mut entries: Vec<_> = cache.keys().cloned().collect();
        
        // Sort by access count (ascending) - least accessed first
        entries.sort_by_key(|k| cache.get(k).unwrap().access_count);
        
        // Remove entries until we're back to 80% of max capacity (16000)
        let remove_count = entries.len().saturating_sub(16000);
        for key in entries.iter().take(remove_count) {
            cache.remove(key);
        }
    }
}

// Add new handler for getting all posters
async fn get_all_posters(data: web::Data<AppState>) -> HttpResponse {
    let cache = data.cache.read().unwrap();
    let posters: HashMap<String, CacheEntry> = cache.iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    HttpResponse::Ok().json(posters)
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
            .route("/posters", web::get().to(get_all_posters))
            .route("/", web::get().to(welcome))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await?;
    child.kill().await?;
    Ok(())
}
