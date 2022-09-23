use std::str::FromStr;

use anyhow::Context;
use axum::{Router, extract::{Extension, Path}, Json, routing::{post, get}};
use itertools::Itertools;
use sqlx::{MySql, Executor};
use uuid::Uuid;

use super::{types::{Timestamptz, DbBool}, profiles::Profile, extractor::{AuthUser, MaybeAuthUser}, ApiContext, ResultExt, Error};
use super::Result;

mod comments;
mod listing;

pub fn router() -> Router {
    Router::new()
        .route(
            "/api/articles",
            post(create_article).get(listing::list_articles),
        )
        .route("/api/articles/feed", get(listing::feed_articles))
        .route(
            "/api/articles/:slug",
            get(get_article).put(update_article).delete(delete_article),
        )
        .route(
            "/api/articles/:slug/favorite",
            post(favorite_article).delete(unfavorite_article),
        )
        .route("/api/tags", get(get_tags))
        .merge(comments::router())
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ArticleBody<T = Article> {
    article: T,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct TagsBody {
    tags: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct CreateArticle {
    title: String,
    description: String,
    body: String,
    tag_list: Vec<String>,
}

#[derive(serde::Deserialize)]
struct UpdateArticle {
    title: Option<String>,
    description: Option<String>,
    body: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Article {
    slug: String,
    title: String,
    description: String,
    body: String,
    tag_list: Vec<String>,
    created_at: Timestamptz,
    updated_at: Timestamptz,
    favorited: DbBool,
    favorites_count: i64,
    author: Profile,
}

struct ArticleFromQuery {
    slug: String,
    title: String,
    description: String,
    body: String,
    tag_list: serde_json::Value,
    created_at: Timestamptz,
    updated_at: Timestamptz,
    favorited: DbBool,
    favorites_count: i64,
    author_username: String,
    author_bio: String,
    author_image: Option<String>,
    following_author: DbBool,
}

impl ArticleFromQuery {
    fn into_article(self) -> Article {
        let tag_list = serde_json::from_value::<Vec<String>>(self.tag_list).unwrap_or_default();
        Article {
            slug: self.slug,
            title: self.title,
            description: self.description,
            body: self.body,
            tag_list,
            created_at: self.created_at,
            updated_at: self.updated_at,
            favorited: self.favorited,
            favorites_count: self.favorites_count,
            author: Profile {
                username: self.author_username,
                bio: self.author_bio,
                image: self.author_image,
                following: self.following_author,
            },
        }
    }
}

async fn create_article(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
    Json(mut req): Json<ArticleBody<CreateArticle>>,
) -> Result<Json<ArticleBody>> {
    let slug = slugify(&req.article.title);

    req.article.tag_list.sort();
    let tag_list = serde_json::to_value(req.article.tag_list).unwrap_or(serde_json::Value::Array(Vec::new()));

    let article_id = Uuid::new_v4();

    let mut tx = ctx.db.begin().await?;

    sqlx::query!(
        r#"
insert into article (article_id, slug, user_id, title, description, body, tag_list)
        values (?, ?, ?, ?, ?, ?, ?)
        "#,
        article_id.to_string(),
        slug,
        auth_user.user_id.to_string(),
        req.article.title,
        req.article.description,
        req.article.body,
        tag_list
    )
        .execute(&mut tx)
        .await
        .on_constraint("key_slug", |_| {
            Error::unprocessable_entity([("slug", format!("duplicate article slug: {}", slug))])
        })?;

    let article = article_by_id(&mut tx, Some(auth_user.user_id), article_id).await?;

    tx.commit().await?;

    Ok(Json(ArticleBody { article }))
}

fn slugify(string: &str) -> String {
    const QUOTE_CHARS: &[char] = &['\'', '"'];

    string
        .split(|c: char| !(QUOTE_CHARS.contains(&c) || c.is_alphabetic()))
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut s = s.replace(QUOTE_CHARS, "");
            s.make_ascii_lowercase();
            s
        })
        .join("-")
}

async fn update_article(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
    Path(slug): Path<String>,
    Json(req): Json<ArticleBody<UpdateArticle>>,
) -> Result<Json<ArticleBody>> {
    let mut tx = ctx.db.begin().await?;

    let new_slug = req.article.title.as_deref().map(slugify);

    let article_meta = sqlx::query!(
        r#"
select article_id, user_id from article where slug = ? for update
        "#,
        slug
    )
        .fetch_optional(&mut tx)
        .await?
        .ok_or(Error::NotFound)?;

    if article_meta.user_id != auth_user.user_id.to_string() {
        return Err(Error::Forbidden);
    }

    sqlx::query!(
        r#"
update article
set
    slug = coalesce(?, slug),
    title = coalesce(?, title),
    description = coalesce(?, description),
    body = coalesce(?, body)
where article_id = ?
        "#,
        new_slug,
        req.article.title,
        req.article.description,
        req.article.body,
        article_meta.article_id
    )
        .execute(&mut tx)
        .await
        .on_constraint("key_slug", |_| {
            Error::unprocessable_entity([(
                "slug",
                format!("duplicate article slug: {}", new_slug.unwrap()),
            )])
        })?;

    let article_id = Uuid::from_str(&article_meta.article_id).context("invalid uuid string")?;
    let article = article_by_id(&mut tx, Some(auth_user.user_id), article_id).await?;

    tx.commit().await?;

    Ok(Json(ArticleBody { article }))
}

async fn delete_article(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
    Path(slug): Path<String>,
) -> Result<()> {
    let mut tx = ctx.db.begin().await?;

    let article_meta = sqlx::query!(
        r#"
select article_id, user_id from article where slug = ? for update
        "#,
        slug
    )
        .fetch_optional(&mut tx)
        .await?
        .ok_or(Error::NotFound)?;

    if article_meta.user_id != auth_user.user_id.to_string() {
        return Err(Error::Forbidden);
    }

    sqlx::query!(
        r#"
delete from article where article_id = ?
        "#,
        article_meta.article_id
    )
        .execute(&mut tx)
        .await?;

    tx.commit().await?;

    Ok(())
}

async fn get_article(
    maybe_auth_user: MaybeAuthUser,
    ctx: Extension<ApiContext>,
    Path(slug): Path<String>,
) -> Result<Json<ArticleBody>> {
    let article = sqlx::query_as!(
        ArticleFromQuery,
        r#"
select
    article.slug,
    article.title,
    article.description,
    article.body,
    article.tag_list,
    article.created_at `created_at: Timestamptz`,
    article.updated_at `updated_at: Timestamptz`,
    exists(select 1 from article_favorite where user_id = ?) `favorited!:_`,
    coalesce(
        (select count(*) from article_favorite fav where fav.article_id = article.article_id),
        0
    ) `favorites_count!`,
    user.username author_username,
    user.bio author_bio,
    user.image author_image,
    0 `following_author:_`
from article
inner join user using (user_id)
where article.slug = ?
        "#,
        maybe_auth_user.user_id().map(|id| id.to_string()),
        slug
    )
        .fetch_optional(&ctx.db)
        .await?
        .ok_or(Error::NotFound)?
        .into_article();

    Ok(Json(ArticleBody { article }))
}

async fn favorite_article(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
    Path(slug): Path<String>,
) -> Result<Json<ArticleBody>> {
    let mut tx = ctx.db.begin().await?;

    let article_id = sqlx::query_scalar!(
        r#"
select article_id from article where slug = ?
        "#,
        slug
    )
        .fetch_optional(&mut tx)
        .await?
        .ok_or(Error::NotFound)?;
    
    sqlx::query!(
        r#"
insert ignore into article_favorite(article_id, user_id)
values (?, ?)
        "#,
        article_id,
        auth_user.user_id.to_string()
    )
        .execute(&mut tx)
        .await?;

    let article_id = Uuid::from_str(&article_id).context("invalid uuid string")?;
    let article = article_by_id(&mut tx, Some(auth_user.user_id), article_id).await?;

    tx.commit().await?;

    Ok(Json(ArticleBody { article }))
}

async fn unfavorite_article(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
    Path(slug): Path<String>,
) -> Result<Json<ArticleBody>> {
    let mut tx = ctx.db.begin().await?;

    let article_id = sqlx::query_scalar!(
        r#"
select article_id from article where slug = ?
        "#,
        slug
    )
        .fetch_optional(&mut tx)
        .await?
        .ok_or(Error::NotFound)?;
    
    sqlx::query!(
        r#"
delete from article_favorite where article_id = ? and user_id = ?
        "#,
        article_id,
        auth_user.user_id.to_string()
    )
        .execute(&mut tx)
        .await?;

    let article_id = Uuid::from_str(&article_id).context("invalid uuid string")?;
    let article = article_by_id(&mut tx, Some(auth_user.user_id), article_id).await?;

    tx.commit().await?;

    Ok(Json(ArticleBody { article }))
}

#[allow(unused_variables)]
async fn get_tags(
    ctx: Extension<ApiContext>,
) -> Result<Json<TagsBody>> {
    todo!("not easy to implement on mysql using json data type")
}

async fn article_by_id(
    e: impl Executor<'_, Database = MySql>,
    user_id: Option<Uuid>,
    article_id: Uuid,
) -> Result<Article> {
    let article = sqlx::query_as!(
        ArticleFromQuery,
        r#"
select
    article.slug,
    article.title,
    article.description,
    article.body,
    article.tag_list,
    article.created_at `created_at: Timestamptz`,
    article.updated_at `updated_at: Timestamptz`,
    exists(select 1 from article_favorite where user_id = ?) `favorited!:_`,
    coalesce(
        (select count(*) from article_favorite fav where fav.article_id = article.article_id),
        0
    ) `favorites_count!`,
    user.username author_username,
    user.bio author_bio,
    user.image author_image,
    0 `following_author:_`
from article
inner join user using (user_id)
where article.article_id = ?
        "#,
        user_id.map(|id| id.to_string()),
        article_id.to_string()
    )
        .fetch_optional(e)
        .await?
        .ok_or(Error::NotFound)?
        .into_article();
    
    Ok(article)
}