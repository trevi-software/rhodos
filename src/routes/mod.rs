use async_redis_session::RedisSessionStore;
use axum::{
    routing::{get, post},
    Router,
};
use axum_sessions::{SameSite, SessionLayer};
use once_cell::sync::OnceCell;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use sea_orm::{ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tower_cookies::{CookieManagerLayer, Key};
use tower_http::trace::TraceLayer;

pub mod admin;
pub mod content;
pub mod health_check;
pub mod index;
pub mod login;
pub mod test;
pub mod user;
pub mod user_confirm;

use health_check::health_check;
use index::index;

use crate::{entities::instance, error::TenantMapError};
use crate::{entities::prelude::*, settings::Settings};

use self::admin::dashboard::admin_dashboard;

const FLASH_COOKIE: &str = "_flash";
static FLASH_KEY: OnceCell<Key> = OnceCell::new();

#[derive(Clone, Debug)]
pub struct TenantData {
    domain: String,
    db: DatabaseConnection,
}

#[derive(Clone, Debug)]
pub struct AppState {
    domain: String,
    rhodos_db: Option<DatabaseConnection>,
    global_config: Settings,
    host_db_map: Arc<RwLock<HashMap<String, TenantData>>>,
}

pub async fn create_routes(
    db: DatabaseConnection,
    global_config: &Settings,
) -> Result<Router, String> {
    // This key will only be valid until the server is restarted,
    // but since we intend to use it for flash cookies only (which
    // last seconds, at most) this is fine.
    let _ = FLASH_KEY.set(Key::from(generate_random_key(64).as_bytes()));

    let session_store = RedisSessionStore::new("redis://127.0.0.1/").map_err(|e| e.to_string())?;
    let session_key = [0u8; 64];
    let session_layer = SessionLayer::new(session_store, &session_key)
        .with_cookie_domain("localhost")
        .with_cookie_path("/")
        .with_same_site_policy(SameSite::Lax)
        .with_session_ttl(Some(std::time::Duration::from_secs(60 * 60 * 24 * 7)))
        .with_secure(false);

    let shared_state = AppState {
        domain: global_config.server.domain.clone(),
        rhodos_db: Some(db),
        global_config: global_config.clone(),
        host_db_map: Arc::new(RwLock::new(HashMap::new())),
    };

    let router = Router::new()
        .route("/", get(index))
        .route("/admin/dashboard", get(admin_dashboard))
        .route("/content", post(content::create))
        .route(
            "/login",
            get(login::get::login_form).post(login::post::login),
        )
        .layer(session_layer)
        .layer(CookieManagerLayer::new())
        .route("/health_check", get(health_check))
        .route("/user", post(user::create))
        .route("/user/confirm", get(user_confirm::confirm))
        .layer(TraceLayer::new_for_http())
        .with_state(shared_state);

    Ok(router)
}

pub async fn get_db_from_host(
    host: &str,
    state: &AppState,
) -> Result<DatabaseConnection, TenantMapError> {
    let mut split = host.split(':');
    let mut key = "".to_string();
    if let Some(i) = split.next() {
        key = i.to_string();
    }

    if key == state.domain {
        if let Some(dbconn) = &state.rhodos_db {
            return Ok(dbconn.clone());
        }
    }
    let res = map_get(&key, state).await;
    match res {
        Ok(td) => Ok(td.db),
        Err(e) => Err(e),
    }
}

async fn map_get(key: &String, state: &AppState) -> Result<TenantData, TenantMapError> {
    // Scope our RwLock
    {
        // Happy path: tenant is already in the Map
        let db_map = state.host_db_map.read().await;
        let found_tenant = db_map.get(key);

        if let Some(value) = found_tenant {
            return Ok(value.clone());
        }
    }

    // Tenant is not in the Map. Search for it in the instance table of the main db.
    //
    if state.rhodos_db.is_none() {
        return Err(TenantMapError::UnexpectedError(
            "no valid connection to main database".to_string(),
        ));
    }
    let db = state.rhodos_db.clone().unwrap();
    let instance = Instance::find()
        .filter(instance::Column::Domain.eq(key.clone()))
        .one(&db)
        .await
        .map_err(|e| TenantMapError::NotFound(e.to_string()))?
        .unwrap();
    let db_url = make_db_uri(&instance);
    let res = map_set(&instance.domain, &db_url, state).await?;
    assert_eq!(res.domain, instance.domain);

    Ok(res)
}

async fn map_set(
    domain: &str,
    db_url: &str,
    state: &AppState,
) -> Result<TenantData, TenantMapError> {
    let db = Database::connect(db_url)
        .await
        .map_err(|e| TenantMapError::NotFound(e.to_string()))?;

    let td = TenantData {
        domain: domain.to_string(),
        db,
    };

    let _ = &state
        .host_db_map
        .write()
        .await
        .insert(domain.to_string(), td.clone());

    Ok(td)
}

fn make_db_uri(inst: &instance::Model) -> String {
    let mut db_host: String = "".to_string();
    let mut db_port: u16 = 0;
    let mut db_user: String = "".to_string();
    let mut db_pass: String = "".to_string();
    let mut user_part: String = "".to_string();
    let mut host_part: String = "".to_string();

    if let Some(du) = inst.db_user.clone() {
        db_user = du;
    }
    if let Some(dpa) = inst.db_password.clone() {
        db_pass = dpa;
    }
    if !db_user.is_empty() {
        user_part = format!("{}:{}", db_user, db_pass);
    }

    if let Some(dh) = inst.db_host.clone() {
        db_host = dh;
    }
    if let Some(dpo) = inst.db_port {
        db_port = dpo as u16;
    }
    if !db_host.is_empty() {
        if !user_part.is_empty() {
            host_part = "@".to_string();
        }
        host_part = format!("{}{}", host_part, db_host);
        if db_port > 0 {
            host_part = format!("{}:{}", host_part, db_port);
        }
    }

    let res = format!(
        "postgres://{}{}/{}",
        user_part,
        host_part,
        inst.db_name.clone()
    );
    println!("db_uri = {}", res);
    res
}

fn generate_random_key(length: usize) -> String {
    let mut rng = thread_rng();
    std::iter::repeat_with(|| rng.sample(Alphanumeric))
        .map(char::from)
        .take(length)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_length_is_variable() {
        let cases = [(25, "25 chars"), (64, "64 chars")];
        for (length, msg) in cases {
            let token = generate_random_key(length);
            assert_eq!(token.len(), length, "Key length is {}", msg);
        }
    }

    #[test]
    fn key_does_not_include_invalid_chars() {
        let invalid_chars = vec![
            ":", "/", "?", "#", "[", "]", "@", "!", "$", "&", "'", "(", ")", "*", "+", ",", ";",
            "=",
        ];
        let token = generate_random_key(50);
        for c in invalid_chars {
            assert!(
                !token.contains(c),
                "Confirmation token does not contain any invalid chars"
            );
        }
    }
}
