use std::{sync::Arc, time::Duration, collections::HashMap};
use std::sync::RwLock;
use actix_cors::Cors;
use thirtyfour::{prelude::*, DesiredCapabilities};
use tokio::{process::Command, sync::Semaphore, time::sleep};
use actix_web::{web, App, HttpServer, HttpResponse};
use serde::{Deserialize, Serialize};
use rusqlite::params;
use r2d2_sqlite::SqliteConnectionManager;
use r2d2::Pool;

#[derive(Deserialize)]
struct PosterQuery {
    movie: String,
}

#[derive(Deserialize, Serialize, Clone)]
struct CacheEntry {
    url: String,
    access_count: u32,
}

struct AppState {
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    semaphore: Arc<Semaphore>,
    driver: Arc<WebDriver>,
    db_pool: Pool<SqliteConnectionManager>,
}

// Initialize database schema
async fn init_db(pool: &Pool<SqliteConnectionManager>) -> Result<(), rusqlite::Error> {
    let conn = pool.get().unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS posters (
            movie_name TEXT PRIMARY KEY,
            url TEXT NOT NULL,
            access_count INTEGER NOT NULL,
            last_accessed INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_access_count ON posters(access_count);"
    )?;
    Ok(())
}

async fn welcome() -> HttpResponse {
    HttpResponse::Ok().body(format!("Successfully connected"))
}

async fn get_poster(
    query: web::Query<PosterQuery>,
    data: web::Data<AppState>
) -> HttpResponse {
    let movie_name = query.movie.clone();
    
    // Check memory cache first
    {
        let mut cache = data.cache.write().unwrap();
        if let Some(entry) = cache.get_mut(&movie_name) {
            entry.access_count += 1;
            
            // Update database access count asynchronously
            let pool = data.db_pool.clone();
            let movie = movie_name.clone();
            let count = entry.access_count;
            tokio::spawn(async move {
                if let Ok(conn) = pool.get() {
                    let _ = conn.execute(
                        "UPDATE posters SET access_count = ?, last_accessed = unixepoch() 
                         WHERE movie_name = ?",
                        params![count, movie],
                    );
                }
            });
            
            return HttpResponse::Ok().body(entry.url.clone());
        }
    }
    
    // Check SQLite if not in memory cache
    if let Ok(conn) = data.db_pool.get() {
        match conn.query_row(
            "SELECT url, access_count FROM posters WHERE movie_name = ?",
            params![movie_name],
            |row| {
                Ok(CacheEntry {
                    url: row.get(0)?,
                    access_count: row.get(1)?,
                })
            },
        ) {
            Ok(entry) => {
                // Update access count
                let _ = conn.execute(
                    "UPDATE posters SET access_count = access_count + 1, 
                     last_accessed = unixepoch() WHERE movie_name = ?",
                    params![movie_name],
                );
                
                // Add to memory cache
                data.cache.write().unwrap().insert(movie_name, entry.clone());
                return HttpResponse::Ok().body(entry.url);
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(_) => return HttpResponse::InternalServerError().body("Database error"),
        }
    }
    
    // If not found in either cache, scrape it
    let result = tokio::spawn(async move {
        let _permit = data.semaphore.clone().acquire_owned().await.unwrap();
        
        println!("Scraping movie: {}", movie_name);
        let url = format!("https://www.movieposters.com/collections/shop?q={}", movie_name);
        
        data.driver.goto(&url).await?;
        
        let img = match data.driver.find(By::ClassName("ss_img_load")).await {
            Ok(element) => element,
            Err(_) => return Ok(String::new()),
        };
        
        let img_src = img.attr("src").await?;
        
        if let Some(src) = &img_src {
            // Store in memory cache
            let entry = CacheEntry {
                url: src.clone(),
                access_count: 1,
            };
            data.cache.write().unwrap().insert(movie_name.clone(), entry.clone());
            
            // Store in database
            if let Ok(conn) = data.db_pool.get() {
                let _ = conn.execute(
                    "INSERT OR REPLACE INTO posters (movie_name, url, access_count, last_accessed) 
                     VALUES (?, ?, ?, unixepoch())",
                    params![movie_name, src, 1],
                );
            }
        }
        
        Ok::<String, Box<dyn std::error::Error + Send + Sync>>(img_src.unwrap_or_default())
    }).await;

    match result {
        Ok(Ok(img_src)) if !img_src.is_empty() => HttpResponse::Ok().body(img_src),
        _ => HttpResponse::InternalServerError().body("Failed to fetch poster image")
    }
}

// Modified clean_cache to also clean database
async fn clean_cache(
    cache: &RwLock<HashMap<String, CacheEntry>>,
    pool: &Pool<SqliteConnectionManager>
) {
    let mut cache = cache.write().unwrap();
    
    if cache.len() >= 10000 {
        if let Ok(conn) = pool.get() {
            // Clean database first
            let _ = conn.execute(
                "DELETE FROM posters 
                 WHERE movie_name IN (
                     SELECT movie_name FROM posters 
                     ORDER BY access_count ASC 
                     LIMIT ?
                 )",
                params![cache.len() - 8000],
            );
            
            // Get remaining entries from database
            let mut stmt = conn.prepare(
                "SELECT movie_name, url, access_count FROM posters"
            ).unwrap();
            
            let entries: HashMap<String, CacheEntry> = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    CacheEntry {
                        url: row.get(1)?,
                        access_count: row.get(2)?,
                    }
                ))
            })
            .unwrap()
            .filter_map(Result::ok)
            .collect();
            
            // Update memory cache
            *cache = entries;
        }
    }
}

async fn get_all_posters(data: web::Data<AppState>) -> HttpResponse {
    let cache = data.cache.read().unwrap();
    let posters: HashMap<String, CacheEntry> = cache.iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    HttpResponse::Ok().json(posters)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let mut child = Command::new("./chromedriver_PATCHED.exe")
        .arg("--port=9515")
        .spawn()?;

    sleep(Duration::from_secs(2)).await;

    let caps = DesiredCapabilities::chrome();
    let driver = WebDriver::new("http://localhost:9515", caps).await.unwrap();

    // Initialize SQLite connection pool
    let manager = SqliteConnectionManager::file("posters.db");
    let pool = Pool::new(manager).expect("Failed to create pool");
    
    // Initialize database
    init_db(&pool).await.expect("Failed to initialize database");

    let app_state = web::Data::new(AppState {
        cache: Arc::new(RwLock::new(HashMap::new())),
        semaphore: Arc::new(Semaphore::new(10000)),
        driver: Arc::new(driver),
        db_pool: pool.clone(),
    });

    // Modified cache cleaning task
    let cache = Arc::clone(&app_state.cache);
    let pool_clone = pool.clone();
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(3600)).await;
            clean_cache(cache.as_ref(), &pool_clone).await;
        }
    });

    println!("Server running at http://127.0.0.1:8080");
    HttpServer::new(move || {
        App::new()
            .wrap(
                Cors::default()
                    .allow_any_origin()
                    .send_wildcard()
            )
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