use crate::{
    domain::{AppUser, UserEmail, UserName, UserRole},
    entities::prelude::User as DbUser,
};
use axum::async_trait;
use axum_login::{secrecy::SecretVec, AuthUser, RequireAuthorizationLayer, UserStore};
use sea_orm::{DatabaseConnection, EntityTrait};
use secrecy::Secret;

type Result<T = ()> = std::result::Result<T, eyre::Error>;

impl AuthUser<UserRole> for AppUser {
    fn get_id(&self) -> String {
        format!("{}", self.id.unwrap_or_default())
    }

    fn get_password_hash(&self) -> axum_login::secrecy::SecretVec<u8> {
        SecretVec::new(self.get_password_hash_as_string().into())
    }

    fn get_role(&self) -> Option<UserRole> {
        Some(self.role)
    }
}

pub type AuthContext = axum_login::extractors::AuthContext<AppUser, SeaOrmStore, UserRole>;
pub type RequireAuth = RequireAuthorizationLayer<AppUser, UserRole>;

#[derive(Debug, Clone)]
pub struct SeaOrmStore {
    conn: DatabaseConnection,
}

impl SeaOrmStore {
    pub fn new(conn: &DatabaseConnection) -> Self {
        Self { conn: conn.clone() }
    }
}

#[async_trait]
impl UserStore<UserRole> for SeaOrmStore
where
    UserRole: PartialOrd + PartialEq + Clone + Send + Sync + 'static,
{
    type User = AppUser;

    async fn load_user(&self, user_id: &str) -> Result<Option<Self::User>> {
        let id = user_id.parse()?;
        let user = DbUser::find_by_id(id).one(&self.conn).await?;
        match user {
            Some(u) => Ok(Some(AppUser {
                id: Some(u.id),
                email: UserEmail::parse(u.email).unwrap_or_default(),
                name: UserName::parse(u.name).unwrap_or_default(),
                password: Some(Secret::from(u.password)),
                role: UserRole::try_from(u.role).unwrap(),
            })),
            None => Ok(None),
        }
    }
}
