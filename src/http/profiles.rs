use axum::{extract::{Extension, Path}, Json, Router, routing::{get, post}};

use super::{extractor::{MaybeAuthUser, AuthUser}, ApiContext, Error, Result, types::DbBool};

pub fn router() -> Router {
    Router::new()
        .route("/api/profiles/:username", get(get_user_profile))
        .route(
            "/api/profiles/:username/follow",
            post(follow_user).delete(unfollow_user)
        )
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileBody {
    profile: Profile,
}

#[derive(serde::Serialize)]
pub struct Profile {
    pub username: String,
    pub bio: String,
    pub image: Option<String>,
    pub following: DbBool,
}

async fn get_user_profile(
    maybe_auth_user: MaybeAuthUser,
    ctx: Extension<ApiContext>,
    Path(username): Path<String>,
) -> Result<Json<ProfileBody>> {
    let profile = sqlx::query_as!(
        Profile,
        r#"
select username, bio, image, exists(
    select 1 from follow where followed_user_id = user.user_id and following_user_id = ?
) `following!:_`
from user
where username = ?
        "#,
        maybe_auth_user.user_id().map(|id| id.to_string()),
        username,
    )
        .fetch_optional(&ctx.db)
        .await?
        .ok_or(Error::NotFound)?;

    Ok(Json(ProfileBody{ profile }))
}

async fn follow_user(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
    Path(username): Path<String>,
) -> Result<Json<ProfileBody>> {
    let mut tx = ctx.db.begin().await?;

    let user = sqlx::query!(
        r#"
select user_id, username, bio, image from user where username = ?
        "#,
        username
    )
        .fetch_optional(&mut tx)
        .await?
        .ok_or(Error::NotFound)?;

    if user.user_id == auth_user.user_id.to_string() {
        return Err(Error::Forbidden);
    }

    sqlx::query!(
        r#"
insert ignore into follow(following_user_id, followed_user_id) values (?, ?)
        "#,
        auth_user.user_id.to_string(),
        user.user_id
    )
    .execute(&mut tx)
    .await?;

    tx.commit().await?;

    Ok(Json(ProfileBody { 
        profile: Profile {
            username: user.username,
            bio: user.bio,
            image: user.image,
            following: true.into(),
        },
     }))
}

async fn unfollow_user(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
    Path(username): Path<String>,
) -> Result<Json<ProfileBody>> {
    let mut tx = ctx.db.begin().await?;

    let user = sqlx::query!(
        r#"
select user_id, username, bio, image from user where username = ?
        "#,
        username
    )
        .fetch_optional(&mut tx)
        .await?
        .ok_or(Error::NotFound)?;

    sqlx::query!(
        r#"
delete from follow where following_user_id = ? and followed_user_id = ?
        "#,
        auth_user.user_id.to_string(),
        user.user_id.to_string()
    )
        .execute(&mut tx)
        .await?;

    tx.commit().await?;

    Ok(Json(ProfileBody { 
        profile: Profile {
            username: user.username,
            bio: user.bio,
            image: user.image,
            following: false.into(),
        }
     }))
}