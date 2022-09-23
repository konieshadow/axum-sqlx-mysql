use std::str::FromStr;

use anyhow::{Context};
use argon2::{password_hash::SaltString, PasswordHash, Argon2};
use axum::{extract::Extension, Json, Router, routing::{post, get}};
use uuid::Uuid;

use super::{ApiContext, Result, ResultExt, Error, extractor::AuthUser};

pub fn router() -> Router {
    Router::new()
        .route("/api/users", post(create_user))
        .route("/api/users/login", post(login_user))
        .route("/api/user", get(get_current_user).put(update_user))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct UserBody<T> {
    user: T,
}

#[derive(serde::Deserialize)]
struct NewUser {
    username: String,
    email: String,
    password: String,
}

#[derive(serde::Deserialize)]
struct LoginUser {
    email: String,
    password: String,
}

#[derive(serde::Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
struct UpdateUser {
    email: Option<String>,
    username: Option<String>,
    password: Option<String>,
    bio: Option<String>,
    image: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct User {
    email: String,
    token: String,
    username: String,
    bio: String,
    image: Option<String>,
}

async fn create_user(
    ctx: Extension<ApiContext>,
    Json(req): Json<UserBody<NewUser>>,
) -> Result<Json<UserBody<User>>> {
    let password_hash = hash_password(req.user.password).await?;
    
    let user_id = Uuid::new_v4();
    sqlx::query!(
        r#"
insert into user (user_id, username, email, password_hash) values (?, ?, ?, ?)
        "#,
        user_id.to_string(),
        req.user.username,
        req.user.email,
        password_hash,
    ).execute(&ctx.db)
        .await
        .on_constraint("key_username", |_| {
            Error::unprocessable_entity([("usernamem", "username taken")])
        })
        .on_constraint("key_email", |_| {
            Error::unprocessable_entity([("email", "email token")])
        })?;

    Ok(Json(UserBody {
        user: User {
            email: req.user.email,
            token: AuthUser { user_id }.to_jwt(&ctx),
            username: req.user.username,
            bio: "".to_string(),
            image: None,
        },
    }))
}

async fn login_user(
    ctx: Extension<ApiContext>,
    Json(req): Json<UserBody<LoginUser>>,
) -> Result<Json<UserBody<User>>> {
    let user = sqlx::query!(
        r#"
select user_id, email, username, bio, image, password_hash
from user where email = ?
        "#,
        req.user.email,
    )
        .fetch_optional(&ctx.db)
        .await?
        .ok_or_else(|| Error::unprocessable_entity([("email", "does not exists")]))?;

    verify_password(req.user.password, user.password_hash).await?;

    Ok(Json(UserBody{
        user: User {
            email: user.email,
            token: AuthUser {
                user_id: Uuid::from_str(&user.user_id).unwrap(),
            }
            .to_jwt(&ctx),
            username: user.username,
            bio: user.bio,
            image: user.image,
        },
    }))
}

async fn update_user(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
    Json(req): Json<UserBody<UpdateUser>>,
) -> Result<Json<UserBody<User>>> {
    if req.user == UpdateUser::default() {
        return get_current_user(auth_user, ctx).await;
    }

    let password_hash = if let Some(password) = req.user.password {
        Some(hash_password(password).await?)
    } else {
        None
    };

    let mut tx = ctx.db.begin().await?;

    sqlx::query!(
        r#"
update user
set email = coalesce(?, user.email),
    username = coalesce(?, user.username),
    password_hash = coalesce(?, user.password_hash),
    bio = coalesce(?, user.bio),
    image = coalesce(?, user.image)
where user_id = ?
        "#,
        req.user.email,
        req.user.username,
        password_hash,
        req.user.bio,
        req.user.image,
        auth_user.user_id.to_string()
    )
        .execute(&mut tx)
        .await
        .on_constraint("key_username", |_| {
            Error::unprocessable_entity([("usernamem", "username taken")])
        })
        .on_constraint("key_email", |_| {
            Error::unprocessable_entity([("email", "email token")])
        })?;

    let user = sqlx::query!(
        r#"
select email, username, bio, image from user where user_id = ?
        "#,
        auth_user.user_id.to_string()
    )
        .fetch_one(&mut tx)
        .await?;

    tx.commit().await?;

    Ok(Json(UserBody{
        user: User {
            email: user.email,
            token: auth_user.to_jwt(&ctx),
            username: user.username,
            bio: user.bio,
            image: user.image,
        },
    }))
}

async fn get_current_user(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
) -> Result<Json<UserBody<User>>> {
    let user = sqlx::query!(
        r#"
select email, username, bio, image from user where user_id = ?
        "#,
        auth_user.user_id.to_string()
    )
    .fetch_one(&ctx.db)
    .await?;

    Ok(Json(UserBody {
        user: User {
            email: user.email,
            token: auth_user.to_jwt(&ctx),
            username: user.username,
            bio: user.bio,
            image: user.image,
        },
    }))
}

async fn hash_password(password: String) -> Result<String> {
    tokio::task::spawn_blocking(move || -> Result<String> {
        let salt = SaltString::generate(rand::thread_rng());
        Ok(
            PasswordHash::generate(Argon2::default(), password, salt.as_str())
                .map_err(|e| anyhow::anyhow!("failed to generate password hash: {}", e))?
                .to_string(),
        )
    })
    .await
    .context("panic in generating password hash")?
}

async fn verify_password(password: String, password_hash: String) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        let hash = PasswordHash::new(&password_hash)
            .map_err(|e| anyhow::anyhow!("invalid password hash: {}", e))?;

        hash.verify_password(&[&Argon2::default()], password)
            .map_err(|e| match e {
                argon2::password_hash::Error::Password => Error::Unauthorized,
                _ => anyhow::anyhow!("falied to verify password hash: {}", e).into(),
            })
    })
    .await
    .context("panic in verifying password hash")?
}